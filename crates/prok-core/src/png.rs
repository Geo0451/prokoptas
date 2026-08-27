use std::io::Cursor;

use png::{BitDepth as PngBitDepth, ColorType, Transformations};

use crate::{
    BitDepth, ColorSpace, DecodeOptions, DecodedImage, Decoder, EncodeOptions, Encoder, Error,
    FormatRegistry, FormatTag, ImageMetadata, LosslessCapability, Orientation, PixelBuffer, Result,
};

pub struct PngDecoder;
pub struct PngEncoder;

pub static PNG_DECODER: PngDecoder = PngDecoder;
pub static PNG_ENCODER: PngEncoder = PngEncoder;
pub static PNG_DECODERS: [&dyn Decoder; 1] = [&PNG_DECODER];
pub static PNG_ENCODERS: [&dyn Encoder; 1] = [&PNG_ENCODER];
pub static PNG_REGISTRY: FormatRegistry<'static> =
    FormatRegistry::new(&PNG_DECODERS, &PNG_ENCODERS);

impl Decoder for PngDecoder {
    fn format(&self) -> FormatTag {
        FormatTag::Png
    }

    fn probe(&self, input: &[u8]) -> bool {
        input.starts_with(b"\x89PNG\r\n\x1a\n")
    }

    fn decode(&self, input: &[u8], options: &DecodeOptions) -> Result<DecodedImage> {
        let mut decoder = png::Decoder::new(Cursor::new(input));
        decoder.set_transformations(Transformations::EXPAND);
        let mut reader = decoder.read_info().map_err(corrupt)?;
        let width = reader.info().width;
        let height = reader.info().height;
        let output_size = reader.output_buffer_size();
        enforce_memory_limit(output_size, options.memory_limit_mb)?;
        let mut buffer = vec![0; output_size];
        let output = reader.next_frame(&mut buffer).map_err(corrupt)?;
        let data = &buffer[..output.buffer_size()];
        let (pixels, bit_depth) = normalize_pixels(data, output.color_type, output.bit_depth)?;
        let exif = extract_exif(input)?;
        let orientation = exif
            .as_deref()
            .and_then(parse_exif_orientation)
            .unwrap_or(Orientation::Normal);
        let color_space = ColorSpace::Srgb;
        let mut image = DecodedImage::new(pixels, width, height, color_space, bit_depth)?;
        image.orientation = orientation;
        image.metadata = ImageMetadata {
            exif,
            ..ImageMetadata::default()
        };

        if options.auto_rotate && orientation != Orientation::Normal {
            apply_orientation(&mut image)?;
        }

        Ok(image)
    }
}

impl Encoder for PngEncoder {
    fn format(&self) -> FormatTag {
        FormatTag::Png
    }

    fn lossless_capability(&self) -> LosslessCapability {
        LosslessCapability::Always
    }

    fn encode(&self, image: &DecodedImage, options: &EncodeOptions) -> Result<Vec<u8>> {
        options.validate(self)?;
        let (depth, bytes) = match (&image.pixels, options.bit_depth) {
            (PixelBuffer::Rgba8(data), BitDepth::Eight) => (PngBitDepth::Eight, data.clone()),
            (PixelBuffer::Rgba16(data), BitDepth::Sixteen) => {
                let bytes = data.iter().flat_map(|value| value.to_be_bytes()).collect();
                (PngBitDepth::Sixteen, bytes)
            }
            (PixelBuffer::Rgba8(_), _) => {
                return Err(Error::InvalidOptions {
                    message: "PNG RGBA8 encoding requires 8-bit output".to_owned(),
                });
            }
            (PixelBuffer::Rgba16(_), _) => {
                return Err(Error::InvalidOptions {
                    message: "PNG encoding supports only 8-bit or 16-bit output".to_owned(),
                });
            }
        };

        let mut encoded = Vec::new();
        let mut encoder = png::Encoder::new(&mut encoded, image.width, image.height);
        encoder.set_color(ColorType::Rgba);
        encoder.set_depth(depth);
        let mut writer = encoder.write_header().map_err(encoding)?;
        if options.metadata_retention.exif {
            if let Some(exif) = &image.metadata.exif {
                writer
                    .write_chunk(png::chunk::eXIf, exif)
                    .map_err(encoding)?;
            }
        }
        writer.write_image_data(&bytes).map_err(encoding)?;
        drop(writer);
        Ok(encoded)
    }
}

fn corrupt(error: png::DecodingError) -> Error {
    Error::CorruptData {
        message: error.to_string(),
    }
}

