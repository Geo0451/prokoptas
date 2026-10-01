use crate::{DecodeOptions, DecodedImage, EncodeOptions, FormatTag, Result};

pub trait Decoder: Sync {
    fn format(&self) -> FormatTag;
    fn probe(&self, input: &[u8]) -> bool;

    fn decode(&self, input: &[u8], options: &DecodeOptions) -> Result<DecodedImage> {
        let mut image = self.decode_native(input, options)?;
        crate::processing::normalize_decoded_image(&mut image, options)?;
        Ok(image)
    }

    fn decode_native(&self, input: &[u8], options: &DecodeOptions) -> Result<DecodedImage>;
}

pub trait Encoder: Sync {
    fn format(&self) -> FormatTag;
    fn lossless_capability(&self) -> crate::LosslessCapability;

    fn encode(&self, image: &DecodedImage, options: &EncodeOptions) -> Result<Vec<u8>> {
        options.validate(self)?;
        let mut prepared = image.clone();
        crate::processing::prepare_for_encoding(&mut prepared, self.format(), options)?;
        self.encode_native(&prepared, options)
    }

    fn encode_native(&self, image: &DecodedImage, options: &EncodeOptions) -> Result<Vec<u8>>;
}
