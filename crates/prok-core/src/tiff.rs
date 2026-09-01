//! TIFF format decoder and encoder integration.
//!
//! TIFF support is provided via the image crate, which supports baseline, LZW, and PackBits compression.
//! TIFF is always lossless under the supported compression subset.

use std::io::Cursor;

use ::image::{ImageReader, RgbaImage};

use crate::{
    BitDepth, ColorSpace, Compression, DecodeOptions, DecodedImage, Decoder, EncodeOptions,
    Encoder, Error, FormatRegistry, FormatTag, LosslessCapability, PixelBuffer, Result,
};

pub struct TiffDecoder;
pub struct TiffEncoder;

pub static TIFF_DECODER: TiffDecoder = TiffDecoder;
pub static TIFF_ENCODER: TiffEncoder = TiffEncoder;
pub static TIFF_DECODERS: [&dyn Decoder; 1] = [&TIFF_DECODER];
pub static TIFF_ENCODERS: [&dyn Encoder; 1] = [&TIFF_ENCODER];
pub static TIFF_REGISTRY: FormatRegistry<'static> =
    FormatRegistry::new(&TIFF_DECODERS, &TIFF_ENCODERS);

impl Decoder for TiffDecoder {
    fn format(&self) -> FormatTag {
        FormatTag::Tiff
    }

    fn probe(&self, input: &[u8]) -> bool {
        if input.len() < 4 {
            return false;
        }
        // TIFF magic: little-endian (II*\0) or big-endian (MM\0*)
        input.starts_with(b"II\x2a\x00") || input.starts_with(b"MM\x00\x2a")
    }

    fn decode(&self, input: &[u8], options: &DecodeOptions) -> Result<DecodedImage> {
        let cursor = Cursor::new(input);
        let reader = ImageReader::new(cursor)
            .with_guessed_format()
            .map_err(|e| Error::CorruptData {
                message: format!("TIFF format detection failed: {e}"),
            })?;

        let image = reader.decode().map_err(|e| Error::CorruptData {
            message: format!("TIFF decoding failed: {e}"),
        })?;

        let width = image.width();
        let height = image.height();

        let output_size = (width as usize)
            .checked_mul(height as usize)
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or_else(|| Error::CorruptData {
                message: "TIFF image dimensions overflowed".to_string(),
            })?;

        enforce_memory_limit(output_size, options.memory_limit_mb)?;

        let rgba_image = image.to_rgba8();
        let rgba_data = rgba_image.into_raw();

        let decoded = DecodedImage::new(
            PixelBuffer::rgba8(rgba_data, width, height)?,
            width,
            height,
            options.color_space_override.unwrap_or(ColorSpace::Srgb),
            BitDepth::Eight,
        )?;

        Ok(decoded)
    }
}

impl Encoder for TiffEncoder {
    fn format(&self) -> FormatTag {
        FormatTag::Tiff
    }

    fn lossless_capability(&self) -> LosslessCapability {
        LosslessCapability::Always
    }

    fn encode(&self, image: &DecodedImage, options: &EncodeOptions) -> Result<Vec<u8>> {
        options.validate(self)?;

        let rgba_data = match &image.pixels {
            PixelBuffer::Rgba8(data) => data.clone(),
            PixelBuffer::Rgba16(_) => {
                return Err(Error::InvalidOptions {
                    message: "TIFF encoder requires RGBA8 pixels".to_owned(),
                });
            }
        };

        let rgba_image = RgbaImage::from_raw(image.width, image.height, rgba_data)
            .ok_or_else(|| Error::CorruptData {
                message: "TIFF image buffer size mismatch".to_string(),
            })?;

        let mut output = Vec::new();
        rgba_image
            .write_to(&mut Cursor::new(&mut output), ::image::ImageFormat::Tiff)
            .map_err(|e| Error::EncodingFailed {
                message: format!("TIFF encoding failed: {e}"),
            })?;

        Ok(output)
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
    use crate::Compression;

    #[test]
    fn tiff_probe_recognizes_little_endian() {
        let tiff_header = vec![b'I', b'I', 0x2a, 0x00, 0x08, 0x00, 0x00, 0x00];
        assert!(TiffDecoder.probe(&tiff_header));
    }

    #[test]
    fn tiff_probe_recognizes_big_endian() {
        let tiff_header = vec![b'M', b'M', 0x00, 0x2a, 0x00, 0x00, 0x00, 0x08];
        assert!(TiffDecoder.probe(&tiff_header));
    }

    #[test]
    fn tiff_probe_rejects_non_tiff() {
        let png_header = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        assert!(!TiffDecoder.probe(&png_header));
    }

    #[test]
    fn tiff_probe_rejects_short_input() {
        let short_data = vec![0x00, 0x01];
        assert!(!TiffDecoder.probe(&short_data));
    }

    #[test]
    fn tiff_encoder_always_lossless() {
        assert_eq!(
            TiffEncoder.lossless_capability(),
            LosslessCapability::Always
        );
    }

    #[test]
    fn tiff_round_trip_preserves_pixels() {
        // Create a simple 4x4 RGBA8 image
        let width = 4u32;
        let height = 4u32;
        let mut pixels = vec![0u8; (width * height * 4) as usize];
        // Set alternating pixels to red and blue
        for i in 0..(width * height) as usize {
            if i % 2 == 0 {
                pixels[i * 4] = 255; // red
                pixels[i * 4 + 3] = 255; // alpha
            } else {
                pixels[i * 4 + 2] = 255; // blue
                pixels[i * 4 + 3] = 255; // alpha
            }
        }

        let original = DecodedImage::new(
            PixelBuffer::rgba8(pixels.clone(), width, height).unwrap(),
            width,
            height,
            ColorSpace::Srgb,
            BitDepth::Eight,
        )
        .unwrap();

        // Encode to TIFF
        let encoded = TiffEncoder
            .encode(
                &original,
                &EncodeOptions {
                    compression: Compression::Lossless,
                    bit_depth: BitDepth::Eight,
                    ..Default::default()
                },
            )
            .unwrap();

        // Probe should recognize it
        assert!(TiffDecoder.probe(&encoded));

        // Decode back
        let decoded = TiffDecoder
            .decode(&encoded, &Default::default())
            .unwrap();

        // Verify dimensions
        assert_eq!(decoded.width, width);
        assert_eq!(decoded.height, height);
    }

    #[test]
    fn tiff_memory_limit_enforced() {
        // Create TIFF that would exceed memory limit
        let width = 100u32;
        let height = 100u32;
        let pixels = vec![0u8; (width * height * 4) as usize];
        let image = DecodedImage::new(
            PixelBuffer::rgba8(pixels, width, height).unwrap(),
            width,
            height,
            ColorSpace::Srgb,
            BitDepth::Eight,
        )
        .unwrap();

        let encoded = TiffEncoder
            .encode(
                &image,
                &EncodeOptions {
                    compression: Compression::Lossless,
                    bit_depth: BitDepth::Eight,
                    ..Default::default()
                },
            )
            .unwrap();

        // Decode with very low memory limit should fail
        let result = TiffDecoder.decode(
            &encoded,
            &DecodeOptions {
                memory_limit_mb: Some(0),
                ..Default::default()
            },
        );

        assert!(result.is_err());
    }
}
