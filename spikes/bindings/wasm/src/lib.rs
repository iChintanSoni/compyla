//! Browser/WASM host for toolchain check 3 (wasm-bindgen).

use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub fn summarize(source: &str) -> String {
    spike_core::summarize(source)
}
