use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub fn version() -> String {
    prok_core::VERSION.to_owned()
}
