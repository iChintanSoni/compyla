//! Names (markdown-spec.md §9.6): sheet-name repairs, Excel table names and table headers.

use std::collections::{HashMap, HashSet};

use super::excerpt;
use super::grid::CellRef;
use crate::diagnostic::{Diagnostic, Sink};
use crate::validate::at;

/// Excel's sheet-name limit, in UTF-16 code units.
const MAX_SHEET_NAME: usize = 31;
const SHEET_NAME_FORBIDDEN: [char; 7] = [':', '\\', '/', '?', '*', '[', ']'];

/// Repairs a sheet name (§9.6), reporting each kind of repair once at `line`. `position` is the
/// sheet's 1-based place in the workbook; `earlier` holds the case-folded repaired names before it.
pub fn sheet_name(
    raw: &str,
    position: usize,
    earlier: &HashSet<String>,
    line: Option<usize>,
    out: &mut Sink,
) -> String {
    let original = raw.replace('\n', " ").trim().to_string();
    let mut name = original.replace(SHEET_NAME_FORBIDDEN, "-");
    let mut problems = Vec::new();
    if name != original {
        problems.push("contains characters Excel doesn't allow in sheet names (: \\ / ? * [ ])");
    }
    let trimmed = name.trim_matches('\'');
    if trimmed.len() != name.len() {
        problems.push("starts or ends with an apostrophe");
        name = trimmed.to_string();
    }
    if name.is_empty() {
        problems.push("is empty");
        name = format!("Sheet{position}");
    }
    if !problems.is_empty() {
        out.push(at(
            Diagnostic::new(
                "sheet_name_invalid",
                format!(
                    "Sheet name `{}` {}; it is renamed `{name}`.",
                    excerpt(&original),
                    problems.join(" and ")
                ),
            )
            .hint("Avoid : \\ / ? * [ ] and leading or trailing apostrophes in level-1 headings."),
            line,
        ));
    }

    if utf16_len(&name) > MAX_SHEET_NAME {
        let before = name.clone();
        // A truncated name must not end with an apostrophe either; it can't start with one here.
        name = truncate_utf16(&name, MAX_SHEET_NAME)
            .trim_end_matches('\'')
            .to_string();
        out.push(at(
            Diagnostic::new(
                "sheet_name_too_long",
                format!("Sheet name `{}` is longer than Excel's 31 characters; it is shortened to `{name}`.", excerpt(&before)),
            )
            .hint("Use a shorter level-1 heading."),
            line,
        ));
    }

    let taken = |candidate: &str| {
        let folded = fold(candidate);
        folded == "history" || earlier.contains(&folded)
    };
    if taken(&name) {
        let repaired = (2..)
            .map(|n| {
                let suffix = format!(" ({n})");
                let base = truncate_utf16(&name, MAX_SHEET_NAME - utf16_len(&suffix));
                format!("{base}{suffix}")
            })
            .find(|candidate| !taken(candidate))
            .expect("an unused suffix exists");
        let reason = if fold(&name) == "history" {
            "is reserved by Excel".to_string()
        } else {
            "is already used by an earlier sheet (names are compared case-insensitively)"
                .to_string()
        };
        out.push(at(
            Diagnostic::new(
                "duplicate_sheet_name",
                format!("Sheet name `{name}` {reason}; it is renamed `{repaired}`."),
            )
            .hint("Give every level-1 heading a distinct title."),
            line,
        ));
        name = repaired;
    }
    name
}

/// A valid Excel table name: `[A-Za-z_\\][A-Za-z0-9_.]*`, at most 255 characters, not a cell
/// reference, and not `R`, `C` or an R1C1 reference.
pub fn valid_table_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == '_' || first == '\\')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
        && name.len() <= 255
        && CellRef::parse(name).is_none()
        && !is_r1c1(name)
}

/// `R`, `C`, `R[0-9]*C[0-9]*`, `R[0-9]+` or `C[0-9]+`, in any case.
fn is_r1c1(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    let digits = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
    if let Some(rest) = upper.strip_prefix('R') {
        digits(rest)
            || rest
                .split_once('C')
                .is_some_and(|(r, c)| digits(r) && digits(c))
    } else {
        upper.strip_prefix('C').is_some_and(digits)
    }
}

