use std::{env, fs};

use prok_core::{Compression, EncodeOptions, FormatTag, MetadataRetention, Result};

fn run() -> Result<()> {
    let mut arguments = env::args().skip(1);
    let input_path = arguments
        .next()
        .ok_or_else(|| prok_core::Error::InvalidOptions {
            message:
                "usage: prok <input.(png|jpg|jpeg|webp|tiff|bmp|heic|heif)> <output.(png|jpg|jpeg|webp|tiff|bmp|avif|jxl)>"
                    .to_owned(),
        })?;
    let output_path = arguments
        .next()
        .ok_or_else(|| prok_core::Error::InvalidOptions {
            message:
                "usage: prok <input.(png|jpg|jpeg|webp|tiff|bmp|heic|heif)> <output.(png|jpg|jpeg|webp|tiff|bmp|avif|jxl)>"
                    .to_owned(),
        })?;
    if arguments.next().is_some() {
        return Err(prok_core::Error::InvalidOptions {
            message:
                "usage: prok <input.(png|jpg|jpeg|webp|tiff|bmp|heic|heif)> <output.(png|jpg|jpeg|webp|tiff|bmp|avif|jxl)>"
                    .to_owned(),
        });
    }

    convert(&input_path, &output_path)
}

fn convert(input_path: &str, output_path: &str) -> Result<()> {
    let input = fs::read(input_path).map_err(|error| prok_core::Error::IoError {
        message: error.to_string(),
    })?;
    let output_format = output_path
        .rsplit('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let (target, options): (FormatTag, EncodeOptions) = match output_format.as_str() {
        "png" => (
            FormatTag::Png,
            EncodeOptions {
                compression: Compression::Lossless,
                bit_depth: prok_core::BitDepth::Eight,
                metadata_retention: MetadataRetention {
                    exif: true,
                    ..MetadataRetention::default()
                },
                ..EncodeOptions::default()
            },
        ),
        "jpg" | "jpeg" => (
            FormatTag::Jpeg,
            EncodeOptions {
                compression: Compression::Lossy { quality: 75 },
                bit_depth: prok_core::BitDepth::Eight,
                chroma_subsampling: Some(prok_core::ChromaSubsampling::Yuv420),
                ..EncodeOptions::default()
            },
        ),
        "webp" => (
            FormatTag::WebP,
            EncodeOptions {
                compression: Compression::Lossy { quality: 75 },
                bit_depth: prok_core::BitDepth::Eight,
                ..EncodeOptions::default()
            },
        ),
        "tiff" | "tif" => (
            FormatTag::Tiff,
            EncodeOptions {
                compression: Compression::Lossless,
                bit_depth: prok_core::BitDepth::Eight,
                ..EncodeOptions::default()
            },
        ),
        "bmp" => (
            FormatTag::Bmp,
            EncodeOptions {
                compression: Compression::Lossless,
                bit_depth: prok_core::BitDepth::Eight,
                ..EncodeOptions::default()
            },
        ),
        "avif" => (
            FormatTag::Avif,
            EncodeOptions {
                compression: Compression::Lossless,
                bit_depth: prok_core::BitDepth::Eight,
                ..EncodeOptions::default()
            },
        ),
        "jxl" => (
            FormatTag::Jxl,
            EncodeOptions {
                compression: Compression::Lossless,
                bit_depth: prok_core::BitDepth::Eight,
                ..EncodeOptions::default()
            },
        ),
        _ => {
            return Err(prok_core::Error::InvalidOptions {
                message:
                    "output extension must be .png, .jpg, .jpeg, .webp, .tiff, .bmp, .avif, or .jxl"
                        .to_owned(),
            });
        }
    };
    let output = prok_core::convert(&input, target, &Default::default(), &options)?;
    fs::write(output_path, output).map_err(|error| prok_core::Error::IoError {
        message: error.to_string(),
    })?;
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{}: {error}", error.code().as_str());
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use prok_core::{
        AvifDecoder, BitDepth, ColorSpace, Compression, DecodedImage, Decoder, EncodeOptions,
        Encoder, JpegDecoder, JxlDecoder, MetadataRetention, PixelBuffer, PngDecoder, PngEncoder,
        WebpDecoder,
    };

    #[test]
    fn cli_converts_png_input_to_png_output() {
        let directory = std::env::temp_dir().join(format!("prok-cli-{}", std::process::id()));
        std::fs::create_dir_all(&directory).expect("create temporary directory");
        let input_path = directory.join("input.png");
        let output_path = directory.join("output.png");
        let source = DecodedImage::new(
            PixelBuffer::rgba8(vec![255, 0, 0, 255, 0, 0, 255, 255], 2, 1)
                .expect("valid fixture pixels"),
            2,
            1,
            ColorSpace::Srgb,
            BitDepth::Eight,
        )
        .expect("valid fixture image");
        let encoded = PngEncoder
            .encode(
                &source,
                &EncodeOptions {
                    compression: Compression::Lossless,
                    bit_depth: BitDepth::Eight,
                    metadata_retention: MetadataRetention::default(),
                    ..EncodeOptions::default()
                },
            )
            .expect("encode fixture");
        std::fs::write(&input_path, encoded).expect("write input fixture");

        super::convert(
            input_path.to_str().expect("UTF-8 input path"),
            output_path.to_str().expect("UTF-8 output path"),
        )
        .expect("run prok conversion");

        let output = std::fs::read(&output_path).expect("read output fixture");
        let decoded = PngDecoder
            .decode(&output, &Default::default())
            .expect("decode output fixture");
        assert_eq!(decoded.pixels, source.pixels);
        std::fs::remove_dir_all(directory).expect("remove temporary directory");
    }

    #[test]
    fn cli_converts_png_input_to_jpeg_output() {
        let directory = std::env::temp_dir().join(format!("prok-cli-jpeg-{}", std::process::id()));
        std::fs::create_dir_all(&directory).expect("create temporary directory");
        let input_path = directory.join("input.png");
        let output_path = directory.join("output.jpg");
        let source = DecodedImage::new(
            PixelBuffer::rgba8(vec![255, 0, 0, 255, 0, 255, 0, 255], 2, 1)
                .expect("valid fixture pixels"),
            2,
            1,
            ColorSpace::Srgb,
            BitDepth::Eight,
        )
        .expect("valid fixture image");
        let encoded = PngEncoder
            .encode(
                &source,
                &EncodeOptions {
                    compression: Compression::Lossless,
                    bit_depth: BitDepth::Eight,
                    ..EncodeOptions::default()
                },
            )
            .expect("encode fixture");
        std::fs::write(&input_path, encoded).expect("write input fixture");

        super::convert(
            input_path.to_str().expect("UTF-8 input path"),
            output_path.to_str().expect("UTF-8 output path"),
        )
        .expect("run JPEG conversion");

        let output = std::fs::read(&output_path).expect("read JPEG output");
        assert!(JpegDecoder.probe(&output));
        let decoded = JpegDecoder
            .decode(&output, &Default::default())
            .expect("decode JPEG output");
        assert_eq!((decoded.width, decoded.height), (2, 1));
    }

    #[test]
    fn cli_converts_png_input_to_avif_and_jxl_output() {
        let directory =
            std::env::temp_dir().join(format!("prok-cli-avif-jxl-{}", std::process::id()));
        std::fs::create_dir_all(&directory).expect("create temporary directory");
        let input_path = directory.join("input.png");
        let avif_output_path = directory.join("output.avif");
        let jxl_output_path = directory.join("output.jxl");
        let source = DecodedImage::new(
            PixelBuffer::rgba8(vec![255, 0, 0, 255, 0, 255, 0, 255], 2, 1)
                .expect("valid fixture pixels"),
            2,
            1,
            ColorSpace::Srgb,
            BitDepth::Eight,
        )
        .expect("valid fixture image");
        let encoded = PngEncoder
            .encode(
                &source,
                &EncodeOptions {
                    compression: Compression::Lossless,
                    bit_depth: BitDepth::Eight,
                    ..EncodeOptions::default()
                },
            )
            .expect("encode fixture");
        std::fs::write(&input_path, encoded).expect("write input fixture");

        super::convert(
            input_path.to_str().expect("UTF-8 input path"),
            avif_output_path.to_str().expect("UTF-8 avif output path"),
        )
        .expect("run AVIF conversion");
        super::convert(
            input_path.to_str().expect("UTF-8 input path"),
            jxl_output_path.to_str().expect("UTF-8 jxl output path"),
        )
        .expect("run JXL conversion");

        let avif_output = std::fs::read(&avif_output_path).expect("read AVIF output");
        let jxl_output = std::fs::read(&jxl_output_path).expect("read JXL output");
        assert!(AvifDecoder.probe(&avif_output));
        assert!(JxlDecoder.probe(&jxl_output));
        assert!(!avif_output.is_empty());
        assert!(!jxl_output.is_empty());
        std::fs::remove_dir_all(directory).expect("remove temporary directory");
    }

    #[test]
    fn cli_converts_png_input_to_webp_output() {
        let directory = std::env::temp_dir().join(format!("prok-cli-webp-{}", std::process::id()));
        std::fs::create_dir_all(&directory).expect("create temporary directory");
        let input_path = directory.join("input.png");
        let output_path = directory.join("output.webp");
        let source = DecodedImage::new(
            PixelBuffer::rgba8(vec![255, 0, 0, 255, 0, 255, 0, 255], 2, 1)
                .expect("valid fixture pixels"),
            2,
            1,
            ColorSpace::Srgb,
            BitDepth::Eight,
        )
        .expect("valid fixture image");
        let encoded = PngEncoder
            .encode(
                &source,
                &EncodeOptions {
                    compression: Compression::Lossless,
                    bit_depth: BitDepth::Eight,
                    ..EncodeOptions::default()
                },
            )
            .expect("encode fixture");
        std::fs::write(&input_path, encoded).expect("write input fixture");

        super::convert(
            input_path.to_str().expect("UTF-8 input path"),
            output_path.to_str().expect("UTF-8 output path"),
        )
        .expect("run WebP conversion");

        let output = std::fs::read(&output_path).expect("read WebP output");
        assert!(WebpDecoder.probe(&output));
        let decoded = WebpDecoder
            .decode(&output, &Default::default())
            .expect("decode WebP output");
        assert_eq!((decoded.width, decoded.height), (2, 1));
        std::fs::remove_dir_all(directory).expect("remove temporary directory");
    }
}
