use std::{env, fs};

use prok_core::{Compression, EncodeOptions, FormatTag, MetadataRetention, Result, PNG_REGISTRY};

fn run() -> Result<()> {
    let mut arguments = env::args().skip(1);
    let input_path = arguments
        .next()
        .ok_or_else(|| prok_core::Error::InvalidOptions {
            message: "usage: prok <input.png> <output.png>".to_owned(),
        })?;
    let output_path = arguments
        .next()
        .ok_or_else(|| prok_core::Error::InvalidOptions {
            message: "usage: prok <input.png> <output.png>".to_owned(),
        })?;
    if arguments.next().is_some() {
        return Err(prok_core::Error::InvalidOptions {
            message: "usage: prok <input.png> <output.png>".to_owned(),
        });
    }

    convert(&input_path, &output_path)
}

fn convert(input_path: &str, output_path: &str) -> Result<()> {
    let input = fs::read(input_path).map_err(|error| prok_core::Error::IoError {
        message: error.to_string(),
    })?;
    let decoder = PNG_REGISTRY.decoder_for(&input)?;
    let image = decoder.decode(&input, &Default::default())?;
    let bit_depth = image.bit_depth;
    let encoder = PNG_REGISTRY.encoder_for(FormatTag::Png)?;
    let options = EncodeOptions {
        compression: Compression::Lossless,
        bit_depth,
        metadata_retention: MetadataRetention {
            exif: true,
            iptc: true,
            xmp: true,
        },
        ..EncodeOptions::default()
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
        MetadataRetention, PixelBuffer, PngDecoder, PngEncoder,
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
}
