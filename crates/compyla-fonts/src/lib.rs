//! Fonts for Compyla's compose stage (markdown-spec.md §3, C62; architecture.md D14): font faces
//! read safely from bytes, the bundled house stand-in Carlito, and text measurement. No I/O (D11):
//! font bytes come from the host's font resolver or from a template, so one build serves native,
//! WASM, napi-rs and PyO3, and measures alike in all of them (§13).
//!
//! Faces are parsed with `ttf-parser` and shaped with `rustybuzz`, the stack the pinned `typst`
//! uses. The Unicode properties measurement reads itself (marks, controls, spaces, East Asian Width,
//! default-ignorables) come from the Unicode Character Database 17.0.0 (`unicode/make_tables.py`),
//! the version `unicode-script` uses; bidi classes (`unicode-bidi`) and the shaper's canonical
//! combining classes (`unicode-ccc`) are Unicode 16.0, the latest those crates have. Line breaks and
//! grapheme clusters come from `icu_segmenter`, Unicode 18.0, which Typst breaks lines with.

mod chars;
mod face;
mod lines;
mod measure;
mod tables;

pub use face::{
    Face, MAX_CMAP_RECORDS, MAX_FACES, MAX_FONT_BYTES, MAX_TABLES, Style, Unreadable, carlito,
};
pub use lines::{SEGMENTER_UNICODE_VERSION, graphemes, line_breaks};
pub use measure::{Features, Font, Fonts, Ligatures, Measurer, Run, measure};

/// The families the engine bundles a stand-in for (§3): Carlito for Calibri and Calibri Light, and
/// Consolas's fixed advance. They always resolve.
pub const STAND_INS: [&str; 3] = ["Calibri", "Calibri Light", "Consolas"];

/// The Unicode version measurement uses.
pub const UNICODE_VERSION: (u8, u8, u8) = tables::UNICODE_VERSION;
