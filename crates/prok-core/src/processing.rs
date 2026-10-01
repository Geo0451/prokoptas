use crate::image::{apply_orientation, parse_exif_orientation, set_exif_orientation};
use crate::{
    AvifDecoder, AvifEncoder, BitDepth, BmpDecoder, BmpEncoder, ColorSpace, DecodeOptions,
    DecodedImage, Decoder, EncodeOptions, Encoder, Error, FormatRegistry, FormatTag, HeicDecoder,
    JpegDecoder, JpegEncoder, JxlDecoder, JxlEncoder, PngDecoder, PngEncoder, RawDecoder, Result,
    TiffDecoder, TiffEncoder, WebpDecoder, WebpEncoder,
};

static DECODERS: [&dyn Decoder; 9] = [
    &PngDecoder,
    &JpegDecoder,
    &TiffDecoder,
    &BmpDecoder,
    &WebpDecoder,
    &AvifDecoder,
    &HeicDecoder,
    &JxlDecoder,
    &RawDecoder,
];

static ENCODERS: [&dyn Encoder; 7] = [
    &PngEncoder,
    &JpegEncoder,
    &TiffEncoder,
    &BmpEncoder,
    &WebpEncoder,
    &AvifEncoder,
    &JxlEncoder,
];

static REGISTRY: FormatRegistry<'static> = FormatRegistry::new(&DECODERS, &ENCODERS);

/// Decode, normalize, apply requested output conversions, then encode in `target` format.
pub fn convert(
    input: &[u8],
    target: FormatTag,
    decode_options: &DecodeOptions,
    encode_options: &EncodeOptions,
) -> Result<Vec<u8>> {
    let decoder = REGISTRY.decoder_for(input)?;
    let image = decoder.decode(input, decode_options)?;

    let encoder = REGISTRY.encoder_for(target)?;
    encoder.encode(&image, encode_options)
}

pub(crate) fn normalize_decoded_image(
    image: &mut DecodedImage,
    options: &DecodeOptions,
) -> Result<()> {
    if image.orientation == crate::Orientation::Normal {
        if let Some(orientation) = image
            .metadata
            .exif
            .as_deref()
            .and_then(parse_exif_orientation)
        {
            image.orientation = orientation;
        }
    }

    if let Some(color_space) = options.color_space_override {
        image.convert_color_space(color_space);
    }

    if options.auto_rotate
        && !image.orientation_applied
        && image.orientation != crate::Orientation::Normal
    {
        apply_orientation(image)?;
        if let Some(exif) = image.metadata.exif.as_mut() {
            set_exif_orientation(exif, 1);
        }
    }
    Ok(())
}

pub(crate) fn prepare_for_encoding(
    image: &mut DecodedImage,
    target: FormatTag,
    options: &EncodeOptions,
) -> Result<()> {
    let supports_sixteen_bit = matches!(target, FormatTag::Png | FormatTag::Tiff);
    if options.bit_depth == BitDepth::Sixteen && !supports_sixteen_bit {
        return Err(Error::InvalidOptions {
            message: format!("{} encoding supports 8-bit output only", target.as_str()),
        });
    }
    if matches!(options.bit_depth, BitDepth::Ten | BitDepth::Twelve) {
        return Err(Error::InvalidOptions {
            message: format!(
                "{} encoding does not support {}-bit output",
                target.as_str(),
                options.bit_depth.bits()
            ),
        });
    }

    image.convert_color_space(ColorSpace::Srgb);
    image.convert_bit_depth(options.bit_depth);

    let retention = options.metadata_retention;
    let supports_exif = matches!(target, FormatTag::Png | FormatTag::WebP);
    let supports_xmp = target == FormatTag::WebP;
    if retention.exif && image.metadata.exif.is_some() && !supports_exif {
        return Err(unsupported_metadata(target, "EXIF"));
    }
    if retention.iptc && image.metadata.iptc.is_some() {
        return Err(unsupported_metadata(target, "IPTC"));
    }
    if retention.xmp && image.metadata.xmp.is_some() && !supports_xmp {
        return Err(unsupported_metadata(target, "XMP"));
    }
    if !retention.exif {
        image.metadata.exif = None;
    }
    if !retention.iptc {
        image.metadata.iptc = None;
    }
    if !retention.xmp {
        image.metadata.xmp = None;
    }
    Ok(())
}

