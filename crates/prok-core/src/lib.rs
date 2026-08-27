#![forbid(unsafe_code)]

use std::fmt;

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
    use super::{Error, ErrorCode};

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
}
