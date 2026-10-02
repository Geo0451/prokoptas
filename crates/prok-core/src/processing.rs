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

pub(crate) fn encoder_for(target: FormatTag) -> Result<&'static dyn Encoder> {
    REGISTRY.encoder_for(target)
}

pub fn probe_format(input: &[u8]) -> Option<FormatTag> {
    REGISTRY.decoder_for(input).ok().map(Decoder::format)
}

pub fn lossless_capability(target: FormatTag) -> Result<crate::LosslessCapability> {
    Ok(encoder_for(target)?.lossless_capability())
}

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
    if options.png_filter.is_some() && target != FormatTag::Png {
        return Err(Error::InvalidOptions {
            message: "PNG filter can only be used with PNG output".to_owned(),
        });
    }
    if options.chroma_subsampling.is_some() && target != FormatTag::Jpeg {
        return Err(Error::InvalidOptions {
            message: "chroma subsampling is currently supported only for JPEG output".to_owned(),
        });
    }
    if (options.jxl_noise_synthesis || options.jxl_gaborish) && target != FormatTag::Jxl {
        return Err(Error::InvalidOptions {
            message: "JXL tuning options require JXL output".to_owned(),
        });
    }
    if (options.jxl_noise_synthesis || options.jxl_gaborish)
        && matches!(options.compression, crate::Compression::Lossless)
    {
        return Err(Error::InvalidOptions {
            message: "JXL noise synthesis and Gaborish are available only in lossy mode".to_owned(),
        });
    }
    apply_image_transforms(image, options)?;
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
    let supports_exif = matches!(target, FormatTag::Png | FormatTag::WebP | FormatTag::Avif);
    let supports_xmp = matches!(target, FormatTag::WebP | FormatTag::Avif);
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