fn encoding(error: png::EncodingError) -> Error {
    Error::EncodingFailed {
        message: error.to_string(),
    }
}

fn extract_exif(input: &[u8]) -> Result<Option<Vec<u8>>> {
    if !input.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err(Error::CorruptData {
            message: "missing PNG signature".to_owned(),
        });
    }
    let mut offset = 8;
    while offset + 12 <= input.len() {
        let length = u32::from_be_bytes([
            input[offset],
            input[offset + 1],
            input[offset + 2],
            input[offset + 3],
        ]) as usize;
        let data_start = offset + 8;
        let data_end = data_start
            .checked_add(length)
            .ok_or_else(|| Error::CorruptData {
                message: "PNG chunk length overflow".to_owned(),
            })?;
        let chunk_end = data_end.checked_add(4).ok_or_else(|| Error::CorruptData {
            message: "PNG chunk boundary overflow".to_owned(),
        })?;
        if chunk_end > input.len() {
            return Err(Error::CorruptData {
                message: "PNG chunk extends beyond input".to_owned(),
            });
        }
        if &input[offset + 4..offset + 8] == b"eXIf" {
            return Ok(Some(input[data_start..data_end].to_vec()));
        }
        if &input[offset + 4..offset + 8] == b"IEND" {
            break;
        }
        offset = chunk_end;
    }
    Ok(None)
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

fn normalize_pixels(
    data: &[u8],
    color_type: ColorType,
    bit_depth: PngBitDepth,
) -> Result<(PixelBuffer, BitDepth)> {
    match (color_type, bit_depth) {
        (ColorType::Rgba, PngBitDepth::Eight) => {
            Ok((PixelBuffer::Rgba8(data.to_vec()), BitDepth::Eight))
        }
        (ColorType::Rgb, PngBitDepth::Eight) => {
            Ok((PixelBuffer::Rgba8(add_alpha(data, 3)), BitDepth::Eight))
        }
        (ColorType::Grayscale, PngBitDepth::Eight) => {
            Ok((PixelBuffer::Rgba8(add_alpha(data, 1)), BitDepth::Eight))
        }
        (ColorType::GrayscaleAlpha, PngBitDepth::Eight) => Ok((
            PixelBuffer::Rgba8(gray_alpha_to_rgba(data)),
            BitDepth::Eight,
        )),
        (ColorType::Rgba, PngBitDepth::Sixteen) => {
            Ok((PixelBuffer::Rgba16(bytes_to_u16(data)?), BitDepth::Sixteen))
        }
        (ColorType::Rgb, PngBitDepth::Sixteen) => Ok((
            PixelBuffer::Rgba16(add_alpha_16(data, 3)),
            BitDepth::Sixteen,
        )),
        (ColorType::Grayscale, PngBitDepth::Sixteen) => Ok((
            PixelBuffer::Rgba16(add_alpha_16(data, 1)),
            BitDepth::Sixteen,
        )),
        (ColorType::GrayscaleAlpha, PngBitDepth::Sixteen) => Ok((
            PixelBuffer::Rgba16(gray_alpha_16_to_rgba(data)?),
            BitDepth::Sixteen,
        )),
        (_, depth) => Err(Error::CorruptData {
            message: format!(
                "unsupported expanded PNG color/depth combination: {color_type:?}/{depth:?}"
            ),
        }),
    }
}

fn add_alpha(data: &[u8], channels: usize) -> Vec<u8> {
    let mut output = Vec::with_capacity(data.len() / channels * 4);
    for pixel in data.chunks_exact(channels) {
        let value = if channels == 1 { pixel[0] } else { 0 };
        if channels == 1 {
            output.extend_from_slice(&[value, value, value, u8::MAX]);
        } else {
            output.extend_from_slice(&[pixel[0], pixel[1], pixel[2], u8::MAX]);
        }
    }
    output
}

fn gray_alpha_to_rgba(data: &[u8]) -> Vec<u8> {
    let mut output = Vec::with_capacity(data.len() * 2);
    for pixel in data.as_chunks::<2>().0 {
        output.extend_from_slice(&[pixel[0], pixel[0], pixel[0], pixel[1]]);
    }
    output
}

fn bytes_to_u16(data: &[u8]) -> Result<Vec<u16>> {
    let (chunks, remainder) = data.as_chunks::<2>();
    if !remainder.is_empty() {
        return Err(Error::CorruptData {
            message: "PNG 16-bit data has an incomplete sample".to_owned(),
        });
    }
    Ok(chunks
        .iter()
        .map(|chunk| u16::from_be_bytes(*chunk))
        .collect())
}

