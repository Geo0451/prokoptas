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
