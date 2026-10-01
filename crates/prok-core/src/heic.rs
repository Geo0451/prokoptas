//! HEIC/HEIF format decoder integration.
//!
//! # Patent and Licensing Notice
//! HEIC decoding utilizes the pure-Rust `heic` crate. HEIC relies on HEVC (H.265) video coding,
//! which is subject to third-party patent portfolios. The open-source software license does NOT grant
//! HEVC patent rights. Prokoptas provides HEIC as an engineering-supported decode input format
//! without claiming patent freedom. Commercial or public releases must undergo separate patent review.

use crate::{
    BitDepth, ColorSpace, DecodeOptions, DecodedImage, Decoder, Error, FormatRegistry, FormatTag,
    ImageMetadata, PixelBuffer, Result,
};

pub struct HeicDecoder;

pub static HEIC_DECODER: HeicDecoder = HeicDecoder;
pub static HEIC_DECODERS: [&dyn Decoder; 1] = [&HEIC_DECODER];
pub static HEIC_REGISTRY: FormatRegistry<'static> = FormatRegistry::new(&HEIC_DECODERS, &[]);

impl Decoder for HeicDecoder {
    fn format(&self) -> FormatTag {
        FormatTag::Heif
    }

    fn probe(&self, input: &[u8]) -> bool {
        if input.len() < 12 {
            return false;
        }
        if heic::ImageInfo::from_bytes(input).is_ok() {
            return true;
        }
        if input.get(4..8) == Some(b"ftyp") {
            matches!(
                input.get(8..12),
                Some(b"heic")
                    | Some(b"heix")
                    | Some(b"hevc")
                    | Some(b"hevx")
                    | Some(b"mif1")
                    | Some(b"msf1")
                    | Some(b"mim1")
                    | Some(b"heis")
                    | Some(b"hevm")
                    | Some(b"hevs")
            )
        } else {
            false
        }
    }

    fn decode_native(&self, input: &[u8], options: &DecodeOptions) -> Result<DecodedImage> {
        let info = heic::ImageInfo::from_bytes(input).map_err(|e| Error::CorruptData {
            message: format!("HEIC metadata parse failed: {e}"),
        })?;

        let output_size = (info.width as usize)
            .checked_mul(info.height as usize)
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or_else(|| Error::CorruptData {
                message: "HEIC image dimensions overflowed".to_string(),
            })?;

        enforce_memory_limit(output_size, options.memory_limit_mb)?;

        let decoder_config = heic::DecoderConfig::new();
        let limits = options.memory_limit_mb.map(|allowed_mb| {
            let mut l = heic::Limits::default();
            l.max_pixels = Some(allowed_mb.saturating_mul(1024 * 1024 / 4));
            l
        });

        let mut decode_req = decoder_config
            .decode_request(input)
            .with_output_layout(heic::PixelLayout::Rgba8);

        if let Some(limits_ref) = &limits {
            decode_req = decode_req.with_limits(limits_ref);
        }

        let output = decode_req.decode().map_err(|e| Error::CorruptData {
            message: format!("HEIC decoding failed: {e}"),
        })?;

        let color_space = if info.color_primaries == 12 {
            ColorSpace::DisplayP3
        } else {
            ColorSpace::Srgb
        };

        let mut decoded_image = DecodedImage::new(
            PixelBuffer::rgba8(output.data, output.width, output.height)?,
            output.width,
            output.height,
            color_space,
            BitDepth::Eight,
        )?;

        let exif = info.exif.filter(|data| !data.is_empty());
        decoded_image.metadata = ImageMetadata {
            exif,
            xmp: info.xmp.filter(|data| !data.is_empty()),
            iptc: None,
        };

        Ok(decoded_image)
    }
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
    use crate::Encoder;

    #[test]
    fn heic_probe_recognizes_valid_heic_format() {
        let heic_header = vec![
            0x00, 0x00, 0x00, 0x20, b'f', b't', b'y', b'p', b'h', b'e', b'i',
            b'c', // Valid HEIC
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ];
        assert!(HeicDecoder.probe(&heic_header));
    }

    #[test]
    fn heic_probe_recognizes_heix_variant() {
        let heix_header = vec![
            0x00, 0x00, 0x00, 0x20, b'f', b't', b'y', b'p', b'h', b'e', b'i', b'x', 0x00, 0x00,
            0x00, 0x00,
        ];
        assert!(HeicDecoder.probe(&heix_header));
    }

    #[test]
    fn heic_probe_rejects_invalid_format() {
        let invalid_header = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]; // PNG
        assert!(!HeicDecoder.probe(&invalid_header));
    }

    #[test]
    fn heic_probe_rejects_short_input() {
        let short_data = vec![0x00, 0x01, 0x02];
        assert!(!HeicDecoder.probe(&short_data));
    }

    #[test]
    fn heic_decode_rejects_corrupt_data() {
        let corrupt_data = vec![
            0x00, 0x00, 0x00, 0x20, b'f', b't', b'y', b'p', b'h', b'e', b'i', b'c', 0x00, 0x00,
            0x00, 0x00, 0x01, 0x02, 0x03, 0x04,
        ];
        let result = HeicDecoder.decode(&corrupt_data, &DecodeOptions::default());
        assert!(matches!(result, Err(Error::CorruptData { .. })));
    }

    #[test]
    fn heic_real_sample_decodes_and_converts_to_output_formats() {
        let sample_path =
            "/home/g_kj/Pictures/iqoobkup/Whatsapp Documents/IMG_20260822_094940.HEIC";
        if let Ok(input) = std::fs::read(sample_path) {
            assert!(HeicDecoder.probe(&input));
            let decoded = HeicDecoder
                .decode(&input, &DecodeOptions::default())
                .expect("decode real HEIC sample");
            assert!(decoded.width > 0 && decoded.height > 0);
            assert_eq!(decoded.bit_depth, BitDepth::Eight);

            // Verify conversion to PNG
            let png_bytes = crate::PngEncoder
                .encode(
                    &decoded,
                    &crate::EncodeOptions {
                        compression: crate::Compression::Lossless,
                        bit_depth: BitDepth::Eight,
                        ..Default::default()
                    },
                )
                .expect("encode decoded HEIC to PNG");
            assert!(crate::PngDecoder.probe(&png_bytes));

            // Verify conversion to JPEG
            let jpeg_bytes = crate::JpegEncoder
                .encode(
                    &decoded,
                    &crate::EncodeOptions {
                        compression: crate::Compression::Lossy { quality: 80 },
                        bit_depth: BitDepth::Eight,
                        ..Default::default()
                    },
                )
                .expect("encode decoded HEIC to JPEG");
            assert!(crate::JpegDecoder.probe(&jpeg_bytes));

            // Verify conversion to WebP
            let webp_bytes = crate::WebpEncoder
                .encode(
                    &decoded,
                    &crate::EncodeOptions {
                        compression: crate::Compression::Lossy { quality: 80 },
                        bit_depth: BitDepth::Eight,
                        ..Default::default()
                    },
                )
                .expect("encode decoded HEIC to WebP");
            assert!(crate::WebpDecoder.probe(&webp_bytes));
        }
    }
}
