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
