//! Line break opportunities and grapheme clusters (markdown-spec.md §3, §8.6): the same in every
//! host, so these run natively and on wasm32 too.

use compyla_fonts::{graphemes, line_breaks};

#[test]
fn lines_break_after_spaces_and_hyphens() {
    assert_eq!(line_breaks("The quick brown"), [4, 10, 15]);
    // Spaces stay at the end of the line they follow.
    assert_eq!(line_breaks("a  b"), [3, 4]);
    // After a hyphen, as PowerPoint breaks `state-of-`, never before one.
    assert_eq!(line_breaks("state-of-the-art"), [6, 9, 13, 16]);
    assert_eq!(line_breaks(""), Vec::<usize>::new());
}

#[test]
fn ideographs_break_between_each_other_but_not_before_closing_marks() {
    // 日本語。です: no break before 。, which closes; a break after each other character.
    assert_eq!(line_breaks("日本語。です"), [3, 6, 12, 15, 18]);
}

#[test]
fn a_word_in_a_dictionary_script_has_no_break_inside() {
    // Thai needs a dictionary to break, which measurement doesn't use (§3).
    let text = "สวัสดีครับ";
    assert_eq!(line_breaks(text), [text.len()]);
}

#[test]
fn grapheme_clusters_keep_marks_emoji_and_flags_whole() {
    assert_eq!(graphemes("ab"), [1, 2]);
    // e and its combining acute; then x.
    assert_eq!(graphemes("e\u{301}x"), [3, 4]);
    // A thumbs-up with its skin tone, and a flag of two regional indicators.
    assert_eq!(graphemes("\u{1F44D}\u{1F3FD}"), [8]);
    assert_eq!(graphemes("\u{1F1EB}\u{1F1F7}"), [8]);
    assert_eq!(graphemes(""), Vec::<usize>::new());
}
