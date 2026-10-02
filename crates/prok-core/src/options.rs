use serde::{Deserialize, Serialize};

use crate::{ColorSpace, Encoder, Error, FormatTag, Result};

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
    pub fn preset_web_optimized(target: FormatTag) -> Result<Self> {
        if !matches!(target, FormatTag::WebP | FormatTag::Avif) {
            return Err(Error::InvalidOptions {
                message: "Web Optimized preset supports WebP and AVIF targets".to_owned(),
            });
        }

        let options = Self {
            compression: Compression::Lossy { quality: 75 },
            bit_depth: crate::BitDepth::Eight,
            metadata_retention: MetadataRetention::default(),
            ..Self::default()
        };
        options.validate(crate::processing::encoder_for(target)?)?;
        Ok(options)
    }

    pub fn preset_max_quality_archive(target: FormatTag) -> Result<Self> {
        let (bit_depth, metadata_retention) = match target {
            FormatTag::Png => (
                crate::BitDepth::Sixteen,
                MetadataRetention {
                    exif: true,
                    ..MetadataRetention::default()
                },
            ),
            FormatTag::WebP => (
                crate::BitDepth::Eight,
                MetadataRetention {
                    exif: true,
                    xmp: true,
                    ..MetadataRetention::default()
                },
            ),
            FormatTag::Avif => (
                crate::BitDepth::Eight,
                MetadataRetention {
                    exif: true,
                    xmp: true,
                    ..MetadataRetention::default()
                },
            ),
            FormatTag::Tiff => (crate::BitDepth::Sixteen, MetadataRetention::default()),
            FormatTag::Bmp | FormatTag::Jxl => {
                (crate::BitDepth::Eight, MetadataRetention::default())
            }
            FormatTag::Jpeg => {
                let encoder = crate::processing::encoder_for(target)?;
                let options = Self {
                    compression: Compression::Lossless,
                    ..Self::default()
                };
                options.validate(encoder)?;
                return Ok(options);
            }
            FormatTag::Heif | FormatTag::Raw => {
                return Err(Error::InvalidOptions {
                    message: format!("{} is not an archive output format", target.as_str()),
                });
            }
        };

        let options = Self {
            compression: Compression::Lossless,
            bit_depth,
            effort: 10,
            metadata_retention,
            ..Self::default()
        };
        options.validate(crate::processing::encoder_for(target)?)?;
        Ok(options)
    }

    pub fn preset_social_media(target: FormatTag) -> Result<Self> {
        if target != FormatTag::Jpeg {
            return Err(Error::InvalidOptions {
                message: "Social Media preset requires JPEG output".to_owned(),
            });
        }
        let options = Self {
            compression: Compression::Lossy { quality: 82 },
            bit_depth: crate::BitDepth::Eight,
            metadata_retention: MetadataRetention::default(),
            resize_long_edge: Some(1350),
            ..Self::default()
        };
        options.validate(crate::processing::encoder_for(target)?)?;
        Ok(options)
    }

    pub fn validate<E: Encoder + ?Sized>(&self, encoder: &E) -> Result<()> {
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

#[cfg(test)]
mod tests {
    use super::{Compression, EncodeOptions};
    use crate::{Error, FormatTag, LosslessCapability};

    #[test]
    fn presets_validate_against_each_targets_actual_capability() {
        for target in [
            FormatTag::Png,
            FormatTag::Jpeg,
            FormatTag::Tiff,
            FormatTag::Bmp,
            FormatTag::WebP,
            FormatTag::Avif,
            FormatTag::Jxl,
        ] {
            let encoder = crate::processing::encoder_for(target).expect("registered encoder");
            match EncodeOptions::preset_max_quality_archive(target) {
                Ok(options) => {
                    assert_eq!(options.compression, Compression::Lossless);
                    assert_ne!(
                        encoder.lossless_capability(),
                        LosslessCapability::Never,
                        "{target:?} preset requested unsupported lossless output"
                    );
                    options.validate(encoder).expect("archive options validate");
                }
                Err(Error::LosslessNotSupported) => assert_eq!(
                    encoder.lossless_capability(),
                    LosslessCapability::Never,
                    "{target:?} rejected lossless despite advertising support"
                ),
                Err(error) => panic!("unexpected {target:?} archive error: {error}"),
            }
        }
    }

    #[test]
    fn web_optimized_preset_is_lossy_for_supported_targets_only() {
        for target in [FormatTag::WebP, FormatTag::Avif] {
            let options = EncodeOptions::preset_web_optimized(target).expect("web preset");
            assert_eq!(options.compression, Compression::Lossy { quality: 75 });
            assert_eq!(options.metadata_retention, Default::default());
            options
                .validate(crate::processing::encoder_for(target).expect("encoder"))
                .expect("web options validate");
        }
        assert!(EncodeOptions::preset_web_optimized(FormatTag::Jpeg).is_err());
    }

    #[test]
    fn archive_preset_uses_maximum_supported_depth_and_metadata() {
        let png = EncodeOptions::preset_max_quality_archive(FormatTag::Png).expect("PNG preset");
        assert_eq!(png.bit_depth, crate::BitDepth::Sixteen);
        assert!(png.metadata_retention.exif);
        assert!(!png.metadata_retention.xmp);

        let webp = EncodeOptions::preset_max_quality_archive(FormatTag::WebP).expect("WebP preset");
        assert_eq!(webp.bit_depth, crate::BitDepth::Eight);
        assert!(webp.metadata_retention.exif && webp.metadata_retention.xmp);
    }

    #[test]
    fn social_media_preset_is_jpeg_lossy_and_resized() {
        let options = EncodeOptions::preset_social_media(FormatTag::Jpeg).expect("JPEG preset");
        assert_eq!(options.compression, Compression::Lossy { quality: 82 });
        assert_eq!(options.resize_long_edge, Some(1350));
        assert_eq!(options.metadata_retention, Default::default());
        assert!(EncodeOptions::preset_social_media(FormatTag::Png).is_err());
    }
}
