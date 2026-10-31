//! Node host for toolchain check 3 (napi-rs).

use napi_derive::napi;

#[napi]
pub fn summarize(source: String) -> String {
    spike_core::summarize(&source)
}
