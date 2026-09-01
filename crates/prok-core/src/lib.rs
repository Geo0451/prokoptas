#![forbid(unsafe_code)]

mod bmp;
mod codec;
mod heic;
mod image;
mod jpeg;
mod options;
mod png;
mod raw;
mod registry;
mod tiff;
mod webp;

use std::fmt;

pub use bmp::{BmpDecoder, BmpEncoder, BMP_REGISTRY};
pub use codec::{Decoder, Encoder};
pub use heic::{HeicDecoder, HEIC_REGISTRY};
pub use image::{BitDepth, ColorSpace, DecodedImage, ImageMetadata, Orientation, PixelBuffer};
pub use jpeg::{JpegDecoder, JpegEncoder, JPEG_REGISTRY};
pub use options::{
    ChromaSubsampling, Compression, CropRect, DecodeOptions, DemosaicQuality, EncodeOptions,
    MetadataRetention, PngFilter,
};
pub use png::{PngDecoder, PngEncoder, PNG_REGISTRY};
pub use raw::{RawDecoder, RAW_REGISTRY};
pub use registry::{FormatRegistry, FormatTag, LosslessCapability};
pub use tiff::{TiffDecoder, TiffEncoder, TIFF_REGISTRY};
pub use webp::{WebpDecoder, WebpEncoder, WEBP_REGISTRY};

/// The stable machine-readable categories exposed by the Core boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ErrorCode {
    UnsupportedFormat,
    CorruptData,
    MemoryLimitExceeded,
    IoError,
    InvalidOptions,
    LosslessNotSupported,
    UnsupportedColorSpace,
    EncodingFailed,
}

impl ErrorCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UnsupportedFormat => "unsupported_format",
            Self::CorruptData => "corrupt_data",
            Self::MemoryLimitExceeded => "memory_limit_exceeded",
            Self::IoError => "io_error",
            Self::InvalidOptions => "invalid_options",
            Self::LosslessNotSupported => "lossless_not_supported",
            Self::UnsupportedColorSpace => "unsupported_color_space",
            Self::EncodingFailed => "encoding_failed",
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum Error {
    UnsupportedFormat,
    CorruptData { message: String },
    MemoryLimitExceeded { required_mb: u64, allowed_mb: u64 },
    IoError { message: String },
    InvalidOptions { message: String },
    LosslessNotSupported,
    UnsupportedColorSpace { message: String },
    EncodingFailed { message: String },
}

impl Error {
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::UnsupportedFormat => ErrorCode::UnsupportedFormat,
            Self::CorruptData { .. } => ErrorCode::CorruptData,
            Self::MemoryLimitExceeded { .. } => ErrorCode::MemoryLimitExceeded,
            Self::IoError { .. } => ErrorCode::IoError,
            Self::InvalidOptions { .. } => ErrorCode::InvalidOptions,
            Self::LosslessNotSupported => ErrorCode::LosslessNotSupported,
            Self::UnsupportedColorSpace { .. } => ErrorCode::UnsupportedColorSpace,
            Self::EncodingFailed { .. } => ErrorCode::EncodingFailed,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedFormat => write!(formatter, "unsupported input format"),
            Self::CorruptData { message } => write!(formatter, "corrupt data: {message}"),
            Self::MemoryLimitExceeded {
                required_mb,
                allowed_mb,
            } => write!(
                formatter,
                "decode memory limit exceeded: requires {required_mb} MiB, allowed {allowed_mb} MiB"
            ),
            Self::IoError { message } => write!(formatter, "I/O error: {message}"),
            Self::InvalidOptions { message } => write!(formatter, "invalid options: {message}"),
            Self::LosslessNotSupported => write!(formatter, "lossless encoding is not supported"),
            Self::UnsupportedColorSpace { message } => {
                write!(formatter, "unsupported color space: {message}")
            }
            Self::EncodingFailed { message } => write!(formatter, "encoding failed: {message}"),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::{
        BitDepth, ColorSpace, Compression, DecodeOptions, DecodedImage, Decoder, EncodeOptions,
        Encoder, Error, ErrorCode, FormatRegistry, FormatTag, LosslessCapability, PixelBuffer,
        Result,
    };

