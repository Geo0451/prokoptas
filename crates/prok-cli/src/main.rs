use std::{collections::HashSet, fs, path::PathBuf, process};

use clap::{Parser, ValueEnum};
use prok_core::{
    BitDepth, ChromaSubsampling, Compression, CropRect, DecodeOptions, DemosaicQuality,
    EncodeOptions, FormatTag, MetadataRetention, Result,
};
use rayon::prelude::*;

#[derive(Debug, Parser)]
#[command(
    version,
    about = "Convert still images using the Prokoptas Core pipeline"
)]
struct Cli {
    input: PathBuf,
    output: PathBuf,
    #[arg(
        long,
        value_enum,
        help = "Output format; required for batch conversion"
    )]
    format: Option<OutputFormat>,
    #[arg(long, help = "Convert supported image files in an input directory")]
    batch: bool,
    #[arg(long, default_value_t = default_threads(), value_parser = parse_threads)]
    threads: usize,
    #[arg(long, value_enum)]
    preset: Option<Preset>,
    #[arg(long, conflicts_with = "verbose", short = 'q')]
    quiet: bool,
    #[arg(long, conflicts_with = "quiet")]
    verbose: bool,
    #[arg(long)]
    dec_no_auto_rotate: bool,
    #[arg(long)]
    dec_limit_memory: Option<u64>,
    #[arg(long, value_enum)]
    dec_demosaic: Option<DemosaicMode>,
    #[arg(long)]
    dec_strict_metadata: bool,
    #[arg(long, value_parser = parse_quality, conflicts_with = "enc_lossless")]
    enc_quality: Option<u8>,
    #[arg(long, conflicts_with = "enc_quality")]
    enc_lossless: bool,
    #[arg(long, value_parser = clap::value_parser!(u8).range(0..=10))]
    enc_speed: Option<u8>,
    #[arg(long, value_enum)]
    enc_chroma: Option<ChromaMode>,
    #[arg(long, value_enum)]
    enc_bitdepth: Option<BitDepthMode>,
    #[arg(long, value_enum)]
    enc_png_filter: Option<CliPngFilter>,
    #[arg(long)]
    enc_jxl_noise: bool,
    #[arg(long)]
    enc_jxl_gaborish: bool,
    #[arg(long, conflicts_with = "keep_metadata")]
    strip_metadata: bool,
    #[arg(
        long,
        value_enum,
        value_delimiter = ',',
        conflicts_with = "strip_metadata"
    )]
    keep_metadata: Option<Vec<MetadataField>>,
    #[arg(long)]
    resize_long_edge: Option<u32>,
    #[arg(long, value_parser = parse_crop_rect)]
    crop: Option<CropRect>,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum OutputFormat {
    Png,
    Jpeg,
    Tiff,
    Bmp,
    Webp,
    Avif,
    Jxl,
}

