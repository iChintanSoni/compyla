//! Values (markdown-spec.md §9.5): the literal tests, column types, inference and conversion.

/// A column's type (§9.4), with its number-format parameter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ColumnType {
    /// `General`, or `#,##0` with this many decimals.
    Number {
        decimals: Option<u8>,
    },
    /// A three-letter currency code, uppercase.
    Currency {
        code: String,
    },
    /// `0%` with this many decimals. `None` until the column's cells are known: then just enough to
    /// show every value as written, at most 10.
    Percent {
        decimals: Option<u8>,
    },
    Date,
    Text,
    Boolean,
    /// Inferred only: each literal keeps the type of the test it matches, and formula cells use
    /// `General`.
    General,
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
            ColumnType::General => "general",
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
            ColumnType::Text | ColumnType::General => true,
        }
    }
}

impl ColumnType {
    /// The number format code (§9.4). A `general` column has no one format: each cell takes its
    /// literal's ([`literal_format`]), and formula cells `General`.
    pub fn number_format(&self) -> String {
        match self {
            ColumnType::Number { decimals: None } | ColumnType::Boolean | ColumnType::General => {
                "General".into()
            }
            ColumnType::Number {
                decimals: Some(decimals),
            } => format!("#,##0{}", fraction(*decimals)),
            ColumnType::Currency { code } => {
                let symbol = match code.as_str() {
                    "USD" => "$".to_string(),
                    "EUR" => "€".into(),
                    "GBP" => "£".into(),
                    "JPY" => "¥".into(),
                    "INR" => "₹".into(),
                    other => format!("{other} "),
                };
                let decimals = if code == "JPY" { 0 } else { 2 };
                format!("\"{symbol}\"#,##0{}", fraction(decimals))
            }
            ColumnType::Percent { decimals } => percent_format(decimals.unwrap_or(0)),
            ColumnType::Date => "yyyy-mm-dd".into(),
            ColumnType::Text => "@".into(),
        }
    }
}

fn fraction(decimals: u8) -> String {
    if decimals == 0 {
        String::new()
    } else {
        format!(".{}", "0".repeat(usize::from(decimals)))
    }
}

fn percent_format(decimals: u8) -> String {
    format!("0{}%", fraction(decimals))
}

/// The id of a built-in number format, which a workbook writes without a `numFmt` record
/// (ECMA-376 Part 1, §18.8.30); every other format is custom.
pub fn builtin_format(code: &str) -> Option<u32> {
    Some(match code {
        "General" => 0,
        "0" => 1,
        "0.00" => 2,
        "#,##0" => 3,
        "#,##0.00" => 4,
        "0%" => 9,
        "0.00%" => 10,
        "@" => 49,
        _ => return None,
    })
}

/// The format a literal takes in a `general` column (§9.5): its own type's default.
pub fn literal_format(text: &str, literal: Literal) -> String {
    match literal {
        Literal::Percent => percent_format(percent_decimals(text, literal)),
        Literal::Date => "yyyy-mm-dd".into(),
        Literal::Number | Literal::Boolean | Literal::Text => "General".into(),
    }
}

/// A converted literal (§9.5): what a cell holds when its text passed a test its column accepts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Converted {
    /// A number, as an exact decimal (a percent already divided by 100, a date as its serial).
    Number(String),
    Boolean(bool),
}

/// Converts `text`, which passed `literal`'s test (`None` for text).
pub fn convert(text: &str, literal: Literal) -> Option<Converted> {
    match literal {
        Literal::Number => Some(Converted::Number(text.to_string())),
        Literal::Percent => Some(Converted::Number(percent_value(text))),
        Literal::Date => date_serial(text).map(|serial| Converted::Number(serial.to_string())),
        Literal::Boolean => Some(Converted::Boolean(text.eq_ignore_ascii_case("true"))),
        Literal::Text => None,
    }
}

/// `14%` → `0.14`, exactly: the decimal point moves two places left on the digits as written.
fn percent_value(text: &str) -> String {
    let number = text.trim_end_matches('%');
    let (negative, unsigned) = match number.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, number),
    };
    let (integer, fraction) = unsigned.split_once('.').unwrap_or((unsigned, ""));
    let padded = format!("{integer:0>3}");
    let (whole, moved) = padded.split_at(padded.len() - 2);
    let whole = whole.trim_start_matches('0');
    let digits = format!("{moved}{fraction}");
    let digits = digits.trim_end_matches('0');
    let mut out = String::new();
    if negative && !(whole.is_empty() && digits.is_empty()) {
        out.push('-');
    }
    out.push_str(if whole.is_empty() { "0" } else { whole });
    if !digits.is_empty() {
        out.push('.');
        out.push_str(digits);
    }
    out
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

/// The decimals a percent format needs to show this literal as written: a percent's own, and two
/// fewer for a number (0.095 is 9.5%), at most 10.
pub fn percent_decimals(text: &str, literal: Literal) -> u8 {
    let text = match literal {
        Literal::Percent => text.trim_end_matches('%'),
        Literal::Number => text,
        _ => return 0,
    };
    let fraction = text.split_once('.').map_or(0, |(_, f)| f.len());
    let decimals = if literal == Literal::Number {
        fraction.saturating_sub(2)
    } else {
        fraction
    };
    decimals.min(10) as u8
}

