//! A1 addressing (markdown-spec.md §9.2, §9.3): column letters, cell references and Excel's grid.

use std::fmt;

/// The last row of a sheet.
pub const MAX_ROW: u32 = 1_048_576;
/// The number of columns, A to XFD.
pub const MAX_COLUMNS: u32 = 16_384;

/// A cell reference, `["$"] col ["$"] row`, within A1:XFD1048576.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellRef {
    /// 0-based: A is 0.
    pub column: u32,
    /// 1-based.
    pub row: u32,
    pub column_absolute: bool,
    pub row_absolute: bool,
}

impl CellRef {
    /// Parses a whole cell reference, in any case. Text outside the grid (`XFE1`, `A0`) is not one.
    pub fn parse(text: &str) -> Option<CellRef> {
        let column_absolute = text.starts_with('$');
        let rest = if column_absolute { &text[1..] } else { text };
        let letters = rest.bytes().take_while(u8::is_ascii_alphabetic).count();
        let column = column_index(&rest[..letters])?;
        let rest = &rest[letters..];
        let row_absolute = rest.starts_with('$');
        let digits = if row_absolute { &rest[1..] } else { rest };
        if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let row: u32 = digits.parse().ok()?;
        (1..=MAX_ROW).contains(&row).then_some(CellRef {
            column,
            row,
            column_absolute,
            row_absolute,
        })
    }
}

/// The uppercase written form, `$` markers kept.
impl fmt::Display for CellRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let column = if self.column_absolute { "$" } else { "" };
        let row = if self.row_absolute { "$" } else { "" };
        write!(
            f,
            "{column}{}{row}{}",
            column_letters(self.column),
            self.row
        )
    }
}

/// The letters of a 0-based column index: 0 → A, 25 → Z, 26 → AA.
pub fn column_letters(index: u32) -> String {
    let mut n = index + 1;
    let mut letters = Vec::new();
    while n > 0 {
        letters.push(b'A' + ((n - 1) % 26) as u8);
        n = (n - 1) / 26;
    }
    letters.reverse();
    String::from_utf8(letters).expect("ASCII letters")
}

/// The 0-based index of 1–3 column letters in any case, if within A..XFD.
pub fn column_index(letters: &str) -> Option<u32> {
    if letters.is_empty() || letters.len() > 3 || !letters.bytes().all(|b| b.is_ascii_alphabetic())
    {
        return None;
    }
    let n = letters.bytes().fold(0, |n, b| {
        n * 26 + u32::from(b.to_ascii_uppercase() - b'A' + 1)
    });
    (n <= MAX_COLUMNS).then(|| n - 1)
}

/// A placed address such as `B4`.
pub fn address(column: u32, row: u32) -> String {
    format!("{}{row}", column_letters(column))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn column_letters_round_trip() {
        for (index, letters) in [(0, "A"), (25, "Z"), (26, "AA"), (701, "ZZ"), (702, "AAA")] {
            assert_eq!(column_letters(index), letters);
            assert_eq!(column_index(letters), Some(index));
        }
        assert_eq!(column_letters(MAX_COLUMNS - 1), "XFD");
        assert_eq!(column_index("xfd"), Some(MAX_COLUMNS - 1));
        assert_eq!(column_index("XFE"), None);
        assert_eq!(column_index("ABCD"), None);
        assert_eq!(column_index(""), None);
    }

    #[test]
    fn cell_references() {
        let cell = CellRef::parse("$b$2").unwrap();
        assert_eq!((cell.column, cell.row), (1, 2));
        assert!(cell.column_absolute && cell.row_absolute);
        assert_eq!(cell.to_string(), "$B$2");
        assert_eq!(CellRef::parse("c5").unwrap().to_string(), "C5");
        assert_eq!(CellRef::parse("A$1").unwrap().to_string(), "A$1");
        assert_eq!(CellRef::parse("LOG10").unwrap().to_string(), "LOG10");
        assert_eq!(CellRef::parse("XFD1048576").unwrap().row, MAX_ROW);
        for invalid in [
            "", "A", "1", "A0", "XFE1", "A1048577", "AB$", "$$A1", "A1B", "A 1", "A-1", "Z",
        ] {
            assert_eq!(CellRef::parse(invalid), None, "{invalid}");
        }
    }
}
