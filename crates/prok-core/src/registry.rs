use serde::{Deserialize, Serialize};

use crate::{Decoder, Encoder, Error, Result};

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum FormatTag {
    Png,
    Jpeg,
    Tiff,
    Bmp,
    WebP,
    Heif,
    Avif,
    Jxl,
    Raw,
}

impl FormatTag {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpeg",
            Self::Tiff => "tiff",
            Self::Bmp => "bmp",
            Self::WebP => "webp",
            Self::Heif => "heif",
            Self::Avif => "avif",
            Self::Jxl => "jxl",
            Self::Raw => "raw",
        }
    }

    pub const fn all() -> [Self; 9] {
        [
            Self::Png,
            Self::Jpeg,
            Self::Tiff,
            Self::Bmp,
            Self::WebP,
            Self::Heif,
            Self::Avif,
            Self::Jxl,
            Self::Raw,
        ]
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LosslessCapability {
    Always,
    Never,
    Configurable,
}

pub struct FormatRegistry<'a> {
    decoders: &'a [&'a dyn Decoder],
    encoders: &'a [&'a dyn Encoder],
}

impl<'a> FormatRegistry<'a> {
    pub const fn new(decoders: &'a [&'a dyn Decoder], encoders: &'a [&'a dyn Encoder]) -> Self {
        Self { decoders, encoders }
    }

    pub fn decoder_for(&self, input: &[u8]) -> Result<&'a dyn Decoder> {
        self.decoders
            .iter()
            .find(|decoder| decoder.probe(input))
            .copied()
            .ok_or(Error::UnsupportedFormat)
    }

    pub fn encoder_for(&self, format: FormatTag) -> Result<&'a dyn Encoder> {
        self.encoders
            .iter()
            .find(|encoder| encoder.format() == format)
            .copied()
            .ok_or(Error::UnsupportedFormat)
    }
}
