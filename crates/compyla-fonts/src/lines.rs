//! Where a line may break, and where a word may break inside (markdown-spec.md §3, §8.6): UAX #14
//! opportunities and UAX #29 extended grapheme clusters, both from `icu_segmenter`, which the
//! pinned Typst breaks lines with (architecture.md D14). Filling lines is the compose stage's.

use icu_segmenter::options::LineBreakOptions;
use icu_segmenter::{GraphemeClusterSegmenter, LineSegmenter};

/// The Unicode version of `icu_segmenter`'s data (its data crate's README).
pub const SEGMENTER_UNICODE_VERSION: (u8, u8, u8) = (18, 0, 0);

/// The byte offsets in `text` after which a line may break: UAX #14 opportunities, with the
/// segmenter's default options and its rules for scripts without a dictionary, so a word in Thai,
/// Lao, Khmer or Myanmar has none inside it. In order, the end of the text last; the start isn't
/// one. Hard breaks are the caller's: it splits the text at them first.
pub fn line_breaks(text: &str) -> Vec<usize> {
    LineSegmenter::new_for_non_complex_scripts(LineBreakOptions::default())
        .segment_str(text)
        .filter(|&offset| offset > 0)
        .collect()
}

/// The byte offsets in `text` where its extended grapheme clusters end, in order, the end of the
/// text last: where a word wider than a line may break (§8.6).
pub fn graphemes(text: &str) -> Vec<usize> {
    GraphemeClusterSegmenter::new()
        .segment_str(text)
        .filter(|&offset| offset > 0)
        .collect()
}
