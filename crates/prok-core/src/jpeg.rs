use jpeg_encoder::{ColorType, SamplingFactor};
use zune_core::{colorspace::ColorSpace as ZuneColorSpace, options::DecoderOptions};

use crate::{
    BitDepth, ColorSpace, DecodeOptions, DecodedImage, Decoder, EncodeOptions, Encoder, Error,
    FormatRegistry, FormatTag, ImageMetadata, LosslessCapability, PixelBuffer, Result,
};

pub struct JpegDecoder;
pub struct JpegEncoder;

pub static JPEG_DECODER: JpegDecoder = JpegDecoder;
pub static JPEG_ENCODER: JpegEncoder = JpegEncoder;
pub static JPEG_DECODERS: [&dyn Decoder; 1] = [&JPEG_DECODER];
pub static JPEG_ENCODERS: [&dyn Encoder; 1] = [&JPEG_ENCODER];
pub static JPEG_REGISTRY: FormatRegistry<'static> =
    FormatRegistry::new(&JPEG_DECODERS, &JPEG_ENCODERS);

impl Decoder for JpegDecoder {
    fn format(&self) -> FormatTag {
        FormatTag::Jpeg
    }

    fn probe(&self, input: &[u8]) -> bool {
        input.starts_with(&[0xff, 0xd8, 0xff])
    }

    fn decode_native(&self, input: &[u8], options: &DecodeOptions) -> Result<DecodedImage> {
        let decoder_options =
            DecoderOptions::default().jpeg_set_out_colorspace(ZuneColorSpace::RGB);
        let mut decoder = zune_jpeg::JpegDecoder::new_with_options(input, decoder_options);
        decoder.decode_headers().map_err(corrupt)?;
        let (width, height) = decoder.dimensions().ok_or_else(|| Error::CorruptData {
            message: "JPEG dimensions are unavailable".to_owned(),
        })?;
        let bytes = decoder
            .output_buffer_size()
            .ok_or_else(|| Error::CorruptData {
                message: "JPEG output size is unavailable".to_owned(),
            })?;
        enforce_memory_limit(bytes * 4 / 3, options.memory_limit_mb)?;
        let rgb = decoder.decode().map_err(corrupt)?;
        let rgba = rgb_to_rgba(&rgb)?;
        let mut image = DecodedImage::new(
            PixelBuffer::rgba8(rgba, width as u32, height as u32)?,
            width as u32,
            height as u32,
            ColorSpace::Srgb,
            BitDepth::Eight,
        )?;
        image.metadata = ImageMetadata {
            exif: decoder.exif().cloned(),
            ..ImageMetadata::default()
        };
        Ok(image)
    }
}

impl Encoder for JpegEncoder {
    fn format(&self) -> FormatTag {
        FormatTag::Jpeg
    }

    fn lossless_capability(&self) -> LosslessCapability {
        LosslessCapability::Never
    }

    fn encode_native(&self, image: &DecodedImage, options: &EncodeOptions) -> Result<Vec<u8>> {
        options.validate(self)?;
        let quality = match options.compression {
            crate::Compression::Lossy { quality } => quality,
            crate::Compression::Lossless => return Err(Error::LosslessNotSupported),
        };
        let rgb = rgba_to_rgb(&image.pixels)?;
        let sampling = match options.chroma_subsampling {
            Some(crate::ChromaSubsampling::Yuv444) | None => SamplingFactor::R_4_4_4,
            Some(crate::ChromaSubsampling::Yuv422) => SamplingFactor::R_4_2_2,
            Some(crate::ChromaSubsampling::Yuv420) => SamplingFactor::R_4_2_0,
        };
        let mut output = Vec::new();
        let mut encoder = jpeg_encoder::Encoder::new(&mut output, quality);
        encoder.set_sampling_factor(sampling);
        encoder
            .encode(
                &rgb,
                image.width as u16,
                image.height as u16,
                ColorType::Rgb,
            )
            .map_err(|error| Error::EncodingFailed {
                message: error.to_string(),
            })?;
        Ok(output)
    }
}

fn corrupt(error: zune_jpeg::errors::DecodeErrors) -> Error {
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
            message: "JPEG decoder returned incomplete RGB data".to_owned(),
        });
    }
    let mut rgba = Vec::with_capacity(rgb.len() / 3 * 4);
    for pixel in pixels {
        rgba.extend_from_slice(&[pixel[0], pixel[1], pixel[2], u8::MAX]);
    }
    Ok(rgba)
}

fn rgba_to_rgb(pixels: &PixelBuffer) -> Result<Vec<u8>> {
    let PixelBuffer::Rgba8(rgba) = pixels else {
        return Err(Error::InvalidOptions {
            message: "JPEG encoding supports RGBA8 images only".to_owned(),
        });
    };
    let (pixels, remainder) = rgba.as_chunks::<4>();
    if !remainder.is_empty() {
        return Err(Error::CorruptData {
            message: "image contains incomplete RGBA data".to_owned(),
        });
    }
    let mut rgb = Vec::with_capacity(rgba.len() / 4 * 3);
    for pixel in pixels {
        rgb.extend_from_slice(&pixel[..3]);
    }
    Ok(rgb)
}

#[cfg(test)]
mod tests {
    use super::{JpegDecoder, JpegEncoder};
    use crate::{
        BitDepth, ColorSpace, Compression, DecodeOptions, DecodedImage, Decoder, EncodeOptions,
        Encoder, Error, LosslessCapability, PixelBuffer,
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
    fn jpeg_lossy_round_trip_works() {
        let encoded = JpegEncoder
            .encode(
                &fixture(),
                &EncodeOptions {
                    compression: Compression::Lossy { quality: 90 },
                    ..EncodeOptions::default()
                },
            )
            .expect("JPEG encoding succeeds");
        assert!(JpegDecoder.probe(&encoded));
        let decoded = JpegDecoder
            .decode(&encoded, &DecodeOptions::default())
            .expect("JPEG decoding succeeds");
        assert_eq!((decoded.width, decoded.height), (2, 1));
        assert_eq!(decoded.pixels.len(), 8);
    }

    #[test]
    fn jpeg_rejects_lossless_encoding() {
        assert_eq!(JpegEncoder.lossless_capability(), LosslessCapability::Never);
        let result = JpegEncoder.encode(
            &fixture(),
            &EncodeOptions {
                compression: Compression::Lossless,
                ..EncodeOptions::default()
            },
        );
        assert_eq!(result, Err(Error::LosslessNotSupported));
    }
}
