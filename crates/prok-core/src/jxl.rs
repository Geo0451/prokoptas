use std::io::Cursor;

use image::ImageReader;

use crate::{
    BitDepth, ColorSpace, DecodeOptions, DecodedImage, Decoder, EncodeOptions, Encoder, Error,
    FormatRegistry, FormatTag, ImageMetadata, LosslessCapability, PixelBuffer, Result,
};

pub struct JxlDecoder;
pub struct JxlEncoder;

pub static JXL_DECODER: JxlDecoder = JxlDecoder;
pub static JXL_ENCODER: JxlEncoder = JxlEncoder;
pub static JXL_DECODERS: [&dyn Decoder; 1] = [&JXL_DECODER];
pub static JXL_ENCODERS: [&dyn Encoder; 1] = [&JXL_ENCODER];
pub static JXL_REGISTRY: FormatRegistry<'static> =
    FormatRegistry::new(&JXL_DECODERS, &JXL_ENCODERS);

impl Decoder for JxlDecoder {
    fn format(&self) -> FormatTag {
        FormatTag::Jxl
    }

    fn probe(&self, input: &[u8]) -> bool {
        input.len() >= 2 && input[0] == 0xFF && input[1] == 0x0A
    }

    fn decode_native(&self, input: &[u8], _options: &DecodeOptions) -> Result<DecodedImage> {
        jxl_oxide::integration::register_image_decoding_hook();
        let header = jxl_oxide::JxlImage::builder()
            .read(Cursor::new(input))
            .map_err(corrupt)?;
        let width = header.width();
        let height = header.height();
        enforce_memory_limit(width, height, _options.memory_limit_mb)?;
        drop(header);
        let image = ImageReader::new(Cursor::new(input))
            .with_guessed_format()
            .map_err(corrupt)?
            .decode()
            .map_err(corrupt)?;
        let rgba = image.to_rgba8();
        let width = image.width();
        let height = image.height();
        let pixels = PixelBuffer::rgba8(rgba.into_raw(), width, height)?;
        let mut decoded =
            DecodedImage::new(pixels, width, height, ColorSpace::Srgb, BitDepth::Eight)?;
        decoded.metadata = ImageMetadata::default();
        Ok(decoded)
    }
}

impl Encoder for JxlEncoder {
    fn format(&self) -> FormatTag {
        FormatTag::Jxl
    }

    fn lossless_capability(&self) -> LosslessCapability {
        LosslessCapability::Configurable
    }

    fn encode_native(&self, image: &DecodedImage, options: &EncodeOptions) -> Result<Vec<u8>> {
        options.validate(self)?;
        let rgba = rgba_to_rgba8(image)?;

        let output = match options.compression {
            crate::Compression::Lossless => jxl_encoder::LosslessConfig::new()
                .with_effort(options.effort.min(10))
                .encode(
                    &rgba,
                    image.width,
                    image.height,
                    jxl_encoder::PixelLayout::Rgba8,
                )
                .map_err(|error| Error::EncodingFailed {
                    message: error.to_string(),
                })?,
            crate::Compression::Lossy { quality } => {
                let distance = jxl_encoder::quality_to_distance(f32::from(quality));
                let config = jxl_encoder::LossyConfig::new(distance)
                    .with_effort(options.effort.min(10))
                    .with_noise(options.jxl_noise_synthesis)
                    .with_gaborish(options.jxl_gaborish);

                config
                    .encode(
                        &rgba,
                        image.width,
                        image.height,
                        jxl_encoder::PixelLayout::Rgba8,
                    )
                    .map_err(|error| Error::EncodingFailed {
                        message: error.to_string(),
                    })?
            }
        };

        Ok(output)
    }
}

fn corrupt<E: std::fmt::Display>(error: E) -> Error {
    Error::CorruptData {
        message: error.to_string(),
    }
}

fn enforce_memory_limit(width: u32, height: u32, limit_mb: Option<u64>) -> Result<()> {
    let bytes = (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| Error::CorruptData {
            message: "JPEG XL image dimensions overflowed".to_owned(),
        })?;
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

fn rgba_to_rgba8(image: &DecodedImage) -> Result<Vec<u8>> {
    let PixelBuffer::Rgba8(pixels) = &image.pixels else {
        return Err(Error::InvalidOptions {
            message: "JPEG XL encoding supports RGBA8 images only".to_owned(),
        });
    };
    Ok(pixels.clone())
}

#[cfg(test)]
mod tests {
    use super::{JxlDecoder, JxlEncoder};
    use crate::{
        BitDepth, ColorSpace, Compression, DecodedImage, Decoder, Encoder, LosslessCapability,
        PixelBuffer,
    };

    fn fixture() -> DecodedImage {
        DecodedImage::new(
            PixelBuffer::rgba8(vec![255, 0, 0, 255, 0, 255, 0, 255], 2, 1).expect("valid fixture"),
            2,
            1,
            ColorSpace::Srgb,
            BitDepth::Eight,
        )
        .expect("valid image")
    }

    #[test]
    fn jxl_probe_matches_known_header_sequence() {
        assert!(JxlDecoder.probe(&[0xFF, 0x0A, 0x00, 0x70, 0xB0, 0x12]));
    }

    #[test]
    fn jxl_reports_lossless_capability_and_encodes() {
        assert_eq!(
            JxlEncoder.lossless_capability(),
            LosslessCapability::Configurable
        );
        let result = JxlEncoder.encode(
            &fixture(),
            &crate::EncodeOptions {
                compression: Compression::Lossless,
                ..crate::EncodeOptions::default()
            },
        );
        assert!(result.is_ok());
        assert!(!result.unwrap().is_empty());
    }
}
