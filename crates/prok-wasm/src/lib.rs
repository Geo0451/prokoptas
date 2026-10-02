use js_sys::{Object, Reflect};
use prok_core::{DecodeOptions, EncodeOptions, FormatTag};
use serde::de::DeserializeOwned;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub fn version() -> String {
    prok_core::VERSION.to_owned()
}

#[wasm_bindgen(js_name = probeFormat)]
pub fn probe_format(input: &[u8]) -> Option<String> {
    prok_core::probe_format(input).map(|format| format.as_str().to_owned())
}

#[wasm_bindgen(js_name = losslessCapability)]
pub fn lossless_capability(format: &str) -> std::result::Result<String, JsValue> {
    let format = parse_format(format).map_err(|error| to_js_error(&error))?;
    let capability = prok_core::lossless_capability(format).map_err(|error| to_js_error(&error))?;
    Ok(match capability {
        prok_core::LosslessCapability::Always => "always",
        prok_core::LosslessCapability::Never => "never",
        prok_core::LosslessCapability::Configurable => "configurable",
    }
    .to_owned())
}

#[wasm_bindgen(js_name = convert)]
pub fn convert_image(
    input: &[u8],
    target: &str,
    decode_options: JsValue,
    encode_options: JsValue,
) -> std::result::Result<Vec<u8>, JsValue> {
    let target = parse_format(target).map_err(|error| to_js_error(&error))?;
    let decode_options = deserialize_options::<DecodeOptions>(decode_options)
        .map_err(|message| to_options_error(&message))?;
    let encode_options = deserialize_options::<EncodeOptions>(encode_options)
        .map_err(|message| to_options_error(&message))?;
    prok_core::convert(input, target, &decode_options, &encode_options)
        .map_err(|error| to_js_error(&error))
}

fn deserialize_options<T: DeserializeOwned + Default>(
    value: JsValue,
) -> std::result::Result<T, String> {
    if value.is_undefined() || value.is_null() {
        return Ok(T::default());
    }
    serde_wasm_bindgen::from_value(value).map_err(|error| error.to_string())
}

fn parse_format(format: &str) -> prok_core::Result<FormatTag> {
    match format.to_ascii_lowercase().as_str() {
        "png" => Ok(FormatTag::Png),
        "jpg" | "jpeg" => Ok(FormatTag::Jpeg),
        "tif" | "tiff" => Ok(FormatTag::Tiff),
        "bmp" => Ok(FormatTag::Bmp),
        "webp" => Ok(FormatTag::WebP),
        "heif" | "heic" => Ok(FormatTag::Heif),
        "avif" => Ok(FormatTag::Avif),
        "jxl" => Ok(FormatTag::Jxl),
        "raw" => Ok(FormatTag::Raw),
        _ => Err(prok_core::Error::InvalidOptions {
            message: format!("unknown image format: {format}"),
        }),
    }
}

fn to_options_error(message: &str) -> JsValue {
    to_js_error(&prok_core::Error::InvalidOptions {
        message: message.to_owned(),
    })
}

fn to_js_error(error: &prok_core::Error) -> JsValue {
    let value = Object::new();
    let _ = Reflect::set(
        &value,
        &JsValue::from_str("code"),
        &JsValue::from_str(error.code().as_str()),
    );
    let _ = Reflect::set(
        &value,
        &JsValue::from_str("message"),
        &JsValue::from_str(&error.to_string()),
    );
    value.into()
}

#[cfg(test)]
mod tests {
    use super::{parse_format, probe_format};
    use prok_core::FormatTag;

    #[test]
    fn probe_uses_core_registry() {
        assert_eq!(probe_format(b"\x89PNG\r\n\x1a\n"), Some("png".to_owned()));
        assert_eq!(probe_format(b"not an image"), None);
    }

    #[test]
    fn target_names_map_to_core_format_tags() {
        assert_eq!(parse_format("JPEG"), Ok(FormatTag::Jpeg));
        assert!(parse_format("unknown").is_err());
    }

    #[test]
    fn capability_query_reflects_real_encoder_support() {
        assert_eq!(
            prok_core::lossless_capability(FormatTag::Jpeg),
            Ok(prok_core::LosslessCapability::Never)
        );
        assert_eq!(
            prok_core::lossless_capability(FormatTag::Avif),
            Ok(prok_core::LosslessCapability::Never)
        );
        assert_eq!(
            prok_core::lossless_capability(FormatTag::Png),
            Ok(prok_core::LosslessCapability::Always)
        );
    }
}
