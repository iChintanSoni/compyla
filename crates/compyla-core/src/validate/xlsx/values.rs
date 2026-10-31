//! Values (markdown-spec.md §9.5): the literal tests, column types, inference and conversion.

/// A column's type (§9.4), with its number-format parameter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ColumnType {
    /// `General`, or `#,##0` with this many decimals.
    Number {
        decimals: Option<u8>,
    },
    /// An ISO 4217 code, uppercase.
    Currency {
        code: String,
    },
    /// `0%` with this many decimals.
    Percent {
        decimals: u8,
    },
    Date,
    Text,
    Boolean,
}

impl ColumnType {
    /// The type's name, as `columns` entries and the compose summary spell it.
    pub fn name(&self) -> &'static str {
        match self {
            ColumnType::Number { .. } => "number",
            ColumnType::Currency { .. } => "currency",
            ColumnType::Percent { .. } => "percent",
            ColumnType::Date => "date",
            ColumnType::Text => "text",
            ColumnType::Boolean => "boolean",
        }
    }

    /// Conversion: whether a literal converts with this type's test. Numeric types accept numbers,
    /// and percent also accepts percents.
    pub fn accepts(&self, literal: Literal) -> bool {
        match self {
            ColumnType::Number { .. } | ColumnType::Currency { .. } => literal == Literal::Number,
            ColumnType::Percent { .. } => matches!(literal, Literal::Number | Literal::Percent),
            ColumnType::Date => literal == Literal::Date,
            ColumnType::Boolean => literal == Literal::Boolean,
            ColumnType::Text => true,
        }
    }
}

/// Which literal test a cell's plain text passes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Literal {
    Number,
    Percent,
    Date,
    Boolean,
    Text,
}

pub fn classify(text: &str) -> Literal {
    if is_number(text) {
        Literal::Number
    } else if text.strip_suffix('%').is_some_and(is_number) {
        Literal::Percent
    } else if date_serial(text).is_some() {
        Literal::Date
    } else if text.eq_ignore_ascii_case("true") || text.eq_ignore_ascii_case("false") {
        Literal::Boolean
    } else {
        Literal::Text
    }
}

/// `-?[0-9]+(\.[0-9]+)?` with at most 15 significant digits and no leading zero before another digit.
/// Significant digits run from the first non-zero digit to the last digit, so trailing zeros count:
/// a 16-digit ID ending in zeros stays text.
pub fn is_number(text: &str) -> bool {
    let unsigned = text.strip_prefix('-').unwrap_or(text);
    let (integer, fraction) = match unsigned.split_once('.') {
        Some((integer, fraction)) => (integer, fraction),
        None => (unsigned, ""),
    };
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    if !digits(integer) || (unsigned.contains('.') && !digits(fraction)) {
        return false;
    }
    if integer.len() > 1 && integer.starts_with('0') {
        return false;
    }
    let significant = integer
        .bytes()
        .chain(fraction.bytes())
        .skip_while(|b| *b == b'0')
        .count();
    significant <= 15
}

/// `[0-9]{4}-[0-9]{2}-[0-9]{2}`, a real Gregorian date from 1900-03-01 to 9999-12-31, as its serial
/// in the 1900 date system (1900-03-01 is 61).
pub fn date_serial(text: &str) -> Option<i64> {
    let bytes = text.as_bytes();
    let shape = bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(i, b)| i == 4 || i == 7 || b.is_ascii_digit());
    if !shape {
        return None;
    }
    let number = |range: std::ops::Range<usize>| text[range].parse::<i64>().ok();
    let (year, month, day) = (number(0..4)?, number(5..7)?, number(8..10)?);
    if !(1..=12).contains(&month) || day < 1 || day > days_in_month(year, month) {
        return None;
    }
    if (year, month, day) < (1900, 3, 1) {
        return None;
    }
    Some(days_from_civil(year, month, day) - days_from_civil(1899, 12, 30))
}

fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Days since 1970-01-01 of a proleptic Gregorian date (H. Hinnant's algorithm).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// Inference over a column's literal cells (formula and blank cells already skipped). Numbers and
/// percents are one family. When the cells fall into more than one kind, the column is text and the
/// kinds found are returned for `mixed_column_type`.
pub fn infer(literals: &[Literal]) -> (ColumnType, Option<String>) {
    let kind = |literal: Literal| match literal {
        Literal::Number | Literal::Percent => "numbers",
        Literal::Date => "dates",
        Literal::Boolean => "booleans",
        Literal::Text => "text",
    };
    let mut kinds: Vec<&str> = Vec::new();
    for literal in literals {
        if !kinds.contains(&kind(*literal)) {
            kinds.push(kind(*literal));
        }
    }
    match kinds.as_slice() {
        [] | ["text"] => (ColumnType::Text, None),
        ["numbers"] if literals.contains(&Literal::Percent) => {
            (ColumnType::Percent { decimals: 0 }, None)
        }
        ["numbers"] => (ColumnType::Number { decimals: None }, None),
        ["dates"] => (ColumnType::Date, None),
        ["booleans"] => (ColumnType::Boolean, None),
        _ => (ColumnType::Text, Some(kinds.join(" and "))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers() {
        for number in [
            "0",
            "-0",
            "12",
            "-12",
            "0.5",
            "12.50",
            "120000",
            "123456789012345",
            "0.000000000000000001",
        ] {
            assert!(is_number(number), "{number}");
        }
        for text in [
            "",
            "-",
            "007",
            "02134",
            "00.5",
            "-01",
            ".5",
            "5.",
            "1e5",
            "+1",
            "1,000",
            "$120,000",
            " 12",
            "1.2.3",
            "1234567890123456",
            "12345678901234567890",
            "1000000000000000",
            "١٢",
        ] {
            assert!(!is_number(text), "{text}");
        }
    }

    #[test]
    fn literal_tests() {
        assert_eq!(classify("14%"), Literal::Percent);
        assert_eq!(classify("9.5%"), Literal::Percent);
        assert_eq!(classify("-5%"), Literal::Percent);
        assert_eq!(classify("007%"), Literal::Text);
        assert_eq!(classify("%"), Literal::Text);
        assert_eq!(classify("2026-03-01"), Literal::Date);
        assert_eq!(classify("2026-02-30"), Literal::Text);
        assert_eq!(classify("TRUE"), Literal::Boolean);
        assert_eq!(classify("False"), Literal::Boolean);
        assert_eq!(classify("yes"), Literal::Text);
        assert_eq!(classify("=SUM(A1)"), Literal::Text);
    }

    #[test]
    fn dates() {
        assert_eq!(date_serial("1900-03-01"), Some(61));
        assert_eq!(date_serial("2026-01-01"), Some(46023));
        assert_eq!(date_serial("2024-02-29"), Some(45351));
        assert_eq!(date_serial("9999-12-31"), Some(2_958_465));
        for text in [
            "1900-02-28",
            "1899-12-31",
            "2023-02-29",
            "1900-02-29",
            "2026-13-01",
            "2026-00-10",
            "2026-04-31",
            "2026-1-01",
            "2026/01/01",
            "20260101",
            "2026-01-01T00:00",
        ] {
            assert_eq!(date_serial(text), None, "{text}");
        }
    }

    #[test]
    fn inference() {
        use Literal::*;
        assert_eq!(infer(&[]), (ColumnType::Text, None));
        assert_eq!(
            infer(&[Number, Number]),
            (ColumnType::Number { decimals: None }, None)
        );
        assert_eq!(
            infer(&[Number, Percent]),
            (ColumnType::Percent { decimals: 0 }, None)
        );
        assert_eq!(infer(&[Date, Date]), (ColumnType::Date, None));
        assert_eq!(infer(&[Boolean]), (ColumnType::Boolean, None));
        assert_eq!(infer(&[Text, Text]), (ColumnType::Text, None));
        assert_eq!(
            infer(&[Number, Text]),
            (ColumnType::Text, Some("numbers and text".into()))
        );
        assert_eq!(
            infer(&[Date, Number, Date]),
            (ColumnType::Text, Some("dates and numbers".into()))
        );
    }

    #[test]
    fn conversion() {
        let currency = ColumnType::Currency { code: "USD".into() };
        assert!(currency.accepts(Literal::Number));
        assert!(!currency.accepts(Literal::Percent));
        assert!(!currency.accepts(Literal::Text));
        let percent = ColumnType::Percent { decimals: 0 };
        assert!(percent.accepts(Literal::Number) && percent.accepts(Literal::Percent));
        assert!(!ColumnType::Number { decimals: None }.accepts(Literal::Percent));
        assert!(ColumnType::Date.accepts(Literal::Date));
        assert!(!ColumnType::Date.accepts(Literal::Text));
        assert!(ColumnType::Text.accepts(Literal::Boolean));
    }
}
