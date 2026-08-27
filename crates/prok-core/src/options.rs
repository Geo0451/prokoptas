use serde::{Deserialize, Serialize};

use crate::{ColorSpace, Encoder, Error, Result};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum Compression {
    Lossy { quality: u8 },
    Lossless,
}

impl Compression {
    pub fn validate(self) -> Result<()> {
        if let Self::Lossy { quality } = self {
            if !(1..=100).contains(&quality) {
                return Err(Error::InvalidOptions {
                    message: "lossy quality must be between 1 and 100".to_owned(),
                });
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum DemosaicQuality {
    #[default]
    Fast,
    HighQuality,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ChromaSubsampling {
    Yuv444,
    Yuv422,
    Yuv420,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum PngFilter {
    Sub,
    Up,
    Average,
    Paeth,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct MetadataRetention {
    pub exif: bool,
    pub iptc: bool,
    pub xmp: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CropRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DecodeOptions {
    pub strict_metadata: bool,
    pub color_space_override: Option<ColorSpace>,
    pub auto_rotate: bool,
    pub demosaic_quality: DemosaicQuality,
    pub memory_limit_mb: Option<u64>,
}

impl Default for DecodeOptions {
    fn default() -> Self {
        Self {
            strict_metadata: false,
            color_space_override: None,
            auto_rotate: true,
            demosaic_quality: DemosaicQuality::Fast,
            memory_limit_mb: None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct EncodeOptions {
    pub compression: Compression,
    pub bit_depth: crate::BitDepth,
    pub chroma_subsampling: Option<ChromaSubsampling>,
    pub png_filter: Option<PngFilter>,
    pub effort: u8,
    pub jxl_noise_synthesis: bool,
    pub jxl_gaborish: bool,
    pub metadata_retention: MetadataRetention,
    pub resize_long_edge: Option<u32>,
    pub crop: Option<CropRect>,
}

impl Default for EncodeOptions {
    fn default() -> Self {
        Self {
            compression: Compression::Lossy { quality: 75 },
            bit_depth: crate::BitDepth::Eight,
            chroma_subsampling: None,
            png_filter: None,
            effort: 5,
            jxl_noise_synthesis: false,
            jxl_gaborish: false,
            metadata_retention: MetadataRetention::default(),
            resize_long_edge: None,
            crop: None,
        }
    }
}

impl EncodeOptions {
    pub fn validate(&self, encoder: &dyn Encoder) -> Result<()> {
        self.compression.validate()?;
        if self.effort > 10 {
            return Err(Error::InvalidOptions {
                message: "effort must be between 0 and 10".to_owned(),
            });
        }
        if matches!(
            (encoder.lossless_capability(), self.compression),
            (crate::LosslessCapability::Always, Compression::Lossy { .. })
        ) {
            return Err(Error::InvalidOptions {
                message: "the selected encoder only supports lossless output".to_owned(),
            });
        }
        if matches!(self.compression, Compression::Lossless)
            && encoder.lossless_capability() == crate::LosslessCapability::Never
        {
            return Err(Error::LosslessNotSupported);
        }
        if matches!(self.compression, Compression::Lossless) && self.chroma_subsampling.is_some() {
            return Err(Error::InvalidOptions {
                message: "chroma subsampling is unavailable in lossless mode".to_owned(),
            });
        }
        Ok(())
    }
}
