use std::io::{BufReader, Cursor};

use image_webp::WebPDecoder as ImageWebpDecoder;
use zenwebp::{EncodeRequest, LosslessConfig, LossyConfig, PixelLayout};

use crate::{
    BitDepth, ColorSpace, DecodeOptions, DecodedImage, Decoder, EncodeOptions, Encoder, Error,
    FormatRegistry, FormatTag, ImageMetadata, LosslessCapability, PixelBuffer, Result,
};

pub struct WebpDecoder;
pub struct WebpEncoder;

pub static WEBP_DECODER: WebpDecoder = WebpDecoder;
pub static WEBP_ENCODER: WebpEncoder = WebpEncoder;
pub static WEBP_DECODERS: [&dyn Decoder; 1] = [&WEBP_DECODER];
pub static WEBP_ENCODERS: [&dyn Encoder; 1] = [&WEBP_ENCODER];
pub static WEBP_REGISTRY: FormatRegistry<'static> =
    FormatRegistry::new(&WEBP_DECODERS, &WEBP_ENCODERS);

impl Decoder for WebpDecoder {
    fn format(&self) -> FormatTag {
        FormatTag::WebP
    }

    fn probe(&self, input: &[u8]) -> bool {
        input.starts_with(b"RIFF") && input.get(8..12) == Some(b"WEBP")
    }

    fn decode(&self, input: &[u8], options: &DecodeOptions) -> Result<DecodedImage> {
        let mut decoder =
            ImageWebpDecoder::new(BufReader::new(Cursor::new(input))).map_err(corrupt)?;
        if decoder.is_animated() {
            return Err(Error::CorruptData {
                message: "animated WebP is outside the still-image Core model".to_owned(),
            });
        }
        let (width, height) = decoder.dimensions();
        let output_size = decoder
            .output_buffer_size()
            .ok_or_else(|| Error::CorruptData {
                message: "WebP output size overflow".to_owned(),
            })?;
        enforce_memory_limit(output_size, options.memory_limit_mb)?;
        let mut data = vec![0; output_size];
        decoder.read_image(&mut data).map_err(corrupt)?;
        let rgba = if decoder.has_alpha() {
            data
        } else {
            rgb_to_rgba(&data)?
        };
        let mut image = DecodedImage::new(
            PixelBuffer::rgba8(rgba, width, height)?,
            width,
            height,
            options.color_space_override.unwrap_or(ColorSpace::Srgb),
            BitDepth::Eight,
        )?;
        image.metadata = ImageMetadata {
            exif: decoder.exif_metadata().map_err(corrupt)?,
            xmp: decoder.xmp_metadata().map_err(corrupt)?,
            ..ImageMetadata::default()
        };
        Ok(image)
    }
}

impl Encoder for WebpEncoder {
    fn format(&self) -> FormatTag {
        FormatTag::WebP
    }

    fn lossless_capability(&self) -> LosslessCapability {
        LosslessCapability::Configurable
    }

    fn encode(&self, image: &DecodedImage, options: &EncodeOptions) -> Result<Vec<u8>> {
        options.validate(self)?;
        let PixelBuffer::Rgba8(rgba) = &image.pixels else {
            return Err(Error::InvalidOptions {
                message: "WebP encoding supports RGBA8 images only".to_owned(),
            });
        };
        match options.compression {
            crate::Compression::Lossless => {
                let config = LosslessConfig::new().with_quality(options.effort as f32 * 10.0);
                let request = EncodeRequest::lossless(
                    &config,
                    rgba,
                    PixelLayout::Rgba8,
                    image.width,
                    image.height,
                );
                let request = add_metadata(request, image, options);
                request.encode().map_err(encode)
            }
            crate::Compression::Lossy { quality } => {
                let config = LossyConfig::new()
                    .with_quality(quality as f32)
                    .with_method(options.effort.min(6));
                let request = EncodeRequest::lossy(
                    &config,
                    rgba,
                    PixelLayout::Rgba8,
                    image.width,
                    image.height,
                );
                let request = add_metadata(request, image, options);
                request.encode().map_err(encode)
            }
        }
    }
}

fn add_metadata<'a>(
    request: EncodeRequest<'a>,
    image: &'a DecodedImage,
    options: &EncodeOptions,
) -> EncodeRequest<'a> {
    let request = if options.metadata_retention.exif {
        if let Some(exif) = image.metadata.exif.as_deref() {
            request.with_exif(exif)
        } else {
            request
        }
    } else {
        request
    };
    if options.metadata_retention.xmp {
        if let Some(xmp) = image.metadata.xmp.as_deref() {
            request.with_xmp(xmp)
        } else {
            request
        }
    } else {
        request
    }
}

fn corrupt<E: std::fmt::Display>(error: E) -> Error {
    Error::CorruptData {
        message: error.to_string(),
    }
}

fn encode<E: std::fmt::Display>(error: E) -> Error {
    Error::EncodingFailed {
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
            message: "WebP decoder returned incomplete RGB data".to_owned(),
        });
    }
    let mut rgba = Vec::with_capacity(rgb.len() / 3 * 4);
    for pixel in pixels {
        rgba.extend_from_slice(&[pixel[0], pixel[1], pixel[2], u8::MAX]);
    }
    Ok(rgba)
}

#[cfg(test)]
mod tests {
    use super::{WebpDecoder, WebpEncoder};
    use crate::{
        BitDepth, ColorSpace, Compression, DecodeOptions, DecodedImage, Decoder, EncodeOptions,
        Encoder, LosslessCapability, PixelBuffer,
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
    fn webp_lossless_round_trip_preserves_pixels() {
        let encoded = WebpEncoder
            .encode(
                &fixture(),
                &EncodeOptions {
                    compression: Compression::Lossless,
                    ..EncodeOptions::default()
                },
            )
            .expect("lossless WebP encoding succeeds");
        let decoded = WebpDecoder
            .decode(&encoded, &DecodeOptions::default())
            .expect("WebP decoding succeeds");
        assert_eq!(decoded.pixels, fixture().pixels);
        assert_eq!(
            WebpEncoder.lossless_capability(),
            LosslessCapability::Configurable
        );
    }

    #[test]
    fn webp_lossy_round_trip_produces_still_image() {
        let encoded = WebpEncoder
            .encode(
                &fixture(),
                &EncodeOptions {
                    compression: Compression::Lossy { quality: 75 },
                    ..EncodeOptions::default()
                },
            )
            .expect("lossy WebP encoding succeeds");
        let decoded = WebpDecoder
            .decode(&encoded, &DecodeOptions::default())
            .expect("WebP decoding succeeds");
        assert_eq!((decoded.width, decoded.height), (2, 1));
        assert_eq!(decoded.pixels.len(), 8);
        if let PixelBuffer::Rgba8(pixels) = decoded.pixels {
            assert!(pixels
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel[0] != pixel[1] || pixel[1] != pixel[2]));
        }
    }
}
