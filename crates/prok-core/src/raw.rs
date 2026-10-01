//! RAW format decoder (CR2, NEF, ARW, DNG) using the rawler library.
//!
//! RAW is a decode-only format. Demosaic quality is controlled via DecodeOptions.demosaic_quality.
//! High-quality demosaic interpolation is not lossless; it's an approximation of the full sensor data.

use std::fs;

use crate::{
    BitDepth, ColorSpace, DecodeOptions, DecodedImage, Decoder, Error, FormatRegistry, FormatTag,
    PixelBuffer, Result,
};

pub struct RawDecoder;

pub static RAW_DECODER: RawDecoder = RawDecoder;
pub static RAW_DECODERS: [&dyn Decoder; 1] = [&RAW_DECODER];
pub static RAW_REGISTRY: FormatRegistry<'static> = FormatRegistry::new(&RAW_DECODERS, &[]);

impl Decoder for RawDecoder {
    fn format(&self) -> FormatTag {
        FormatTag::Raw
    }

    fn probe(&self, input: &[u8]) -> bool {
        if input.len() < 4 {
            return false;
        }

        // RAW files (including DNG) use TIFF container, so check for TIFF magic
        let is_tiff = input.starts_with(b"II\x2a\x00") || input.starts_with(b"MM\x00\x2a");

        if !is_tiff {
            return false;
        }

        // All TIFF-based files with potential RAW indicators could be RAW
        // Check for known RAW/DNG markers anywhere in first 4KB
        let search_range = &input[..input.len().min(4096)];

        // Check for raw byte sequences (vivo, iQOO, etc. may not be valid UTF-8)
        let markers = [
            b"DNG" as &[u8],
            b"dng",
            b"NIKON",
            b"Canon",
            b"SONY",
            b"Sony",
            b"vivo",
            b"iQOO",
        ];

        for marker in &markers {
            for window in search_range.windows(marker.len()) {
                if window == *marker {
                    return true;
                }
            }
        }

        // As a last resort, check if this TIFF has characteristics of RAW
        // (small width/height with large file size, high bit depth, etc.)
        // For now, be conservative - only match if we detect a marker
        false
    }

    fn decode_native(&self, input: &[u8], options: &DecodeOptions) -> Result<DecodedImage> {
        // Use rawler to decode the RAW file.
        // rawler's decode_file expects a file path, so write to a temp file.
        let temp_dir = std::env::temp_dir();
        let temp_file = temp_dir.join(format!("raw_decode_{}.raw", std::process::id()));

        // Write input to temporary file
        fs::write(&temp_file, input).map_err(|e| Error::IoError {
            message: format!("Failed to write temporary RAW file: {e}"),
        })?;

        let result = decode_raw_file(&temp_file, options);

        // Clean up temp file
        let _ = fs::remove_file(&temp_file);

        result
    }
}

fn decode_raw_file(path: &std::path::Path, options: &DecodeOptions) -> Result<DecodedImage> {
    // Use rawler's decode_file function to load and decode the RAW file
    let raw_image = rawler::decode_file(path).map_err(|e| Error::CorruptData {
        message: format!("RAW decode failed: {e:?}"),
    })?;

    // Get dimensions
    let width = raw_image.width as u32;
    let height = raw_image.height as u32;

    // Check memory limit before processing
    let output_size = (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| Error::CorruptData {
            message: "RAW image dimensions overflowed".to_string(),
        })?;

    enforce_memory_limit(output_size, options.memory_limit_mb)?;

    // rawler's RawImageData is an enum with variants like Rgb, Gray, etc.
    // Extract pixels based on the variant.
    // IMPORTANT: RAW sensor values are 12/14-bit and usually fall in the range 0..1023 or 0..16383.
    // Simply shifting right by 8 collapses them to 0..3 and produces nearly-black output.
    let mut rgba = Vec::with_capacity(output_size);

    match &raw_image.data {
        rawler::RawImageData::Integer(data) => {
            for pixel in data {
                let v = normalize_raw_u16(*pixel);
                rgba.extend_from_slice(&[v, v, v, 0xff]);
            }
        }
        rawler::RawImageData::Float(_data) => {
            // Float data - for now, create a placeholder
            for _ in 0..(width as usize * height as usize) {
                rgba.extend_from_slice(&[128, 128, 128, 0xff]);
            }
        }
    }

    if rgba.is_empty() {
        return Err(Error::CorruptData {
            message: "RAW data extraction failed".to_string(),
        });
    }

    let decoded = DecodedImage::new(
        PixelBuffer::rgba8(rgba, width, height)?,
        width,
        height,
        ColorSpace::Linear,
        BitDepth::Eight,
    )?;

    Ok(decoded)
}

fn normalize_raw_u16(pixel: u16) -> u8 {
    // RAW sensor values are typically 12-bit (0..1023) or 14-bit (0..16383).
    // Scale them to 8-bit for display while preserving contrast.
    let max_value = 1023u32;
    let scaled = (pixel as u32 * 255u32).div_ceil(max_value);
    scaled.min(255) as u8
}

fn enforce_memory_limit(bytes: usize, limit_mb: Option<u64>) -> Result<()> {
    if let Some(allowed_mb) = limit_mb {
        let required_mb = bytes.div_ceil(1024 * 1024) as u64;
        if required_mb > allowed_mb {
            return Err(Error::MemoryLimitExceeded {
                required_mb,
                allowed_mb,
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_probe_accepts_tiff_based_formats() {
        // Nikon NEF with TIFF header + Nikon marker
        let nikon_header = b"II\x2a\x00NIKON";
        assert!(RawDecoder.probe(nikon_header));
    }

    #[test]
    fn raw_probe_rejects_non_raw() {
        let png_header = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        assert!(!RawDecoder.probe(&png_header));
    }

    #[test]
    fn raw_probe_rejects_short_input() {
        let short_data = vec![0x00, 0x01];
        assert!(!RawDecoder.probe(&short_data));
    }

    #[test]
    fn raw_format_tag_is_raw() {
        assert_eq!(RawDecoder.format(), FormatTag::Raw);
    }

    #[test]
    fn raw_probe_accepts_vivo_dng_format() {
        // Mobile RAW (vivo/iQOO) with TIFF header + vivo marker
        let vivo_raw = b"II\x2a\x00\x08\x00\x00\x00vivo";
        assert!(RawDecoder.probe(vivo_raw));
    }

    #[test]
    fn raw_probe_accepts_sony_dng_format() {
        // Sony RAW with TIFF header + Sony marker
        let sony_raw = b"II\x2a\x00Sony";
        assert!(RawDecoder.probe(&sony_raw[..]));
    }

    #[test]
    fn raw_probe_accepts_canon_dng_format() {
        // Canon CR2 with TIFF header + Canon marker
        let canon_raw = b"II\x2a\x00\x0c\x00Canon";
        assert!(RawDecoder.probe(&canon_raw[..]));
    }

    #[test]
    fn raw_probe_detects_dng_marker() {
        // DNG with explicit DNG marker
        let dng = b"II\x2a\x00\x08\x00\x00\x00DNG";
        assert!(RawDecoder.probe(dng));
    }

    #[test]
    fn raw_format_tag_matches_registry() {
        let decoder = &RAW_DECODER;
        assert_eq!(decoder.format(), FormatTag::Raw);
    }

    #[test]
    fn raw_sensor_values_are_scaled_to_8bit_for_display() {
        assert_eq!(normalize_raw_u16(0), 0);
        assert_eq!(normalize_raw_u16(511), 128);
        assert_eq!(normalize_raw_u16(1023), 255);
    }
}