fn apply_image_transforms(image: &mut DecodedImage, options: &EncodeOptions) -> Result<()> {
    if let Some(crop) = options.crop {
        let right = crop.x.checked_add(crop.width);
        let bottom = crop.y.checked_add(crop.height);
        if crop.width == 0
            || crop.height == 0
            || right.is_none_or(|right| right > image.width)
            || bottom.is_none_or(|bottom| bottom > image.height)
        {
            return Err(Error::InvalidOptions {
                message: "crop rectangle must be non-empty and inside the image".to_owned(),
            });
        }

        match &image.pixels {
            crate::PixelBuffer::Rgba8(data) => {
                let buffer = ::image::RgbaImage::from_raw(image.width, image.height, data.clone())
                    .ok_or_else(|| Error::CorruptData {
                        message: "crop source buffer size mismatch".to_owned(),
                    })?;
                image.pixels = crate::PixelBuffer::Rgba8(
                    ::image::imageops::crop_imm(&buffer, crop.x, crop.y, crop.width, crop.height)
                        .to_image()
                        .into_raw(),
                );
            }
            crate::PixelBuffer::Rgba16(data) => {
                let buffer = ::image::ImageBuffer::<::image::Rgba<u16>, Vec<u16>>::from_raw(
                    image.width,
                    image.height,
                    data.clone(),
                )
                .ok_or_else(|| Error::CorruptData {
                    message: "crop source buffer size mismatch".to_owned(),
                })?;
                image.pixels = crate::PixelBuffer::Rgba16(
                    ::image::imageops::crop_imm(&buffer, crop.x, crop.y, crop.width, crop.height)
                        .to_image()
                        .into_raw(),
                );
            }
        }
        image.width = crop.width;
        image.height = crop.height;
    }

    if let Some(long_edge) = options.resize_long_edge {
        if long_edge == 0 {
            return Err(Error::InvalidOptions {
                message: "resize long edge must be greater than zero".to_owned(),
            });
        }
        let current_long_edge = image.width.max(image.height);
        if current_long_edge != long_edge {
            let width = (u64::from(image.width) * u64::from(long_edge)
                + u64::from(current_long_edge / 2))
                / u64::from(current_long_edge);
            let height = (u64::from(image.height) * u64::from(long_edge)
                + u64::from(current_long_edge / 2))
                / u64::from(current_long_edge);
            let width = width.max(1) as u32;
            let height = height.max(1) as u32;
            match &image.pixels {
                crate::PixelBuffer::Rgba8(data) => {
                    let buffer =
                        ::image::RgbaImage::from_raw(image.width, image.height, data.clone())
                            .ok_or_else(|| Error::CorruptData {
                                message: "resize source buffer size mismatch".to_owned(),
                            })?;
                    image.pixels = crate::PixelBuffer::Rgba8(
                        ::image::imageops::resize(
                            &buffer,
                            width,
                            height,
                            ::image::imageops::FilterType::Lanczos3,
                        )
                        .into_raw(),
                    );
                }
                crate::PixelBuffer::Rgba16(data) => {
                    let buffer = ::image::ImageBuffer::<::image::Rgba<u16>, Vec<u16>>::from_raw(
                        image.width,
                        image.height,
                        data.clone(),
                    )
                    .ok_or_else(|| Error::CorruptData {
                        message: "resize source buffer size mismatch".to_owned(),
                    })?;
                    image.pixels = crate::PixelBuffer::Rgba16(
                        ::image::imageops::resize(
                            &buffer,
                            width,
                            height,
                            ::image::imageops::FilterType::Lanczos3,
                        )
                        .into_raw(),
                    );
                }
            }
            image.width = width;
            image.height = height;
        }
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
        DecodeOptions, DecodedImage, Decoder, EncodeOptions, Encoder, Error, FormatTag,
        ImageMetadata, JpegDecoder, JpegEncoder, JxlDecoder, JxlEncoder, LosslessCapability,
        MetadataRetention, Orientation, PixelBuffer, PngDecoder, PngEncoder, TiffDecoder,
        TiffEncoder, WebpDecoder, WebpEncoder,
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
            FormatTag::Jpeg | FormatTag::WebP | FormatTag::Avif => {
                Compression::Lossy { quality: 85 }
            }
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
    fn archive_presets_are_pixel_exact_for_lossless_targets_only() {
        let source = fixture();
        let source_bytes = PngEncoder
            .encode(&source, &options_for(FormatTag::Png))
            .expect("encode source image");

        for (format, decoder, _) in FORMATS {
            let options = match EncodeOptions::preset_max_quality_archive(format) {
                Ok(options) => options,
                Err(Error::LosslessNotSupported) => {
                    assert_eq!(
                        crate::processing::encoder_for(format)
                            .expect("registered encoder")
                            .lossless_capability(),
                        LosslessCapability::Never
                    );
                    continue;
                }
                Err(error) => panic!("unexpected {format:?} archive preset error: {error}"),
            };
            assert_eq!(options.compression, Compression::Lossless);

            let output = convert(&source_bytes, format, &DecodeOptions::default(), &options)
                .unwrap_or_else(|error| panic!("{format:?} archive conversion failed: {error}"));
            let decoded = decoder
                .decode(&output, &DecodeOptions::default())
                .unwrap_or_else(|error| panic!("{format:?} archive output failed: {error}"));
            let expected = if options.bit_depth == BitDepth::Sixteen {
                let PixelBuffer::Rgba8(pixels) = &source.pixels else {
                    panic!("fixture must be RGBA8");
                };
                PixelBuffer::Rgba16(
                    pixels
                        .iter()
                        .map(|sample| u16::from(*sample) * 257)
                        .collect(),
                )
            } else {
                source.pixels.clone()
            };
            assert_eq!(
                decoded.pixels, expected,
                "{format:?} archive samples changed"
            );
        }
    }

    #[test]
    fn large_image_memory_limits_reject_before_rgba_render() {
        let width = 513;
        let height = 513;
        let pixels = vec![127; width * height * 4];
        let image = DecodedImage::new(
            PixelBuffer::rgba8(pixels, width as u32, height as u32)
                .expect("valid large image pixels"),
            width as u32,
            height as u32,
            ColorSpace::Srgb,
            BitDepth::Eight,
        )
        .expect("valid large image");
        let png = PngEncoder
            .encode(&image, &options_for(FormatTag::Png))
            .expect("encode large PNG");
        let avif = AvifEncoder
            .encode(&image, &options_for(FormatTag::Avif))
            .expect("encode large AVIF");
        let jxl = JxlEncoder
            .encode(&image, &options_for(FormatTag::Jxl))
            .expect("encode large JXL");
        let decode_options = DecodeOptions {
            memory_limit_mb: Some(1),
            ..DecodeOptions::default()
        };

        for (format, bytes) in [
            (FormatTag::Png, png),
            (FormatTag::Avif, avif),
            (FormatTag::Jxl, jxl),
        ] {
            let result = convert(
                &bytes,
                FormatTag::Png,
                &decode_options,
                &options_for(FormatTag::Png),
            );
            assert_eq!(
                result,
                Err(Error::MemoryLimitExceeded {
                    required_mb: 2,
                    allowed_mb: 1,
                }),
                "{format:?} should reject the oversized decoded buffer"
            );
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
    fn crop_and_long_edge_resize_transform_dimensions_and_samples() {
        let mut image = fixture();
        prepare_for_encoding(
            &mut image,
            FormatTag::Png,
            &EncodeOptions {
                crop: Some(crate::CropRect {
                    x: 1,
                    y: 0,
                    width: 1,
                    height: 2,
                }),
                ..options_for(FormatTag::Png)
            },
        )
        .expect("crop image");
        assert_eq!((image.width, image.height), (1, 2));
        assert_eq!(
            image.pixels,
            PixelBuffer::Rgba8(vec![0, 255, 0, 255, 64, 128, 192, 255])
        );

        prepare_for_encoding(
            &mut image,
            FormatTag::Png,
            &EncodeOptions {
                resize_long_edge: Some(4),
                ..options_for(FormatTag::Png)
            },
        )
        .expect("resize image");
        assert_eq!((image.width, image.height), (2, 4));
        assert_eq!(image.pixels.len(), 2 * 4 * 4);
    }

    #[test]
    fn invalid_crop_and_resize_are_rejected() {
        let mut image = fixture();
        assert!(prepare_for_encoding(
            &mut image,
            FormatTag::Png,
            &EncodeOptions {
                crop: Some(crate::CropRect {
                    x: 1,
                    y: 1,
                    width: 2,
                    height: 1,
                }),
                ..options_for(FormatTag::Png)
            }
        )
        .is_err());

        let mut image = fixture();
        assert!(prepare_for_encoding(
            &mut image,
            FormatTag::Png,
            &EncodeOptions {
                resize_long_edge: Some(0),
                ..options_for(FormatTag::Png)
            }
        )
        .is_err());
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
