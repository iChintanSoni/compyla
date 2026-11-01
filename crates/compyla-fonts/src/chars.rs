//! Character properties measurement uses (markdown-spec.md §3, C62): a character's class, whether
//! it is a mark, whether it advances nothing when no face maps it, and its resolved script.

use unicode_script::{Script, UnicodeScript};

use crate::tables;

/// Which of a run's fonts measures a character (§3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Class {
    Latin,
    EastAsian,
    Complex,
}

/// The scripts whose characters are complex (§3).
const COMPLEX: [Script; 20] = [
    Script::Arabic,
    Script::Hebrew,
    Script::Syriac,
    Script::Thaana,
    Script::Nko,
    Script::Devanagari,
    Script::Bengali,
    Script::Gurmukhi,
    Script::Gujarati,
    Script::Oriya,
    Script::Tamil,
    Script::Telugu,
    Script::Kannada,
    Script::Malayalam,
    Script::Sinhala,
    Script::Thai,
    Script::Lao,
    Script::Tibetan,
    Script::Myanmar,
    Script::Khmer,
];

fn within(table: &[(u32, u32)], c: char) -> bool {
    let c = c as u32;
    table
        .binary_search_by(|&(first, last)| {
            if last < c {
                std::cmp::Ordering::Less
            } else if first > c {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .is_ok()
}

/// East Asian Width Wide or Fullwidth.
pub fn is_wide(c: char) -> bool {
    within(tables::WIDE, c)
}

/// A space separator (Zs).
pub fn is_space(c: char) -> bool {
    within(tables::SPACE, c)
}

/// A mark: a nonspacing or enclosing mark (Mn, Me), or an Inherited character.
pub fn is_mark(c: char) -> bool {
    within(tables::MARK, c) || c.script() == Script::Inherited
}

/// A character that advances nothing when no face maps it: default-ignorable, Mn, Me or Cc.
pub fn advances_nothing(c: char) -> bool {
    within(tables::DEFAULT_IGNORABLE, c) || within(tables::MARK, c) || within(tables::CONTROL, c)
}

/// A character's own class, from its Script and East Asian Width.
fn own_class(c: char) -> Class {
    let script = c.script();
    if matches!(
        script,
        Script::Han | Script::Hiragana | Script::Katakana | Script::Hangul | Script::Bopomofo
    ) || is_wide(c)
    {
        Class::EastAsian
    } else if COMPLEX.contains(&script) {
        Class::Complex
    } else {
        Class::Latin
    }
}

/// Each character's class: its own, except that a mark takes its base's, the nearest character
/// before it that isn't a mark, else the first after it, else latin.
pub fn classes(chars: &[char]) -> Vec<Class> {
    let own: Vec<Option<Class>> = chars
        .iter()
        .map(|&c| (!is_mark(c)).then(|| own_class(c)))
        .collect();
    let first = own.iter().flatten().next().copied().unwrap_or(Class::Latin);
    let mut base = first;
    own.into_iter()
        .map(|class| {
            if let Some(class) = class {
                base = class;
            }
            base
        })
        .collect()
}

/// The index of each character's base: itself, or for a mark the nearest character before it that
/// isn't a mark, else the first after it, else itself.
pub fn bases(chars: &[char]) -> Vec<usize> {
    let first = chars.iter().position(|&c| !is_mark(c));
    let mut base = None;
    (0..chars.len())
        .map(|i| {
            if !is_mark(chars[i]) {
                base = Some(i);
            }
            base.or(first).unwrap_or(i)
        })
        .collect()
}

/// Each character's resolved script: its own, or for a Common or Inherited character the nearest
/// before it that has one, else after it, else Common.
pub fn scripts(chars: &[char]) -> Vec<Script> {
    let real = |s: Script| !matches!(s, Script::Common | Script::Inherited | Script::Unknown);
    let own: Vec<Script> = chars.iter().map(|c| c.script()).collect();
    let first = own
        .iter()
        .copied()
        .find(|&s| real(s))
        .unwrap_or(Script::Common);
    let mut current = first;
    own.into_iter()
        .map(|s| {
            if real(s) {
                current = s;
            }
            current
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classes_follow_script_and_width_and_marks_follow_their_base() {
        let text: Vec<char> = "A漢\u{3042}\u{0644}1\u{0301}\u{FF21}".chars().collect();
        assert_eq!(
            classes(&text),
            [
                Class::Latin,
                Class::EastAsian,
                Class::EastAsian,
                Class::Complex,
                Class::Latin,
                Class::Latin, // the mark takes its base's class
                Class::EastAsian,
            ]
        );
        // A mark at the start takes the class of the first character after it.
        let text: Vec<char> = "\u{0301}\u{0644}".chars().collect();
        assert_eq!(classes(&text), [Class::Complex, Class::Complex]);
        assert_eq!(bases(&text), [1, 1]);
    }

    #[test]
    fn common_characters_take_a_neighbors_script() {
        let text: Vec<char> = "1 \u{0627}b".chars().collect();
        assert_eq!(
            scripts(&text),
            [
                Script::Arabic,
                Script::Arabic,
                Script::Arabic,
                Script::Latin
            ]
        );
    }

    #[test]
    fn the_zero_advance_set() {
        for c in [
            '\u{00AD}', '\u{200B}', '\u{0301}', '\u{20DD}', '\t', '\u{FE0F}',
        ] {
            assert!(advances_nothing(c), "{c:?}");
        }
        for c in ['a', ' ', '\u{3000}', '\u{4E00}'] {
            assert!(!advances_nothing(c), "{c:?}");
        }
    }
}
