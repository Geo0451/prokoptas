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
        let frame_limit = if let Some(allowed_mb) = options.memory_limit_mb {
            let allowed_bytes = allowed_mb.saturating_mul(1024 * 1024);
            let pixels = allowed_bytes / 4;
            if pixels == 0 {
                return Err(Error::MemoryLimitExceeded {
                    required_mb: 1,
                    allowed_mb,
                });
            }
            pixels.min(u64::from(u32::MAX)) as u32
        } else {
            0
        };
        let config = zenavif::DecoderConfig::new().frame_size_limit(frame_limit);
        let decoded = zenavif::decode_with(input, &config, &zenavif::Unstoppable).map_err(
            |error| match error.error() {
                zenavif::Error::ImageTooLarge { width, height } => {
                    let required_mb = (*width as u64)
                        .saturating_mul(u64::from(*height))
                        .saturating_mul(4)
                        .div_ceil(1024 * 1024);
                    Error::MemoryLimitExceeded {
                        required_mb,
                        allowed_mb: options.memory_limit_mb.unwrap_or_default(),
                    }
                }
                _ => corrupt(error),
            },
        )?;
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
        LosslessCapability::Never
    }

    fn encode_native(&self, image: &DecodedImage, options: &EncodeOptions) -> Result<Vec<u8>> {
        options.validate(self)?;
        let crate::Compression::Lossy { quality } = options.compression else {
            return Err(Error::LosslessNotSupported);
        };
        let rgba = rgba_to_rgba8(image)?;

        let pixels =
            ZenPixelBuffer::from_vec(rgba, image.width, image.height, PixelDescriptor::RGBA8)
                .map_err(|error| Error::EncodingFailed {
                    message: error.to_string(),
                })?;

        let mut config = zenavif::EncoderConfig::new()
            .quality(quality as f32)
            .speed(options.effort);
        if options.metadata_retention.exif {
            if let Some(exif) = image.metadata.exif.as_ref() {
                config = config.exif(exif.clone());
            }
        }
        if options.metadata_retention.xmp {
            if let Some(xmp) = image.metadata.xmp.as_ref() {
                config = config.xmp(xmp.clone());
            }
        }

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
    fn avif_does_not_claim_lossless_encoding() {
        assert_eq!(AvifEncoder.lossless_capability(), LosslessCapability::Never);
        let lossless = AvifEncoder.encode(
            &fixture(),
            &crate::EncodeOptions {
                compression: Compression::Lossless,
                ..crate::EncodeOptions::default()
            },
        );
        assert_eq!(lossless, Err(crate::Error::LosslessNotSupported));

        let encoded = AvifEncoder
            .encode(
                &fixture(),
                &crate::EncodeOptions {
                    compression: Compression::Lossy { quality: 75 },
                    ..crate::EncodeOptions::default()
                },
            )
            .expect("AVIF lossy encoding succeeds");
        assert!(!encoded.is_empty());
        assert!(AvifDecoder.probe(&encoded));
    }
}