    struct TestDecoder;

    impl Decoder for TestDecoder {
        fn format(&self) -> FormatTag {
            FormatTag::Png
        }

        fn probe(&self, input: &[u8]) -> bool {
            input == b"test"
        }

        fn decode(&self, _input: &[u8], _options: &DecodeOptions) -> Result<DecodedImage> {
            unreachable!()
        }
    }

    struct TestEncoder {
        capability: LosslessCapability,
    }

    impl Encoder for TestEncoder {
        fn format(&self) -> FormatTag {
            FormatTag::Png
        }

        fn lossless_capability(&self) -> LosslessCapability {
            self.capability
        }

        fn encode(&self, _image: &DecodedImage, _options: &EncodeOptions) -> Result<Vec<u8>> {
            unreachable!()
        }
    }

    #[test]
    fn error_codes_are_stable_and_machine_readable() {
        assert_eq!(
            Error::LosslessNotSupported.code(),
            ErrorCode::LosslessNotSupported
        );
        assert_eq!(
            ErrorCode::LosslessNotSupported.as_str(),
            "lossless_not_supported"
        );
        assert_eq!(
            Error::MemoryLimitExceeded {
                required_mb: 128,
                allowed_mb: 64,
            }
            .code(),
            ErrorCode::MemoryLimitExceeded
        );
    }

    #[test]
    fn decoded_image_requires_row_major_rgba_storage() {
        let pixels = PixelBuffer::rgba8(vec![0; 16], 2, 2).expect("valid RGBA8 buffer");
        let image = DecodedImage::new(pixels, 2, 2, ColorSpace::Srgb, BitDepth::Eight)
            .expect("valid decoded image");

        assert_eq!(image.pixels.len(), 16);
        assert_eq!(image.orientation, super::Orientation::Normal);
        assert!(!image.orientation_applied);
        assert!(PixelBuffer::rgba8(vec![0; 4], 2, 2).is_err());
    }

    #[test]
    fn compression_validation_is_explicit_and_capability_driven() {
        assert!(Compression::Lossy { quality: 1 }.validate().is_ok());
        assert!(Compression::Lossy { quality: 100 }.validate().is_ok());
        assert!(Compression::Lossy { quality: 0 }.validate().is_err());

        let jpeg = TestEncoder {
            capability: LosslessCapability::Never,
        };
        let lossless = EncodeOptions {
            compression: Compression::Lossless,
            ..EncodeOptions::default()
        };
        assert_eq!(lossless.validate(&jpeg), Err(Error::LosslessNotSupported));
    }

    #[test]
    fn lossless_mode_rejects_chroma_subsampling() {
        let encoder = TestEncoder {
            capability: LosslessCapability::Configurable,
        };
        let options = EncodeOptions {
            compression: Compression::Lossless,
            chroma_subsampling: Some(super::ChromaSubsampling::Yuv420),
            ..EncodeOptions::default()
        };

        assert!(matches!(
            options.validate(&encoder),
            Err(Error::InvalidOptions { .. })
        ));
    }

    #[test]
    fn registry_dispatches_by_probe_and_stable_format_tag() {
        let decoder = TestDecoder;
        let encoder = TestEncoder {
            capability: LosslessCapability::Always,
        };
        let decoders: [&dyn Decoder; 1] = [&decoder];
        let encoders: [&dyn Encoder; 1] = [&encoder];
        let registry = FormatRegistry::new(&decoders, &encoders);

        assert_eq!(
            registry
                .decoder_for(b"test")
                .expect("matching decoder")
                .format(),
            FormatTag::Png
        );
        assert_eq!(
            registry
                .encoder_for(FormatTag::Png)
                .expect("matching encoder")
                .format(),
            FormatTag::Png
        );
        assert!(matches!(
            registry.decoder_for(b"other"),
            Err(Error::UnsupportedFormat)
        ));
        assert_eq!(FormatTag::Jpeg.as_str(), "jpeg");
        assert_eq!(FormatTag::all().len(), 9);
    }
}