/// Excel table headers must be non-empty and unique (case-insensitive): an empty header becomes
/// `Column{n}`, and a repeated one gets `2`, `3`, … appended (the smallest suffix not yet used).
pub fn repair_headers(headers: &[String]) -> Vec<String> {
    let mut used = HashSet::new();
    // Per repeated name, the next suffix to try, so a run of repeats isn't quadratic.
    let mut next: HashMap<String, usize> = HashMap::new();
    let mut repaired = Vec::with_capacity(headers.len());
    for (index, header) in headers.iter().enumerate() {
        let mut name = if header.is_empty() {
            format!("Column{}", index + 1)
        } else {
            header.clone()
        };
        let folded = fold(&name);
        if used.contains(&folded) {
            let k = next.entry(folded).or_insert(2);
            while used.contains(&fold(&format!("{name}{k}"))) {
                *k += 1;
            }
            name = format!("{name}{k}");
            *k += 1;
        }
        used.insert(fold(&name));
        repaired.push(name);
    }
    repaired
}

/// Unicode simple case folding, for comparing names case-insensitively. Per character: the
/// single-character lowercase mapping, plus the folds that differ from it (CaseFolding.txt, status
/// C and S).
pub fn fold(text: &str) -> String {
    text.chars().map(fold_char).collect()
}

fn fold_char(c: char) -> char {
    match c {
        'ς' => 'σ',
        'ϐ' => 'β',
        'ϑ' => 'θ',
        'ϕ' => 'φ',
        'ϖ' => 'π',
        'ϰ' => 'κ',
        'ϱ' => 'ρ',
        'ϵ' => 'ε',
        'ſ' => 's',
        'µ' => 'μ',
        '\u{345}' | '\u{1FBE}' => 'ι',
        'ẛ' => 'ṡ',
        // Cyrillic variant letters, which are their own lowercase.
        '\u{1C80}' => 'в',
        '\u{1C81}' => 'д',
        '\u{1C82}' => 'о',
        '\u{1C83}' => 'с',
        '\u{1C84}' | '\u{1C85}' => 'т',
        '\u{1C86}' => 'ъ',
        '\u{1C87}' => 'ѣ',
        '\u{1C88}' => 'ꙋ',
        // Status S since Unicode 15.1.
        '\u{1FD3}' => '\u{390}',
        '\u{1FE3}' => '\u{3B0}',
        '\u{FB05}' => '\u{FB06}',
        // No simple folding: İ (only a full one), and uppercase Cherokee, which lowercase Cherokee
        // folds to.
        '\u{130}' | '\u{13A0}'..='\u{13F5}' => c,
        '\u{13F8}'..='\u{13FD}' => char::from_u32(c as u32 - 8).unwrap_or(c),
        '\u{AB70}'..='\u{ABBF}' => char::from_u32(c as u32 - 0xAB70 + 0x13A0).unwrap_or(c),
        _ => {
            let mut lower = c.to_lowercase();
            match (lower.next(), lower.next()) {
                (Some(l), None) => l,
                _ => c,
            }
        }
    }
}

pub fn utf16_len(text: &str) -> usize {
    text.chars().map(char::len_utf16).sum()
}