fn add_alpha_16(data: &[u8], channels: usize) -> Vec<u16> {
    let values: Vec<u16> = data
        .as_chunks::<2>()
        .0
        .iter()
        .map(|chunk| u16::from_be_bytes(*chunk))
        .collect();
    let mut output = Vec::with_capacity(values.len() / channels * 4);
    for pixel in values.chunks_exact(channels) {
        if channels == 1 {
            output.extend_from_slice(&[pixel[0], pixel[0], pixel[0], u16::MAX]);
        } else {
            output.extend_from_slice(&[pixel[0], pixel[1], pixel[2], u16::MAX]);
        }
    }
    output
}

fn gray_alpha_16_to_rgba(data: &[u8]) -> Result<Vec<u16>> {
    let values = bytes_to_u16(data)?;
    let mut output = Vec::with_capacity(values.len() * 2);
    for pixel in values.as_chunks::<2>().0 {
        output.extend_from_slice(&[pixel[0], pixel[0], pixel[0], pixel[1]]);
    }
    Ok(output)
}

fn parse_exif_orientation(data: &[u8]) -> Option<Orientation> {
    let data = data.strip_prefix(b"Exif\0\0").unwrap_or(data);
    let little_endian = match data.get(..2)? {
        b"II" => true,
        b"MM" => false,
        _ => return None,
    };
    let read_u16 = |bytes: &[u8]| {
        let bytes = [bytes[0], bytes[1]];
        if little_endian {
            u16::from_le_bytes(bytes)
        } else {
            u16::from_be_bytes(bytes)
        }
    };
    let read_u32 = |bytes: &[u8]| {
        let bytes = [bytes[0], bytes[1], bytes[2], bytes[3]];
        if little_endian {
            u32::from_le_bytes(bytes)
        } else {
            u32::from_be_bytes(bytes)
        }
    };
    if read_u16(data.get(2..4)?) != 42 {
        return None;
    }
    let ifd_offset = usize::try_from(read_u32(data.get(4..8)?)).ok()?;
    let count = usize::from(read_u16(data.get(ifd_offset..ifd_offset + 2)?));
    for index in 0..count {
        let entry = ifd_offset + 2 + index * 12;
        if read_u16(data.get(entry..entry + 2)?) == 0x0112 {
            let value = read_u16(data.get(entry + 8..entry + 10)?);
            return Some(match value {
                1 => Orientation::Normal,
                2 => Orientation::FlipHorizontal,
                3 => Orientation::Rotate180,
                4 => Orientation::FlipVertical,
                5 => Orientation::Transpose,
                6 => Orientation::Rotate90,
                7 => Orientation::Transverse,
                8 => Orientation::Rotate270,
                _ => return None,
            });
        }
    }
    None
}

fn apply_orientation(image: &mut DecodedImage) -> Result<()> {
    let orientation = image.orientation;
    let (new_width, new_height) = match orientation {
        Orientation::Transpose
        | Orientation::Rotate90
        | Orientation::Transverse
        | Orientation::Rotate270 => (image.height, image.width),
        _ => (image.width, image.height),
    };
    match &image.pixels {
        PixelBuffer::Rgba8(data) => {
            image.pixels = PixelBuffer::Rgba8(remap(data, image.width, image.height, orientation))
        }
        PixelBuffer::Rgba16(data) => {
            image.pixels = PixelBuffer::Rgba16(remap(data, image.width, image.height, orientation))
        }
    }
    image.width = new_width;
    image.height = new_height;
    image.orientation_applied = true;
    Ok(())
}

