use zenpixels::{PixelBuffer as ZenPixelBuffer, PixelDescriptor};

use crate::{
    BitDepth, ColorSpace, DecodeOptions, DecodedImage, Decoder, EncodeOptions, Encoder, Error,
    FormatRegistry, FormatTag, ImageMetadata, LosslessCapability, PixelBuffer, Result,
};

pub struct AvifDecoder;
pub struct AvifEncoder;

pub static AVIF_DECODER: AvifDecoder = AvifDecoder;
pub static AVIF_ENCODER: AvifEncoder = AvifEncoder;
pub static AVIF_DECODERS: [&dyn Decoder; 1] = [&AVIF_DECODER];
pub static AVIF_ENCODERS: [&dyn Encoder; 1] = [&AVIF_ENCODER];
pub static AVIF_REGISTRY: FormatRegistry<'static> =
    FormatRegistry::new(&AVIF_DECODERS, &AVIF_ENCODERS);

impl Decoder for AvifDecoder {
    fn format(&self) -> FormatTag {
        FormatTag::Avif
    }

    fn probe(&self, input: &[u8]) -> bool {
        input.len() >= 12
            && input.windows(4).any(|window| window == b"ftyp")
            && input[input.len().min(8)..]
                .windows(4)
                .any(|window| window == b"avif")
    }

    fn decode_native(&self, input: &[u8], options: &DecodeOptions) -> Result<DecodedImage> {
        let decoded = zenavif::decode(input).map_err(corrupt)?;
        let width = decoded.width();
        let height = decoded.height();
        let descriptor = decoded.descriptor();
        let decoded_pixels = decoded.into_vec();
        let rgba = if descriptor == PixelDescriptor::RGBA8 {
            decoded_pixels
        } else if descriptor == PixelDescriptor::RGB8 {
            rgb_to_rgba(&decoded_pixels)?
        } else {
            return Err(Error::CorruptData {
                message: format!("AVIF decoder returned unsupported pixel layout {descriptor:?}"),
            });
        };
        enforce_memory_limit(rgba.len(), options.memory_limit_mb)?;
        let pixels = PixelBuffer::rgba8(rgba, width, height)?;
        let mut decoded =
            DecodedImage::new(pixels, width, height, ColorSpace::Srgb, BitDepth::Eight)?;
        decoded.metadata = ImageMetadata::default();
        Ok(decoded)
    }
}

impl Encoder for AvifEncoder {
    fn format(&self) -> FormatTag {
        FormatTag::Avif
    }

    fn lossless_capability(&self) -> LosslessCapability {
        LosslessCapability::Configurable
    }

    fn encode_native(&self, image: &DecodedImage, options: &EncodeOptions) -> Result<Vec<u8>> {
        options.validate(self)?;
        let rgba = rgba_to_rgba8(image)?;

        let pixels =
            ZenPixelBuffer::from_vec(rgba, image.width, image.height, PixelDescriptor::RGBA8)
                .map_err(|error| Error::EncodingFailed {
                    message: error.to_string(),
                })?;

        let config = match options.compression {
            crate::Compression::Lossless => zenavif::EncoderConfig::new().quality(100.0),
            crate::Compression::Lossy { quality } => {
                zenavif::EncoderConfig::new().quality(quality as f32)
            }
        };

        let encoded = zenavif::encode_with(
            &pixels,
            &config,
            almost_enough::StopToken::new(zenavif::Unstoppable),
        )
        .map_err(|error| Error::EncodingFailed {
            message: error.to_string(),
        })?;

        Ok(encoded.avif_file)
    }
}

fn corrupt<E: std::fmt::Display>(error: E) -> Error {
    Error::CorruptData {
        message: error.to_string(),
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

fn rgb_to_rgba(rgb: &[u8]) -> Result<Vec<u8>> {
    let (pixels, remainder) = rgb.as_chunks::<3>();
    if !remainder.is_empty() {
        return Err(Error::CorruptData {
            message: "AVIF decoder returned incomplete RGB data".to_owned(),
        });
    }
    let mut rgba = Vec::with_capacity(rgb.len() / 3 * 4);
    for pixel in pixels {
        rgba.extend_from_slice(&[pixel[0], pixel[1], pixel[2], u8::MAX]);
    }
    Ok(rgba)
}

fn rgba_to_rgba8(image: &DecodedImage) -> Result<Vec<u8>> {
    let PixelBuffer::Rgba8(pixels) = &image.pixels else {
        return Err(Error::InvalidOptions {
            message: "AVIF encoding supports RGBA8 images only".to_owned(),
        });
    };
    Ok(pixels.clone())
}

#[cfg(test)]
mod tests {
    use super::{AvifDecoder, AvifEncoder};
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
    fn avif_probe_matches_known_header_sequence() {
        assert!(AvifDecoder.probe(b"\0\0\0\x18ftypavif"));
    }

    #[test]
    fn avif_reports_lossless_capability_and_encodes() {
        assert_eq!(
            AvifEncoder.lossless_capability(),
            LosslessCapability::Configurable
        );
        let result = AvifEncoder.encode(
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
