use std::{env, fs};

use prok_core::{
    BmpDecoder, BmpEncoder, Compression, EncodeOptions, HeicDecoder, JpegDecoder, JpegEncoder,
    MetadataRetention, PngDecoder, PngEncoder, RawDecoder, Result, TiffDecoder, TiffEncoder,
    WebpDecoder, WebpEncoder, BMP_REGISTRY, HEIC_REGISTRY, JPEG_REGISTRY, PNG_REGISTRY,
    RAW_REGISTRY, TIFF_REGISTRY, WEBP_REGISTRY,
};

fn run() -> Result<()> {
    let mut arguments = env::args().skip(1);
    let input_path = arguments
        .next()
        .ok_or_else(|| prok_core::Error::InvalidOptions {
            message:
                "usage: prok <input.(png|jpg|jpeg|webp|tiff|bmp|heic|heif)> <output.(png|jpg|jpeg|webp|tiff|bmp)>"
                    .to_owned(),
        })?;
    let output_path = arguments
        .next()
        .ok_or_else(|| prok_core::Error::InvalidOptions {
            message:
                "usage: prok <input.(png|jpg|jpeg|webp|tiff|bmp|heic|heif)> <output.(png|jpg|jpeg|webp|tiff|bmp)>"
                    .to_owned(),
        })?;
    if arguments.next().is_some() {
        return Err(prok_core::Error::InvalidOptions {
            message:
                "usage: prok <input.(png|jpg|jpeg|webp|tiff|bmp|heic|heif)> <output.(png|jpg|jpeg|webp|tiff|bmp)>"
                    .to_owned(),
        });
    }

    convert(&input_path, &output_path)
}

fn convert(input_path: &str, output_path: &str) -> Result<()> {
    let input = fs::read(input_path).map_err(|error| prok_core::Error::IoError {
        message: error.to_string(),
    })?;
    let decoder = if PNG_REGISTRY.decoder_for(&input).is_ok() {
        &PngDecoder as &dyn prok_core::Decoder
    } else if JPEG_REGISTRY.decoder_for(&input).is_ok() {
        &JpegDecoder as &dyn prok_core::Decoder
    } else if WEBP_REGISTRY.decoder_for(&input).is_ok() {
        &WebpDecoder as &dyn prok_core::Decoder
    } else if RAW_REGISTRY.decoder_for(&input).is_ok() {
        &RawDecoder as &dyn prok_core::Decoder
    } else if TIFF_REGISTRY.decoder_for(&input).is_ok() {
        &TiffDecoder as &dyn prok_core::Decoder
    } else if BMP_REGISTRY.decoder_for(&input).is_ok() {
        &BmpDecoder as &dyn prok_core::Decoder
    } else if HEIC_REGISTRY.decoder_for(&input).is_ok() {
        &HeicDecoder as &dyn prok_core::Decoder
    } else {
        return Err(prok_core::Error::UnsupportedFormat);
    };
    let image = decoder.decode(&input, &Default::default())?;
    let bit_depth = image.bit_depth;
    let output_format = output_path
        .rsplit('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let (encoder, options): (&dyn prok_core::Encoder, EncodeOptions) = match output_format.as_str()
    {
        "png" => (
            &PngEncoder,
            EncodeOptions {
                compression: Compression::Lossless,
                bit_depth,
                metadata_retention: MetadataRetention {
                    exif: true,
                    iptc: true,
                    xmp: true,
                },
                ..EncodeOptions::default()
            },
        ),
        "jpg" | "jpeg" => (
            &JpegEncoder,
            EncodeOptions {
                compression: Compression::Lossy { quality: 75 },
                bit_depth: prok_core::BitDepth::Eight,
                chroma_subsampling: Some(prok_core::ChromaSubsampling::Yuv420),
                ..EncodeOptions::default()
            },
        ),
        "webp" => (
            &WebpEncoder,
            EncodeOptions {
                compression: Compression::Lossy { quality: 75 },
                bit_depth: prok_core::BitDepth::Eight,
                ..EncodeOptions::default()
            },
        ),
        "tiff" | "tif" => (
            &TiffEncoder,
            EncodeOptions {
                compression: Compression::Lossless,
                bit_depth,
                ..EncodeOptions::default()
            },
        ),
        "bmp" => (
            &BmpEncoder,
            EncodeOptions {
                compression: Compression::Lossless,
                bit_depth,
                ..EncodeOptions::default()
            },
        ),
        _ => {
            return Err(prok_core::Error::InvalidOptions {
                message: "output extension must be .png, .jpg, .jpeg, .webp, .tiff, or .bmp".to_owned(),
            });
        }
    };
    let output = encoder.encode(&image, &options)?;
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
        BitDepth, ColorSpace, Compression, DecodedImage, Decoder, EncodeOptions, Encoder,
        JpegDecoder, MetadataRetention, PixelBuffer, PngDecoder, PngEncoder, WebpDecoder,
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