fn remap<T: Copy>(data: &[T], width: u32, height: u32, orientation: Orientation) -> Vec<T> {
    let width = width as usize;
    let height = height as usize;
    let channels = 4;
    let new_width = if matches!(
        orientation,
        Orientation::Transpose
            | Orientation::Rotate90
            | Orientation::Transverse
            | Orientation::Rotate270
    ) {
        height
    } else {
        width
    };
    let mut output = vec![data[0]; data.len()];
    for y in 0..height {
        for x in 0..width {
            let (new_x, new_y) = match orientation {
                Orientation::Normal => (x, y),
                Orientation::FlipHorizontal => (width - 1 - x, y),
                Orientation::Rotate180 => (width - 1 - x, height - 1 - y),
                Orientation::FlipVertical => (x, height - 1 - y),
                Orientation::Transpose => (y, x),
                Orientation::Rotate90 => (height - 1 - y, x),
                Orientation::Transverse => (height - 1 - y, width - 1 - x),
                Orientation::Rotate270 => (y, width - 1 - x),
            };
            let source = (y * width + x) * channels;
            let destination = (new_y * new_width + new_x) * channels;
            output[destination..destination + channels]
                .copy_from_slice(&data[source..source + channels]);
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::{PngDecoder, PngEncoder};
    use crate::{
        BitDepth, ColorSpace, Compression, DecodeOptions, DecodedImage, Decoder, EncodeOptions,
        Encoder, Error, ImageMetadata, Orientation, PixelBuffer,
    };

    fn image() -> DecodedImage {
        DecodedImage::new(
            PixelBuffer::rgba8(
                vec![
                    255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
                ],
                2,
                2,
            )
            .expect("valid fixture pixels"),
            2,
            2,
            ColorSpace::Srgb,
            BitDepth::Eight,
        )
        .expect("valid fixture image")
    }

    fn lossless_options() -> EncodeOptions {
        EncodeOptions {
            compression: Compression::Lossless,
            bit_depth: BitDepth::Eight,
            ..EncodeOptions::default()
        }
    }

    fn orientation_exif(value: u16) -> Vec<u8> {
        vec![
            b'I',
            b'I',
            42,
            0,
            8,
            0,
            0,
            0,
            1,
            0,
            18,
            1,
            3,
            0,
            1,
            0,
            0,
            0,
            value as u8,
            (value >> 8) as u8,
            0,
            0,
            0,
            0,
            0,
            0,
        ]
    }

    #[test]
    fn rgba8_round_trip_is_lossless() {
        let encoder = PngEncoder;
        let decoder = PngDecoder;
        let source = image();
        let encoded = encoder
            .encode(&source, &lossless_options())
            .expect("PNG encoding succeeds");
        assert!(encoded.starts_with(b"\x89PNG\r\n\x1a\n"));

        let decoded = decoder
            .decode(&encoded, &DecodeOptions::default())
            .expect("PNG decoding succeeds");
        assert_eq!(decoded.width, source.width);
        assert_eq!(decoded.height, source.height);
        assert_eq!(decoded.pixels, source.pixels);
    }

    #[test]
    fn exif_is_retained_and_orientation_is_applied_when_requested() {
        let encoder = PngEncoder;
        let decoder = PngDecoder;
        let mut source = DecodedImage::new(
            PixelBuffer::rgba8(vec![255, 0, 0, 255, 0, 255, 0, 255], 2, 1)
                .expect("valid orientation fixture pixels"),
            2,
            1,
            ColorSpace::Srgb,
            BitDepth::Eight,
        )
        .expect("valid orientation fixture image");
        source.metadata = ImageMetadata {
            exif: Some(orientation_exif(6)),
            ..ImageMetadata::default()
        };
        let options = EncodeOptions {
            metadata_retention: crate::MetadataRetention {
                exif: true,
                ..crate::MetadataRetention::default()
            },
            ..lossless_options()
        };
        let encoded = encoder
            .encode(&source, &options)
            .expect("PNG encoding succeeds");

        let untouched = decoder
            .decode(
                &encoded,
                &DecodeOptions {
                    auto_rotate: false,
                    ..DecodeOptions::default()
                },
            )
            .expect("PNG decoding succeeds");
        assert_eq!(untouched.orientation, Orientation::Rotate90);
        assert!(!untouched.orientation_applied);
        assert_eq!(untouched.metadata.exif, source.metadata.exif);

        let rotated = decoder
            .decode(&encoded, &DecodeOptions::default())
            .expect("PNG decoding succeeds");
        assert_eq!((rotated.width, rotated.height), (1, 2));
        assert_eq!(
            rotated.pixels,
            PixelBuffer::rgba8(vec![255, 0, 0, 255, 0, 255, 0, 255], 1, 2)
                .expect("valid rotated pixels")
        );
        assert!(rotated.orientation_applied);
    }

    #[test]
    fn decode_memory_limit_is_checked_before_pixel_allocation() {
        let encoder = PngEncoder;
        let encoded = encoder
            .encode(&image(), &lossless_options())
            .expect("PNG encoding succeeds");
        let result = PngDecoder.decode(
            &encoded,
            &DecodeOptions {
                memory_limit_mb: Some(0),
                ..DecodeOptions::default()
            },
        );
        assert_eq!(
            result,
            Err(Error::MemoryLimitExceeded {
                required_mb: 1,
                allowed_mb: 0
            })
        );
    }
}