/// The longest prefix of at most `max` UTF-16 code units, never splitting a character.
fn truncate_utf16(text: &str, max: usize) -> &str {
    let mut units = 0;
    for (index, c) in text.char_indices() {
        units += c.len_utf16();
        if units > max {
            return &text[..index];
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repair(raw: &str, earlier: &[&str]) -> (String, Vec<String>) {
        let taken: HashSet<String> = earlier.iter().map(|s| fold(s)).collect();
        let mut out = Sink::new();
        let name = sheet_name(raw, earlier.len() + 1, &taken, Some(1), &mut out);
        (name, out.iter().map(|d| d.code.clone()).collect())
    }

    #[test]
    fn sheet_name_repairs() {
        assert_eq!(repair("Sales", &[]), ("Sales".into(), vec![]));
        assert_eq!(
            repair("Q1/Q2", &[]),
            ("Q1-Q2".into(), vec!["sheet_name_invalid".into()])
        );
        assert_eq!(
            repair("'Quoted' [x]", &[]),
            ("Quoted' -x-".into(), vec!["sheet_name_invalid".into()])
        );
        assert_eq!(
            repair("  ", &["A"]),
            ("Sheet2".into(), vec!["sheet_name_invalid".into()])
        );
        assert_eq!(repair("Line\nbreak", &[]).0, "Line break");
        assert_eq!(
            repair("A sheet name that is far too long for Excel", &[]),
            (
                "A sheet name that is far too lo".into(),
                vec!["sheet_name_too_long".into()]
            )
        );
        assert_eq!(
            repair("data", &["Data"]),
            ("data (2)".into(), vec!["duplicate_sheet_name".into()])
        );
        assert_eq!(repair("Data", &["Data", "Data (2)"]).0, "Data (3)");
        assert_eq!(
            repair("HISTORY", &[]),
            ("HISTORY (2)".into(), vec!["duplicate_sheet_name".into()])
        );
        let long = "abcdefghijklmnopqrstuvwxyz01234";
        assert_eq!(repair(long, &[long]).0, "abcdefghijklmnopqrstuvwxyz0 (2)");
    }

    #[test]
    fn truncation_keeps_surrogate_pairs_whole() {
        let name = format!("{}😀tail", "a".repeat(30));
        let (repaired, codes) = repair(&name, &[]);
        assert_eq!(repaired, "a".repeat(30));
        assert_eq!(codes, vec!["sheet_name_too_long"]);
        let (repaired, _) = repair(&format!("{}'xyz", "a".repeat(30)), &[]);
        assert_eq!(
            repaired,
            "a".repeat(30),
            "no trailing apostrophe after truncation"
        );
    }

    #[test]
    fn case_folding() {
        assert_eq!(fold("ΣΑΣ"), fold("σας"));
        assert_eq!(fold("Straße"), "straße");
        assert_eq!(fold("ẞ"), "ß");
        assert_eq!(fold("İ"), "İ");
        assert_eq!(fold("ꭰ"), "Ꭰ");
        assert_eq!(fold("\u{1C80}"), fold("В"));
        assert_eq!(fold("\u{1C88}"), fold("Ꙋ"));
        assert_eq!(fold("\u{FB05}"), fold("\u{FB06}"));
        assert_eq!(fold("Ꭰ"), "Ꭰ");
        assert_eq!(fold("K"), "k");
    }

    #[test]
    fn table_names() {
        for valid in ["sales_q3", "_x", "\\x", "Table1.a", "RCC", "R1C1x", "XFE1"] {
            assert!(valid_table_name(valid), "{valid}");
        }
        for invalid in [
            "", "A1", "a1", "XFD1", "R", "c", "R1", "C12", "RC", "R1C", "RC1", "r1c1", "1x", "x y",
            "x-y", ".x",
        ] {
            assert!(!valid_table_name(invalid), "{invalid}");
        }
        assert!(!valid_table_name(&"a".repeat(256)));
        assert!(valid_table_name(&"a".repeat(255)));
    }

    #[test]
    fn header_repairs() {
        let headers =
            |names: &[&str]| -> Vec<String> { names.iter().map(|s| s.to_string()).collect() };
        assert_eq!(repair_headers(&headers(&["A", "B"])), headers(&["A", "B"]));
        assert_eq!(
            repair_headers(&headers(&["Item", "item", "ITEM"])),
            headers(&["Item", "item2", "ITEM3"])
        );
        assert_eq!(
            repair_headers(&headers(&["", "Column1", "x"])),
            headers(&["Column1", "Column12", "x"])
        );
        assert_eq!(
            repair_headers(&headers(&["x", "x2", "x", "x"])),
            headers(&["x", "x2", "x3", "x4"])
        );
        // Many repeats stay linear.
        let many = vec!["x".to_string(); 20_000];
        assert_eq!(repair_headers(&many)[19_999], "x20000");
    }
}
