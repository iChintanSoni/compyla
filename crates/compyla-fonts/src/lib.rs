//! Fonts for Compyla's compose stage (markdown-spec.md §3, C62; architecture.md D14): font faces
//! read safely from bytes, the bundled house stand-in Carlito, and text measurement. No I/O (D11):
//! font bytes come from the host's font resolver or from a template, so one build serves native,
//! WASM, napi-rs and PyO3, and measures alike in all of them (§13).
//!
//! Faces are parsed with `ttf-parser` and shaped with `rustybuzz`, the stack the pinned `typst`
//! uses. The Unicode properties measurement needs come from the Unicode Character Database 17.0.0
//! (`unicode/make_tables.py`), the version `unicode-script` uses.

mod chars;
mod face;
mod measure;
mod tables;

pub use face::{Face, MAX_FONT_BYTES, Style, Unreadable, carlito};
pub use measure::{Features, Font, Fonts, Ligatures, Run, measure};

/// The Unicode version measurement uses.
pub const UNICODE_VERSION: (u8, u8, u8) = tables::UNICODE_VERSION;