impl OutputFormat {
    fn tag(self) -> FormatTag {
        match self {
            Self::Png => FormatTag::Png,
            Self::Jpeg => FormatTag::Jpeg,
            Self::Tiff => FormatTag::Tiff,
            Self::Bmp => FormatTag::Bmp,
            Self::Webp => FormatTag::WebP,
            Self::Avif => FormatTag::Avif,
            Self::Jxl => FormatTag::Jxl,
        }
    }
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum Preset {
    #[value(name = "web-optimized", alias = "web_optimized")]
    WebOptimized,
    #[value(name = "max-quality-archive", alias = "max_quality_archive")]
    MaxQualityArchive,
    #[value(name = "social-media", alias = "social_media")]
    SocialMedia,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum DemosaicMode {
    Fast,
    #[value(alias = "hq")]
    HighQuality,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum ChromaMode {
    #[value(name = "444")]
    Yuv444,
    #[value(name = "422")]
    Yuv422,
    #[value(name = "420")]
    Yuv420,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum BitDepthMode {
    #[value(name = "8")]
    Eight,
    #[value(name = "10")]
    Ten,
    #[value(name = "12")]
    Twelve,
    #[value(name = "16")]
    Sixteen,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum CliPngFilter {
    Sub,
    Up,
    Average,
    Paeth,
}

impl From<BitDepthMode> for BitDepth {
    fn from(value: BitDepthMode) -> Self {
        match value {
            BitDepthMode::Eight => Self::Eight,
            BitDepthMode::Ten => Self::Ten,
            BitDepthMode::Twelve => Self::Twelve,
            BitDepthMode::Sixteen => Self::Sixteen,
        }
    }
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum MetadataField {
    Exif,
    Iptc,
    Xmp,
}

fn default_threads() -> usize {
    std::thread::available_parallelism()
        .map(|threads| threads.get())
        .unwrap_or(1)
}

fn parse_quality(value: &str) -> std::result::Result<u8, String> {
    let quality = value
        .parse::<u8>()
        .map_err(|_| "quality must be an integer between 1 and 100".to_owned())?;
    if (1..=100).contains(&quality) {
        Ok(quality)
    } else {
        Err("quality must be between 1 and 100".to_owned())
    }
}

fn parse_threads(value: &str) -> std::result::Result<usize, String> {
    let threads = value
        .parse::<usize>()
        .map_err(|_| "thread count must be a positive integer".to_owned())?;
    if threads == 0 {
        Err("thread count must be greater than zero".to_owned())
    } else {
        Ok(threads)
    }
}

fn parse_crop_rect(value: &str) -> std::result::Result<CropRect, String> {
    let values = value
        .split(',')
        .map(str::parse::<u32>)
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|_| "crop must be X,Y,W,H with unsigned integers".to_owned())?;
    if values.len() != 4 || values[2] == 0 || values[3] == 0 {
        return Err("crop must be X,Y,W,H with non-zero width and height".to_owned());
    }
    Ok(CropRect {
        x: values[0],
        y: values[1],
        width: values[2],
        height: values[3],
    })
}

fn run() -> Result<()> {
    run_cli(Cli::parse())
}

fn run_cli(cli: Cli) -> Result<()> {
    if cli.batch {
        run_batch(&cli)
    } else {
        if cli.input.is_dir() {
            return Err(prok_core::Error::InvalidOptions {
                message: "input is a directory; pass --batch and --format for batch conversion"
                    .to_owned(),
            });
        }
        let target = resolve_target(&cli)?;
        let options = encode_options(&cli, target)?;
        let decode = decode_options(&cli);
        convert_file(&cli.input, &cli.output, target, &decode, &options)
            .map_err(|(_, error)| error)?;
        if cli.verbose {
            eprintln!(
                "{} -> {} ({})",
                cli.input.display(),
                cli.output.display(),
                target.as_str()
            );
        } else if !cli.quiet {
            eprintln!(
                "converted {} -> {}",
                cli.input.display(),
                cli.output.display()
            );
        }
        Ok(())
    }
}

fn resolve_target(cli: &Cli) -> Result<FormatTag> {
    let from_extension = cli
        .output
        .extension()
        .and_then(|extension| extension.to_str())
        .and_then(format_from_extension);
    match (cli.format, from_extension) {
        (Some(format), Some(extension_format)) if format.tag() != extension_format => {
            Err(prok_core::Error::InvalidOptions {
                message: "--format does not match the output file extension".to_owned(),
            })
        }
        (Some(format), _) => Ok(format.tag()),
        (None, Some(format)) => Ok(format),
        (None, None) => Err(prok_core::Error::InvalidOptions {
            message: "output extension is unsupported; use --format for extensionless output"
                .to_owned(),
        }),
    }
}

fn format_from_extension(extension: &str) -> Option<FormatTag> {
    match extension.to_ascii_lowercase().as_str() {
        "png" => Some(FormatTag::Png),
        "jpg" | "jpeg" => Some(FormatTag::Jpeg),
        "tif" | "tiff" => Some(FormatTag::Tiff),
        "bmp" => Some(FormatTag::Bmp),
        "webp" => Some(FormatTag::WebP),
        "avif" => Some(FormatTag::Avif),
        "jxl" => Some(FormatTag::Jxl),
        _ => None,
    }
}

fn encode_options(cli: &Cli, target: FormatTag) -> Result<EncodeOptions> {
    let mut options = match cli.preset {
        Some(Preset::WebOptimized) => EncodeOptions::preset_web_optimized(target)?,
        Some(Preset::MaxQualityArchive) => EncodeOptions::preset_max_quality_archive(target)?,
        Some(Preset::SocialMedia) => EncodeOptions::preset_social_media(target)?,
        None => match target {
            FormatTag::Png | FormatTag::Tiff | FormatTag::Bmp | FormatTag::Jxl => EncodeOptions {
                compression: Compression::Lossless,
                ..EncodeOptions::default()
            },
            _ => EncodeOptions::default(),
        },
    };

    if cli.enc_lossless {
        options.compression = Compression::Lossless;
    } else if let Some(quality) = cli.enc_quality {
        options.compression = Compression::Lossy { quality };
    }
    if let Some(speed) = cli.enc_speed {
        options.effort = speed;
    }
    if let Some(chroma) = cli.enc_chroma {
        options.chroma_subsampling = Some(match chroma {
            ChromaMode::Yuv444 => ChromaSubsampling::Yuv444,
            ChromaMode::Yuv422 => ChromaSubsampling::Yuv422,
            ChromaMode::Yuv420 => ChromaSubsampling::Yuv420,
        });
    }
    if let Some(bit_depth) = cli.enc_bitdepth {
        options.bit_depth = bit_depth.into();
    }
    options.png_filter = cli.enc_png_filter.map(|filter| match filter {
        CliPngFilter::Sub => prok_core::PngFilter::Sub,
        CliPngFilter::Up => prok_core::PngFilter::Up,
        CliPngFilter::Average => prok_core::PngFilter::Average,
        CliPngFilter::Paeth => prok_core::PngFilter::Paeth,
    });
    options.jxl_noise_synthesis = cli.enc_jxl_noise;
    options.jxl_gaborish = cli.enc_jxl_gaborish;
    if let Some(long_edge) = cli.resize_long_edge {
        options.resize_long_edge = Some(long_edge);
    }
    if let Some(crop) = cli.crop {
        options.crop = Some(crop);
    }
    if cli.strip_metadata {
        options.metadata_retention = MetadataRetention::default();
    }
    if let Some(fields) = &cli.keep_metadata {
        options.metadata_retention = MetadataRetention {
            exif: fields
                .iter()
                .any(|field| matches!(field, MetadataField::Exif)),
            iptc: fields
                .iter()
                .any(|field| matches!(field, MetadataField::Iptc)),
            xmp: fields
                .iter()
                .any(|field| matches!(field, MetadataField::Xmp)),
        };
    }
    Ok(options)
}

fn decode_options(cli: &Cli) -> DecodeOptions {
    DecodeOptions {
        strict_metadata: cli.dec_strict_metadata,
        auto_rotate: !cli.dec_no_auto_rotate,
        demosaic_quality: match cli.dec_demosaic.unwrap_or(DemosaicMode::Fast) {
            DemosaicMode::Fast => DemosaicQuality::Fast,
            DemosaicMode::HighQuality => DemosaicQuality::HighQuality,
        },
        memory_limit_mb: cli.dec_limit_memory,
        ..DecodeOptions::default()
    }
}

fn run_batch(cli: &Cli) -> Result<()> {
    if !cli.input.is_dir() {
        return Err(prok_core::Error::InvalidOptions {
            message: "--batch requires an input directory".to_owned(),
        });
    }
    let target = cli
        .format
        .ok_or_else(|| prok_core::Error::InvalidOptions {
            message: "--batch requires --format".to_owned(),
        })?
        .tag();
    let options = encode_options(cli, target)?;
    let decode = decode_options(cli);
    let mut files = fs::read_dir(&cli.input)
        .map_err(|error| io_error(&cli.input, error))?
        .map(|entry| {
            entry
                .map(|entry| entry.path())
                .map_err(|error| io_error(&cli.input, error))
        })
        .collect::<Result<Vec<_>>>()?;
    files.retain(|path| path.is_file() && supported_input(path));
    files.sort();
    if files.is_empty() {
        return Err(prok_core::Error::InvalidOptions {
            message: "batch input contains no supported image files".to_owned(),
        });
    }
    fs::create_dir_all(&cli.output).map_err(|error| io_error(&cli.output, error))?;

    let mut outputs = HashSet::new();
    let jobs = files
        .iter()
        .map(|input| {
            let mut output = cli.output.join(input.file_name().unwrap_or_default());
            output.set_extension(output_extension(target));
            (input.clone(), output)
        })
        .collect::<Vec<_>>();
    for (_, output) in &jobs {
        if !outputs.insert(output.clone()) {
            return Err(prok_core::Error::InvalidOptions {
                message: format!("multiple inputs map to output {}", output.display()),
            });
        }
    }

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(cli.threads)
        .build()
        .map_err(|error| prok_core::Error::InvalidOptions {
            message: format!("cannot create worker pool: {error}"),
        })?;
    let results = pool.install(|| {
        jobs.par_iter()
            .map(|(input, output)| {
                convert_file(input, output, target, &decode, &options)
                    .map(|bytes| (input, output, bytes))
            })
            .collect::<Vec<_>>()
    });

    let mut successes = 0usize;
    let mut total_bytes = 0u64;
    let mut first_error = None;
    for result in results {
        match result {
            Ok((input, output, bytes)) => {
                successes += 1;
                total_bytes = total_bytes.saturating_add(bytes);
                if cli.verbose {
                    eprintln!(
                        "{} -> {} ({} bytes)",
                        input.display(),
                        output.display(),
                        bytes
                    );
                }
            }
            Err((input, error)) => {
                eprintln!("{}: {error}", input.display());
                if first_error.is_none() {
                    first_error = Some(error);
                }
            }
        }
    }
    if let Some(error) = first_error {
        return Err(error);
    }
    if !cli.quiet && !cli.verbose {
        eprintln!(
            "converted {successes} files ({} bytes) to {}",
            total_bytes,
            target.as_str()
        );
    }
    Ok(())
}

fn supported_input(path: &std::path::Path) -> bool {
    matches!(
        path.extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some(
            "png"
                | "jpg"
                | "jpeg"
                | "tif"
                | "tiff"
                | "bmp"
                | "webp"
                | "avif"
                | "jxl"
                | "heic"
                | "heif"
                | "cr2"
                | "nef"
                | "arw"
                | "dng"
        )
    )
}

fn output_extension(target: FormatTag) -> &'static str {
    match target {
        FormatTag::Png => "png",
        FormatTag::Jpeg => "jpg",
        FormatTag::Tiff => "tiff",
        FormatTag::Bmp => "bmp",
        FormatTag::WebP => "webp",
        FormatTag::Avif => "avif",
        FormatTag::Jxl => "jxl",
        FormatTag::Heif | FormatTag::Raw => unreachable!("not output formats"),
    }
}

fn convert_file(
    input_path: &std::path::Path,
    output_path: &std::path::Path,
    target: FormatTag,
    decode: &DecodeOptions,
    encode: &EncodeOptions,
) -> std::result::Result<u64, (PathBuf, prok_core::Error)> {
    let input = fs::read(input_path)
        .map_err(|error| (input_path.to_owned(), io_error(input_path, error)))?;
    let output = prok_core::convert(&input, target, decode, encode)
        .map_err(|error| (input_path.to_owned(), error))?;
    if let Some(parent) = output_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)
            .map_err(|error| (output_path.to_owned(), io_error(parent, error)))?;
    }
    let bytes = output.len() as u64;
    fs::write(output_path, output)
        .map_err(|error| (output_path.to_owned(), io_error(output_path, error)))?;
    Ok(bytes)
}

fn io_error(path: &std::path::Path, error: std::io::Error) -> prok_core::Error {
    prok_core::Error::IoError {
        message: format!("{}: {error}", path.display()),
    }
}

fn exit_code(error: &prok_core::Error) -> i32 {
    match error {
        prok_core::Error::UnsupportedFormat => 3,
        prok_core::Error::CorruptData { .. } => 4,
        prok_core::Error::MemoryLimitExceeded { .. } => 5,
        prok_core::Error::IoError { .. } => 6,
        prok_core::Error::EncodingFailed { .. } => 7,
        prok_core::Error::InvalidOptions { .. }
        | prok_core::Error::LosslessNotSupported
        | prok_core::Error::UnsupportedColorSpace { .. } => 2,
    }
}

#[cfg(test)]
fn convert(input_path: &str, output_path: &str) -> Result<()> {
    let cli = Cli::try_parse_from(["prok", input_path, output_path]).map_err(|error| {
        prok_core::Error::InvalidOptions {
            message: error.to_string(),
        }
    })?;
    run_cli(cli)
}

fn main() {
    if let Err(error) = run() {
        eprintln!("prok[{}]: {error}", error.code().as_str());
        process::exit(exit_code(&error));
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;
    use prok_core::{
        AvifDecoder, BitDepth, ColorSpace, Compression, DecodedImage, Decoder, EncodeOptions,
        Encoder, JpegDecoder, JxlDecoder, MetadataRetention, PixelBuffer, PngDecoder, PngEncoder,
        WebpDecoder,
    };

    fn png_bytes() -> Vec<u8> {
        let image = DecodedImage::new(
            PixelBuffer::rgba8(vec![255, 0, 0, 255, 0, 255, 0, 255], 2, 1)
                .expect("valid fixture pixels"),
            2,
            1,
            ColorSpace::Srgb,
            BitDepth::Eight,
        )
        .expect("valid fixture image");
        PngEncoder
            .encode(
                &image,
                &EncodeOptions {
                    compression: Compression::Lossless,
                    bit_depth: BitDepth::Eight,
                    ..EncodeOptions::default()
                },
            )
            .expect("encode PNG fixture")
    }

    #[test]
    fn documented_cli_flags_parse_and_validate_conflicts() {
        let cli = super::Cli::try_parse_from([
            "prok",
            "input.png",
            "output.jpg",
            "--enc-quality",
            "86",
            "--enc-speed",
            "8",
            "--enc-chroma",
            "420",
            "--enc-bitdepth",
            "8",
            "--keep-metadata",
            "exif,xmp",
            "--crop",
            "1,2,30,40",
            "--dec-limit-memory",
            "512",
            "--dec-no-auto-rotate",
            "--quiet",
        ])
        .expect("documented options parse");
        assert_eq!(cli.enc_quality, Some(86));
        assert_eq!(cli.threads, super::default_threads());
        assert!(cli.quiet);
        assert_eq!(cli.crop.expect("crop set").width, 30);

        assert!(super::Cli::try_parse_from([
            "prok",
            "input.png",
            "output.jpg",
            "--quiet",
            "--verbose"
        ])
        .is_err());
        assert!(super::Cli::try_parse_from([
            "prok",
            "input.png",
            "output.jpg",
            "--enc-quality",
            "0"
        ])
        .is_err());
        assert!(
            super::Cli::try_parse_from(["prok", "input.png", "output.jpg", "--threads", "0"])
                .is_err()
        );
    }

    #[test]
    fn batch_conversion_uses_requested_workers_and_output_format() {
        let directory = std::env::temp_dir().join(format!("prok-cli-batch-{}", std::process::id()));
        let input_dir = directory.join("input");
        let output_dir = directory.join("output");
        std::fs::create_dir_all(&input_dir).expect("create input directory");
        std::fs::write(input_dir.join("one.png"), png_bytes()).expect("write first fixture");
        std::fs::write(input_dir.join("two.png"), png_bytes()).expect("write second fixture");

        let cli = super::Cli::try_parse_from([
            "prok",
            input_dir.to_str().expect("UTF-8 input path"),
            output_dir.to_str().expect("UTF-8 output path"),
            "--batch",
            "--format",
            "webp",
            "--threads",
            "2",
            "--quiet",
        ])
        .expect("batch CLI arguments parse");
        super::run_cli(cli).expect("batch conversion succeeds");
        for name in ["one.webp", "two.webp"] {
            let output = std::fs::read(output_dir.join(name)).expect("read batch output");
            assert!(WebpDecoder.probe(&output));
        }
        std::fs::remove_dir_all(directory).expect("remove temporary directory");
    }

    #[test]
    fn memory_limit_is_reported_and_maps_to_resource_exit_code() {
        let directory =
            std::env::temp_dir().join(format!("prok-cli-memory-{}", std::process::id()));
        std::fs::create_dir_all(&directory).expect("create temporary directory");
        let input = directory.join("input.png");
        let output = directory.join("output.png");
        std::fs::write(&input, png_bytes()).expect("write fixture");
        let cli = super::Cli::try_parse_from([
            "prok",
            input.to_str().expect("UTF-8 input path"),
            output.to_str().expect("UTF-8 output path"),
            "--dec-limit-memory",
            "0",
            "--quiet",
        ])
        .expect("CLI arguments parse");
        let error = super::run_cli(cli).expect_err("zero memory budget must fail");
        assert!(matches!(
            error,
            prok_core::Error::MemoryLimitExceeded { .. }
        ));
        assert_eq!(super::exit_code(&error), 5);
        std::fs::remove_dir_all(directory).expect("remove temporary directory");
    }

    #[test]
    fn exit_codes_distinguish_usage_data_and_io_failures() {
        assert_eq!(super::exit_code(&prok_core::Error::UnsupportedFormat), 3);
        assert_eq!(
            super::exit_code(&prok_core::Error::CorruptData {
                message: "bad image".to_owned()
            }),
            4
        );
        assert_eq!(
            super::exit_code(&prok_core::Error::IoError {
                message: "missing file".to_owned()
            }),
            6
        );
    }

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
