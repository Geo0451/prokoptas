use serde::{Deserialize, Serialize};

use crate::{Error, Result};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PixelBuffer {
    Rgba8(Vec<u8>),
    Rgba16(Vec<u16>),
}

impl PixelBuffer {
    pub fn rgba8(data: Vec<u8>, width: u32, height: u32) -> Result<Self> {
        validate_len(data.len(), width, height)?;
        Ok(Self::Rgba8(data))
    }

    pub fn rgba16(data: Vec<u16>, width: u32, height: u32) -> Result<Self> {
        validate_len(data.len(), width, height)?;
        Ok(Self::Rgba16(data))
    }

    pub const fn bit_depth(&self) -> BitDepth {
        match self {
            Self::Rgba8(_) => BitDepth::Eight,
            Self::Rgba16(_) => BitDepth::Sixteen,
        }
    }

    pub fn len(&self) -> usize {
        match self {
            Self::Rgba8(data) => data.len(),
            Self::Rgba16(data) => data.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

fn validate_len(len: usize, width: u32, height: u32) -> Result<()> {
    let expected = usize::try_from(width)
        .ok()
        .and_then(|width| {
            usize::try_from(height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .and_then(|pixels| pixels.checked_mul(4));

    if expected != Some(len) {
        return Err(Error::CorruptData {
            message: format!("expected {expected:?} RGBA components, got {len}"),
        });
    }

    Ok(())
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ColorSpace {
    Srgb,
    Linear,
    DisplayP3,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum BitDepth {
    Eight,
    Ten,
    Twelve,
    Sixteen,
}

impl BitDepth {
    pub const fn bits(self) -> u8 {
        match self {
            Self::Eight => 8,
            Self::Ten => 10,
            Self::Twelve => 12,
            Self::Sixteen => 16,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum Orientation {
    Normal,
    FlipHorizontal,
    Rotate180,
    FlipVertical,
    Transpose,
    Rotate90,
    Transverse,
    Rotate270,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ImageMetadata {
    pub exif: Option<Vec<u8>>,
    pub iptc: Option<Vec<u8>>,
    pub xmp: Option<Vec<u8>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecodedImage {
    pub pixels: PixelBuffer,
    pub width: u32,
    pub height: u32,
    pub color_space: ColorSpace,
    pub bit_depth: BitDepth,
    pub orientation: Orientation,
    pub orientation_applied: bool,
    pub metadata: ImageMetadata,
}

impl DecodedImage {
    pub fn new(
        pixels: PixelBuffer,
        width: u32,
        height: u32,
        color_space: ColorSpace,
        bit_depth: BitDepth,
    ) -> Result<Self> {
        let pixel_depth = pixels.bit_depth();
        if pixel_depth == BitDepth::Eight && bit_depth != BitDepth::Eight {
            return Err(Error::CorruptData {
                message: "RGBA8 pixels require 8-bit metadata".to_owned(),
            });
        }
        if pixel_depth == BitDepth::Sixteen && bit_depth == BitDepth::Eight {
            return Err(Error::CorruptData {
                message: "RGBA16 pixels cannot use 8-bit metadata".to_owned(),
            });
        }

        Ok(Self {
            pixels,
            width,
            height,
            color_space,
            bit_depth,
            orientation: Orientation::Normal,
            orientation_applied: false,
            metadata: ImageMetadata::default(),
        })
    }

    pub(crate) fn convert_color_space(&mut self, target: ColorSpace) {
        if self.color_space == target {
            return;
        }

        let max_sample = sample_max(self.bit_depth);
        match &mut self.pixels {
            PixelBuffer::Rgba8(pixels) => {
                for pixel in pixels.chunks_exact_mut(4) {
                    let rgb = convert_rgb_values(
                        [
                            f64::from(pixel[0]),
                            f64::from(pixel[1]),
                            f64::from(pixel[2]),
                        ],
                        255.0,
                        self.color_space,
                        target,
                    );
                    pixel[..3].copy_from_slice(&rgb.map(|value| value.round() as u8));
                }
            }
            PixelBuffer::Rgba16(pixels) => {
                for pixel in pixels.chunks_exact_mut(4) {
                    let rgb = convert_rgb_values(
                        [
                            f64::from(pixel[0]),
                            f64::from(pixel[1]),
                            f64::from(pixel[2]),
                        ],
                        max_sample,
                        self.color_space,
                        target,
                    );
                    pixel[..3].copy_from_slice(&rgb.map(|value| value.round() as u16));
                }
            }
        }
        self.color_space = target;
    }

    pub(crate) fn convert_bit_depth(&mut self, target: BitDepth) {
        if self.bit_depth == target {
            return;
        }

        let source_max = sample_max(self.bit_depth);
        let target_max = sample_max(target);
        let pixels: Vec<u16> = match &self.pixels {
            PixelBuffer::Rgba8(data) => data
                .iter()
                .map(|value| scale_sample(f64::from(*value), 255.0, target_max))
                .collect(),
            PixelBuffer::Rgba16(data) => data
                .iter()
                .map(|value| scale_sample(f64::from(*value), source_max, target_max))
                .collect(),
        };
        self.pixels = if target == BitDepth::Eight {
            PixelBuffer::Rgba8(pixels.into_iter().map(|value| value as u8).collect())
        } else {
            PixelBuffer::Rgba16(pixels)
        };
        self.bit_depth = target;
    }
}

fn sample_max(bit_depth: BitDepth) -> f64 {
    ((1_u32 << bit_depth.bits()) - 1) as f64
}

fn scale_sample(value: f64, source_max: f64, target_max: f64) -> u16 {
    (value * target_max / source_max)
        .round()
        .clamp(0.0, target_max) as u16
}

fn convert_rgb_values(
    rgb: [f64; 3],
    max_sample: f64,
    source: ColorSpace,
    target: ColorSpace,
) -> [f64; 3] {
    let mut linear = rgb.map(|channel| decode_transfer(channel / max_sample, source));

    if source == ColorSpace::DisplayP3 && target != ColorSpace::DisplayP3 {
        linear = p3_to_srgb(linear);
    } else if source != ColorSpace::DisplayP3 && target == ColorSpace::DisplayP3 {
        linear = srgb_to_p3(linear);
    }

    linear.map(|value| encode_transfer(value, target) * max_sample)
}

fn decode_transfer(value: f64, color_space: ColorSpace) -> f64 {
    if color_space == ColorSpace::Linear {
        value
    } else if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

fn encode_transfer(value: f64, color_space: ColorSpace) -> f64 {
    let value = value.max(0.0);
    if color_space == ColorSpace::Linear {
        value
    } else if value <= 0.0031308 {
        value * 12.92
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    }
}

fn srgb_to_p3(rgb: [f64; 3]) -> [f64; 3] {
    [
        0.82259287 * rgb[0] + 0.17753395 * rgb[1],
        0.03319951 * rgb[0] + 0.96678350 * rgb[1],
        0.01708535 * rgb[0] + 0.07239572 * rgb[1] + 0.91030148 * rgb[2],
    ]
}

fn p3_to_srgb(rgb: [f64; 3]) -> [f64; 3] {
    [
        1.22494018 * rgb[0] - 0.22494018 * rgb[1],
        -0.04205695 * rgb[0] + 1.04205695 * rgb[1],
        -0.01963755 * rgb[0] - 0.07863605 * rgb[1] + 1.09827360 * rgb[2],
    ]
}

pub(crate) fn parse_exif_orientation(data: &[u8]) -> Option<Orientation> {
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

pub(crate) fn set_exif_orientation(data: &mut [u8], orientation: u16) {
    let prefix_len = usize::from(data.starts_with(b"Exif\0\0")) * 6;
    let Some(tiff) = data.get_mut(prefix_len..) else {
        return;
    };
    let little_endian = match tiff.get(..2) {
        Some(b"II") => true,
        Some(b"MM") => false,
        _ => return,
    };
    let Some(header) = tiff.get(2..8) else {
        return;
    };
    let magic = if little_endian {
        u16::from_le_bytes([header[0], header[1]])
    } else {
        u16::from_be_bytes([header[0], header[1]])
    };
    if magic != 42 {
        return;
    }
    let offset_bytes = [header[2], header[3], header[4], header[5]];
    let offset = if little_endian {
        u32::from_le_bytes(offset_bytes)
    } else {
        u32::from_be_bytes(offset_bytes)
    } as usize;
    let Some(count_bytes) = offset
        .checked_add(0)
        .and_then(|start| tiff.get(start..start + 2))
    else {
        return;
    };
    let count = if little_endian {
        u16::from_le_bytes([count_bytes[0], count_bytes[1]])
    } else {
        u16::from_be_bytes([count_bytes[0], count_bytes[1]])
    };
    for index in 0..usize::from(count) {
        let Some(entry) = offset.checked_add(2).and_then(|start| {
            index
                .checked_mul(12)
                .and_then(|index| start.checked_add(index))
        }) else {
            return;
        };
        let Some(tag_bytes) = tiff.get(entry..entry + 2) else {
            return;
        };
        let tag = if little_endian {
            u16::from_le_bytes([tag_bytes[0], tag_bytes[1]])
        } else {
            u16::from_be_bytes([tag_bytes[0], tag_bytes[1]])
        };
        if tag == 0x0112 {
            let Some(value) = tiff.get_mut(entry + 8..entry + 10) else {
                return;
            };
            value.copy_from_slice(&if little_endian {
                orientation.to_le_bytes()
            } else {
                orientation.to_be_bytes()
            });
            return;
        }
    }
}

pub(crate) fn apply_orientation(image: &mut DecodedImage) -> Result<()> {
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