/// Inference over a column's literal cells (formula and blank cells already skipped). When the
/// cells match more than one test, plain text counting as one, the column is `general` and the
/// kinds found are returned for `mixed_column_type`.
pub fn infer(literals: &[Literal]) -> (ColumnType, Option<String>) {
    let kind = |literal: Literal| match literal {
        Literal::Number => "numbers",
        Literal::Percent => "percents",
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
        [] => (ColumnType::General, None),
        ["text"] => (ColumnType::Text, None),
        ["numbers"] => (ColumnType::Number { decimals: None }, None),
        ["percents"] => (ColumnType::Percent { decimals: None }, None),
        ["dates"] => (ColumnType::Date, None),
        ["booleans"] => (ColumnType::Boolean, None),
        _ => (ColumnType::General, Some(kinds.join(" and "))),
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
        assert_eq!(infer(&[]), (ColumnType::General, None));
        assert_eq!(
            infer(&[Number, Number]),
            (ColumnType::Number { decimals: None }, None)
        );
        assert_eq!(
            infer(&[Percent, Percent]),
            (ColumnType::Percent { decimals: None }, None)
        );
        assert_eq!(
            infer(&[Number, Number, Percent]),
            (ColumnType::General, Some("numbers and percents".into()))
        );
        assert_eq!(infer(&[Date, Date]), (ColumnType::Date, None));
        assert_eq!(infer(&[Boolean]), (ColumnType::Boolean, None));
        assert_eq!(infer(&[Text, Text]), (ColumnType::Text, None));
        assert_eq!(
            infer(&[Number, Text]),
            (ColumnType::General, Some("numbers and text".into()))
        );
        assert_eq!(
            infer(&[Date, Number, Date]),
            (ColumnType::General, Some("dates and numbers".into()))
        );
    }

    #[test]
    fn percent_values_are_exact() {
        for (text, value) in [
            ("14%", "0.14"),
            ("9.5%", "0.095"),
            ("100%", "1"),
            ("250%", "2.5"),
            ("0%", "0"),
            ("-0%", "0"),
            ("-5%", "-0.05"),
            ("1234.5%", "12.345"),
            ("0.5%", "0.005"),
            ("10.50%", "0.105"),
        ] {
            assert_eq!(percent_value(text), value, "{text}");
        }
    }

    #[test]
    fn number_formats() {
        let format = |t: ColumnType| t.number_format();
        assert_eq!(format(ColumnType::Number { decimals: None }), "General");
        assert_eq!(format(ColumnType::Number { decimals: Some(2) }), "#,##0.00");
        assert_eq!(format(ColumnType::Number { decimals: Some(0) }), "#,##0");
        let currency = |code: &str| ColumnType::Currency { code: code.into() };
        assert_eq!(format(currency("USD")), "\"$\"#,##0.00");
        assert_eq!(format(currency("JPY")), "\"¥\"#,##0");
        assert_eq!(format(currency("CHF")), "\"CHF \"#,##0.00");
        assert_eq!(format(ColumnType::Percent { decimals: Some(1) }), "0.0%");
        assert_eq!(format(ColumnType::Date), "yyyy-mm-dd");
        assert_eq!(format(ColumnType::Text), "@");
        assert_eq!(literal_format("9.5%", Literal::Percent), "0.0%");
        assert_eq!(
            convert("2026-01-01", Literal::Date),
            Some(Converted::Number("46023".into()))
        );
        assert_eq!(
            convert("TRUE", Literal::Boolean),
            Some(Converted::Boolean(true))
        );
    }

    #[test]
    fn decimals_for_percent_formats() {
        assert_eq!(percent_decimals("14%", Literal::Percent), 0);
        assert_eq!(percent_decimals("9.5%", Literal::Percent), 1);
        assert_eq!(percent_decimals("0.14", Literal::Number), 0);
        assert_eq!(percent_decimals("0.095", Literal::Number), 1);
        assert_eq!(percent_decimals("1", Literal::Number), 0);
        assert_eq!(percent_decimals("0.000000000000001", Literal::Number), 10);
        assert_eq!(percent_decimals("2026-01-01", Literal::Date), 0);
    }

    #[test]
    fn conversion() {
        let currency = ColumnType::Currency { code: "USD".into() };
        assert!(currency.accepts(Literal::Number));
        assert!(!currency.accepts(Literal::Percent));
        assert!(!currency.accepts(Literal::Text));
        let percent = ColumnType::Percent { decimals: None };
        assert!(percent.accepts(Literal::Number) && percent.accepts(Literal::Percent));
        assert!(!ColumnType::Number { decimals: None }.accepts(Literal::Percent));
        assert!(ColumnType::Date.accepts(Literal::Date));
        assert!(!ColumnType::Date.accepts(Literal::Text));
        assert!(ColumnType::Text.accepts(Literal::Boolean));
    }
}