fn unsupported_metadata(target: FormatTag, metadata: &str) -> Error {
    Error::InvalidOptions {
        message: format!(
            "{} encoding cannot retain {metadata} metadata",
            target.as_str()
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::{convert, normalize_decoded_image, prepare_for_encoding};
    use crate::{
        AvifDecoder, AvifEncoder, BitDepth, BmpDecoder, BmpEncoder, ColorSpace, Compression,
        DecodeOptions, DecodedImage, Decoder, EncodeOptions, Encoder, FormatTag, ImageMetadata,
        JpegDecoder, JpegEncoder, JxlDecoder, JxlEncoder, MetadataRetention, Orientation,
        PixelBuffer, PngDecoder, PngEncoder, TiffDecoder, TiffEncoder, WebpDecoder, WebpEncoder,
    };

    const FORMATS: [(FormatTag, &dyn Decoder, &dyn Encoder); 7] = [
        (FormatTag::Png, &PngDecoder, &PngEncoder),
        (FormatTag::Jpeg, &JpegDecoder, &JpegEncoder),
        (FormatTag::Tiff, &TiffDecoder, &TiffEncoder),
        (FormatTag::Bmp, &BmpDecoder, &BmpEncoder),
        (FormatTag::WebP, &WebpDecoder, &WebpEncoder),
        (FormatTag::Avif, &AvifDecoder, &AvifEncoder),
        (FormatTag::Jxl, &JxlDecoder, &JxlEncoder),
    ];

    fn fixture() -> DecodedImage {
        DecodedImage::new(
            PixelBuffer::rgba8(
                vec![
                    255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 64, 128, 192, 255,
                ],
                2,
                2,
            )
            .expect("valid matrix fixture pixels"),
            2,
            2,
            ColorSpace::Srgb,
            BitDepth::Eight,
        )
        .expect("valid matrix fixture")
    }

    fn options_for(format: FormatTag) -> EncodeOptions {
        let compression = match format {
            FormatTag::Jpeg | FormatTag::WebP => Compression::Lossy { quality: 85 },
            _ => Compression::Lossless,
        };
        EncodeOptions {
            compression,
            bit_depth: BitDepth::Eight,
            metadata_retention: MetadataRetention::default(),
            ..EncodeOptions::default()
        }
    }

    #[test]
    fn conversion_matrix_supports_every_registered_still_image_pair() {
        let source = fixture();
        let mut encoded_sources = Vec::new();
        for (format, _, encoder) in FORMATS {
            let encoded = encoder
                .encode(&source, &options_for(format))
                .expect("encode matrix source");
            encoded_sources.push((format, encoded));
        }

        for (source_format, source_bytes) in &encoded_sources {
            for (target_format, target_decoder, _) in FORMATS {
                let encoded = convert(
                    source_bytes,
                    target_format,
                    &DecodeOptions::default(),
                    &options_for(target_format),
                )
                .unwrap_or_else(|error| {
                    panic!(
                        "conversion {} -> {} failed: {error}",
                        source_format.as_str(),
                        target_format.as_str()
                    )
                });
                assert!(target_decoder.probe(&encoded));
                let decoded = target_decoder
                    .decode(&encoded, &DecodeOptions::default())
                    .expect("decode matrix output");
                assert_eq!((decoded.width, decoded.height), (2, 2));
            }
        }
    }

    #[test]
    fn color_override_converts_samples_instead_of_relabeling_them() {
        let mut image = DecodedImage::new(
            PixelBuffer::rgba8(vec![128, 128, 128, 255], 1, 1).expect("valid gray pixel"),
            1,
            1,
            ColorSpace::Srgb,
            BitDepth::Eight,
        )
        .expect("valid image");
        normalize_decoded_image(
            &mut image,
            &DecodeOptions {
                color_space_override: Some(ColorSpace::Linear),
                auto_rotate: false,
                ..DecodeOptions::default()
            },
        )
        .expect("normalize image");
        assert_eq!(image.color_space, ColorSpace::Linear);
        assert!(matches!(image.pixels, PixelBuffer::Rgba8(ref pixels) if pixels[0] == 55));
    }

    #[test]
    fn srgb_to_display_p3_uses_transfer_and_primaries() {
        let mut image = DecodedImage::new(
            PixelBuffer::rgba8(vec![255, 0, 0, 255], 1, 1).expect("valid red pixel"),
            1,
            1,
            ColorSpace::Srgb,
            BitDepth::Eight,
        )
        .expect("valid image");
        image.convert_color_space(ColorSpace::DisplayP3);
        assert_eq!(image.color_space, ColorSpace::DisplayP3);
        let PixelBuffer::Rgba8(converted) = image.pixels else {
            panic!("expected RGBA8 conversion");
        };
        assert!(converted[0].abs_diff(234) <= 1);
        assert!(converted[1].abs_diff(51) <= 1);
        assert!(converted[2].abs_diff(35) <= 1);
        assert_eq!(converted[3], 255);
    }

    #[test]
    fn bit_depth_conversion_scales_sample_ranges() {
        let mut image = DecodedImage::new(
            PixelBuffer::rgba16(vec![0, 32768, 65535, 65535], 1, 1).expect("valid 16-bit pixel"),
            1,
            1,
            ColorSpace::Srgb,
            BitDepth::Sixteen,
        )
        .expect("valid image");
        image.convert_bit_depth(BitDepth::Eight);
        assert_eq!(image.bit_depth, BitDepth::Eight);
        assert_eq!(image.pixels, PixelBuffer::Rgba8(vec![0, 128, 255, 255]));
        image.convert_bit_depth(BitDepth::Sixteen);
        assert_eq!(
            image.pixels,
            PixelBuffer::Rgba16(vec![0, 32896, 65535, 65535])
        );
    }

    #[test]
    fn auto_rotation_resets_retained_exif_orientation() {
        let mut image = fixture();
        image.width = 2;
        image.height = 2;
        image.orientation = Orientation::Rotate90;
        let mut exif = vec![
            b'I', b'I', 42, 0, 8, 0, 0, 0, 1, 0, 18, 1, 3, 0, 1, 0, 0, 0, 6, 0, 0, 0,
        ];
        image.metadata = ImageMetadata {
            exif: Some(exif.clone()),
            ..ImageMetadata::default()
        };
        normalize_decoded_image(&mut image, &DecodeOptions::default())
            .expect("normalize rotated image");
        exif[18] = 1;
        assert_eq!(image.metadata.exif, Some(exif));
        assert!(image.orientation_applied);
    }

    #[test]
    fn metadata_retention_is_selective_and_rejects_unsupported_fields() {
        let mut image = fixture();
        image.metadata = ImageMetadata {
            exif: Some(vec![1, 2, 3]),
            iptc: Some(vec![4, 5]),
            xmp: Some(vec![6, 7]),
        };
        let retain_exif = EncodeOptions {
            metadata_retention: MetadataRetention {
                exif: true,
                ..MetadataRetention::default()
            },
            ..options_for(FormatTag::Png)
        };
        prepare_for_encoding(&mut image, FormatTag::Png, &retain_exif)
            .expect("PNG can retain EXIF");
        assert!(image.metadata.exif.is_some());
        assert!(image.metadata.iptc.is_none());
        assert!(image.metadata.xmp.is_none());

        let mut image = fixture();
        image.metadata.xmp = Some(vec![1, 2, 3]);
        let retain_xmp = EncodeOptions {
            metadata_retention: MetadataRetention {
                xmp: true,
                ..MetadataRetention::default()
            },
            ..options_for(FormatTag::Png)
        };
        assert!(prepare_for_encoding(&mut image, FormatTag::Png, &retain_xmp).is_err());

        let mut image = fixture();
        image.metadata = ImageMetadata {
            exif: Some(vec![1]),
            xmp: Some(vec![2]),
            ..ImageMetadata::default()
        };
        prepare_for_encoding(
            &mut image,
            FormatTag::WebP,
            &EncodeOptions {
                metadata_retention: MetadataRetention {
                    exif: true,
                    xmp: true,
                    ..MetadataRetention::default()
                },
                ..options_for(FormatTag::WebP)
            },
        )
        .expect("WebP can retain EXIF and XMP");
        assert!(image.metadata.exif.is_some());
        assert!(image.metadata.xmp.is_some());
    }

    #[test]
    fn sixteen_bit_png_and_tiff_conversions_preserve_samples() {
        let source = DecodedImage::new(
            PixelBuffer::rgba16(vec![1000, 32768, 65535, 65535], 1, 1)
                .expect("valid 16-bit source pixels"),
            1,
            1,
            ColorSpace::Srgb,
            BitDepth::Sixteen,
        )
        .expect("valid 16-bit source");
        let source_bytes = PngEncoder
            .encode(
                &source,
                &EncodeOptions {
                    compression: Compression::Lossless,
                    bit_depth: BitDepth::Sixteen,
                    ..options_for(FormatTag::Png)
                },
            )
            .expect("encode 16-bit PNG source");

        for (format, decoder) in [
            (FormatTag::Png, &PngDecoder as &dyn Decoder),
            (FormatTag::Tiff, &TiffDecoder as &dyn Decoder),
        ] {
            let output = convert(
                &source_bytes,
                format,
                &DecodeOptions::default(),
                &EncodeOptions {
                    compression: Compression::Lossless,
                    bit_depth: BitDepth::Sixteen,
                    ..options_for(format)
                },
            )
            .expect("convert 16-bit image");
            let decoded = decoder
                .decode(&output, &DecodeOptions::default())
                .expect("decode 16-bit output");
            assert_eq!(decoded.bit_depth, BitDepth::Sixteen);
            assert_eq!(decoded.pixels, source.pixels);
        }
    }

    #[test]
    fn retained_exif_does_not_reapply_applied_orientation() {
        let exif = vec![
            b'I', b'I', 42, 0, 8, 0, 0, 0, 1, 0, 18, 1, 3, 0, 1, 0, 0, 0, 6, 0, 0, 0,
        ];
        let source = DecodedImage::new(
            PixelBuffer::rgba8(vec![255, 0, 0, 255, 0, 255, 0, 255], 2, 1)
                .expect("valid orientation pixels"),
            2,
            1,
            ColorSpace::Srgb,
            BitDepth::Eight,
        )
        .expect("valid orientation image");
        let source_bytes = PngEncoder
            .encode(
                &DecodedImage {
                    metadata: ImageMetadata {
                        exif: Some(exif),
                        ..ImageMetadata::default()
                    },
                    ..source
                },
                &EncodeOptions {
                    compression: Compression::Lossless,
                    metadata_retention: MetadataRetention {
                        exif: true,
                        ..MetadataRetention::default()
                    },
                    ..options_for(FormatTag::Png)
                },
            )
            .expect("encode oriented PNG source");
        let output = convert(
            &source_bytes,
            FormatTag::Png,
            &DecodeOptions::default(),
            &EncodeOptions {
                compression: Compression::Lossless,
                metadata_retention: MetadataRetention {
                    exif: true,
                    ..MetadataRetention::default()
                },
                ..options_for(FormatTag::Png)
            },
        )
        .expect("convert oriented PNG");
        let decoded = PngDecoder
            .decode(
                &output,
                &DecodeOptions {
                    auto_rotate: false,
                    ..DecodeOptions::default()
                },
            )
            .expect("decode normalized output");
        assert_eq!((decoded.width, decoded.height), (1, 2));
        assert_eq!(decoded.orientation, Orientation::Normal);
    }

    #[test]
    fn unsupported_bit_depth_is_rejected() {
        let mut image = fixture();
        let options = EncodeOptions {
            bit_depth: BitDepth::Ten,
            ..options_for(FormatTag::Jpeg)
        };
        assert!(prepare_for_encoding(&mut image, FormatTag::Jpeg, &options).is_err());
    }
}
