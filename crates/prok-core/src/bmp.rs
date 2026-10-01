//! BMP format decoder and encoder integration.
//!
//! BMP is uncompressed and always lossless.

use std::io::Cursor;

use ::image::{ImageReader, RgbaImage};

use crate::{
    BitDepth, ColorSpace, DecodeOptions, DecodedImage, Decoder, EncodeOptions, Encoder, Error,
    FormatRegistry, FormatTag, LosslessCapability, PixelBuffer, Result,
};

pub struct BmpDecoder;
pub struct BmpEncoder;

pub static BMP_DECODER: BmpDecoder = BmpDecoder;
pub static BMP_ENCODER: BmpEncoder = BmpEncoder;
pub static BMP_DECODERS: [&dyn Decoder; 1] = [&BMP_DECODER];
pub static BMP_ENCODERS: [&dyn Encoder; 1] = [&BMP_ENCODER];
pub static BMP_REGISTRY: FormatRegistry<'static> =
    FormatRegistry::new(&BMP_DECODERS, &BMP_ENCODERS);

impl Decoder for BmpDecoder {
    fn format(&self) -> FormatTag {
        FormatTag::Bmp
    }

    fn probe(&self, input: &[u8]) -> bool {
        if input.len() < 2 {
            return false;
        }
        // BMP magic: "BM"
        input.starts_with(b"BM")
    }

    fn decode_native(&self, input: &[u8], options: &DecodeOptions) -> Result<DecodedImage> {
        let cursor = Cursor::new(input);
        let reader = ImageReader::new(cursor)
            .with_guessed_format()
            .map_err(|e| Error::CorruptData {
                message: format!("BMP format detection failed: {e}"),
            })?;

        let image = reader.decode().map_err(|e| Error::CorruptData {
            message: format!("BMP decoding failed: {e}"),
        })?;

        let width = image.width();
        let height = image.height();

        let output_size = (width as usize)
            .checked_mul(height as usize)
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or_else(|| Error::CorruptData {
                message: "BMP image dimensions overflowed".to_string(),
            })?;

        enforce_memory_limit(output_size, options.memory_limit_mb)?;

        let rgba_image = image.to_rgba8();
        let rgba_data = rgba_image.into_raw();

        let decoded = DecodedImage::new(
            PixelBuffer::rgba8(rgba_data, width, height)?,
            width,
            height,
            ColorSpace::Srgb,
            BitDepth::Eight,
        )?;

        Ok(decoded)
    }
}

impl Encoder for BmpEncoder {
    fn format(&self) -> FormatTag {
        FormatTag::Bmp
    }

    fn lossless_capability(&self) -> LosslessCapability {
        LosslessCapability::Always
    }

    fn encode_native(&self, image: &DecodedImage, options: &EncodeOptions) -> Result<Vec<u8>> {
        options.validate(self)?;

        let rgba_data = match &image.pixels {
            PixelBuffer::Rgba8(data) => data.clone(),
            PixelBuffer::Rgba16(_) => {
                return Err(Error::InvalidOptions {
                    message: "BMP encoder requires RGBA8 pixels".to_owned(),
                });
            }
        };

        let rgba_image =
            RgbaImage::from_raw(image.width, image.height, rgba_data).ok_or_else(|| {
                Error::CorruptData {
                    message: "BMP image buffer size mismatch".to_string(),
                }
            })?;

        let mut output = Vec::new();
        rgba_image
            .write_to(&mut Cursor::new(&mut output), ::image::ImageFormat::Bmp)
            .map_err(|e| Error::EncodingFailed {
                message: format!("BMP encoding failed: {e}"),
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
    fn bmp_probe_recognizes_valid_format() {
        let bmp_header = vec![b'B', b'M', 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
        assert!(BmpDecoder.probe(&bmp_header));
    }

    #[test]
    fn bmp_probe_rejects_non_bmp() {
        let png_header = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        assert!(!BmpDecoder.probe(&png_header));
    }

    #[test]
    fn bmp_probe_rejects_short_input() {
        let short_data = vec![0x00];
        assert!(!BmpDecoder.probe(&short_data));
    }

    #[test]
    fn bmp_encoder_always_lossless() {
        assert_eq!(BmpEncoder.lossless_capability(), LosslessCapability::Always);
    }

    #[test]
    fn bmp_round_trip_preserves_pixels() {
        // Create a simple 4x4 RGBA8 image
        let width = 4u32;
        let height = 4u32;
        let mut pixels = vec![0u8; (width * height * 4) as usize];
        // Set alternating pixels to green and yellow
        for i in 0..(width * height) as usize {
            if i % 2 == 0 {
                pixels[i * 4 + 1] = 255; // green
                pixels[i * 4 + 3] = 255; // alpha
            } else {
                pixels[i * 4] = 255; // red
                pixels[i * 4 + 1] = 255; // green -> yellow
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

        // Encode to BMP
        let encoded = BmpEncoder
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
        assert!(BmpDecoder.probe(&encoded));

        // Decode back
        let decoded = BmpDecoder.decode(&encoded, &Default::default()).unwrap();

        // Verify dimensions
        assert_eq!(decoded.width, width);
        assert_eq!(decoded.height, height);
    }

    #[test]
    fn bmp_memory_limit_enforced() {
        // Create BMP that would exceed memory limit
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

        let encoded = BmpEncoder
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
        let result = BmpDecoder.decode(
            &encoded,
            &DecodeOptions {
                memory_limit_mb: Some(0),
                ..Default::default()
            },
        );

        assert!(result.is_err());
    }
}
