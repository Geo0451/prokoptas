use crate::{DecodeOptions, DecodedImage, EncodeOptions, FormatTag, Result};

pub trait Decoder: Sync {
    fn format(&self) -> FormatTag;
    fn probe(&self, input: &[u8]) -> bool;
    fn decode(&self, input: &[u8], options: &DecodeOptions) -> Result<DecodedImage>;
}

pub trait Encoder: Sync {
    fn format(&self) -> FormatTag;
    fn lossless_capability(&self) -> crate::LosslessCapability;
    fn encode(&self, image: &DecodedImage, options: &EncodeOptions) -> Result<Vec<u8>>;
}
