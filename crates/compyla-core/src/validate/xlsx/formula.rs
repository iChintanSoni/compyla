//! Formulas (markdown-spec.md §9.3): the grammar, the function allowlist, table-relative reference
//! translation and the written form.
//!
//! The grammar is recognized in two steps. The lexer splits the formula into tokens: numbers,
//! strings, booleans, function calls (`fname [ws] "("`), references (a whole `reference`, including
//! its sheet qualifiers and range colon), operators, parentheses and commas, with whitespace allowed
//! between tokens. The recognizer is then a state machine with an explicit stack of open parentheses
//! and calls: precedence shapes the parse tree but not which formulas match, so this accepts exactly
//! the grammar's language, and arbitrarily deep nesting can't exhaust the call stack.

use std::ops::Range;

use super::excerpt;
use super::grid::{CellRef, MAX_COLUMNS, MAX_ROW};
use super::names::fold;

/// Pure spreadsheet functions only (§9.3).
const ALLOWLIST: &[&str] = &[
    "SUM",
    "SUMIF",
    "SUMIFS",
    "AVERAGE",
    "AVERAGEIF",
    "AVERAGEIFS",
    "COUNT",
    "COUNTA",
    "COUNTBLANK",
    "COUNTIF",
    "COUNTIFS",
    "MIN",
    "MAX",
    "MEDIAN",
    "IF",
    "IFS",
    "IFERROR",
    "IFNA",
    "AND",
    "OR",
    "NOT",
    "XOR",
    "VLOOKUP",
    "HLOOKUP",
    "INDEX",
    "MATCH",
    "LOOKUP",
    "ROUND",
    "ROUNDUP",
    "ROUNDDOWN",
    "ABS",
    "SQRT",
    "POWER",
    "MOD",
    "INT",
    "TRUNC",
    "CONCATENATE",
    "CONCAT",
    "TEXT",
    "LEFT",
    "RIGHT",
    "MID",
    "LEN",
    "TRIM",
    "UPPER",
    "LOWER",
    "SUBSTITUTE",
    "DATE",
    "YEAR",
    "MONTH",
    "DAY",
    "TODAY",
    "NOW",
    "DATEDIF",
    "ISBLANK",
    "ISNUMBER",
    "ISTEXT",
    "ISERROR",
    "ISNA",
];

/// Functions newer than Excel 2010, stored with the `_xlfn.` prefix.
const XLFN: &[&str] = &["IFS", "IFNA", "XOR", "CONCAT"];

/// Where the formula's table is placed, and the sheets the workbook has.
pub struct Context<'a> {
    /// The table's first column, 0-based.
    pub first_column: u32,
    /// The table's header row, 1-based.
    pub header_row: u32,
    /// The table's own grid: its column count and data row count.
    pub columns: u32,
    pub data_rows: u32,
    /// The workbook's repaired sheet names, case-folded.
    pub sheets: &'a [String],
}

/// A formula that matches the grammar and passes the allowlist and sheet checks.
#[derive(Debug, PartialEq, Eq)]
pub struct Formula {
    /// The written form, without the leading `=`.
    pub written: String,
    /// Unqualified references outside the table's own grid (`formula_ref_out_of_range`), as written
    /// in the source.
    pub out_of_range: Vec<String>,
    /// References whose translation leaves the sheet (`xlsx_limit_exceeded`). The formula can't be
    /// written when there are any.
    pub past_grid: Vec<String>,
}

/// Why a formula is a `formula_error`.
#[derive(Debug, PartialEq, Eq)]
pub struct Error {
    /// Phrased to follow "The formula …".
    pub reason: String,
    pub hint: &'static str,
}

impl Error {
    fn syntax(reason: String) -> Error {
        Error {
            reason,
            hint: "Formulas follow the grammar in markdown-spec.md §9.3; to write text that starts with `=`, escape it as `\\=`.",
        }
    }
}

/// Checks and translates `source` (a cell's `raw` text without the leading `=`).
pub fn translate(source: &str, context: &Context<'_>) -> Result<Formula, Error> {
    let tokens = lex(source).map_err(Error::syntax)?;
    recognize(source, &tokens).map_err(Error::syntax)?;
    for token in &tokens {
        if let Kind::Call { name, .. } = &token.kind
            && !ALLOWLIST.contains(&name.as_str())
        {
            return Err(Error {
                reason: format!("calls `{}`, which isn't an allowed function", excerpt(name)),
                hint: "Only pure spreadsheet functions are allowed, such as SUM, IF, ROUND, VLOOKUP and TEXT (markdown-spec.md §9.3).",
            });
        }
    }
    for token in &tokens {
        if let Kind::Reference(reference) = &token.kind {
            for endpoint in reference.endpoints() {
                if let Some(sheet) = &endpoint.sheet
                    && !context.sheets.contains(&fold(&sheet.name))
                {
                    return Err(Error {
                        reason: format!(
                            "refers to sheet `{}`, which the workbook doesn't have",
                            excerpt(&sheet.name)
                        ),
                        hint: "Use a sheet's name as it is written to the workbook (after repairs), quoted when it isn't a plain name: `'Q3 Data'!A1`.",
                    });
                }
            }
        }
    }

    let mut formula = Formula {
        written: String::with_capacity(source.len() + 8),
        out_of_range: Vec::new(),
        past_grid: Vec::new(),
    };
    let mut last = 0;
    for token in &tokens {
        formula.written.push_str(&source[last..token.start]);
        let text = &source[token.start..token.end];
        match &token.kind {
            Kind::Call { name, name_end } => {
                if XLFN.contains(&name.as_str()) {
                    formula.written.push_str("_xlfn.");
                }
                formula.written.push_str(name);
                formula.written.push_str(&source[*name_end..token.end]);
            }
            Kind::Reference(reference) => {
                let (inside, past) = reference.write(source, context, &mut formula.written);
                if !inside {
                    formula.out_of_range.push(text.to_string());
                }
                if past {
                    formula.past_grid.push(text.to_string());
                }
            }
            _ => formula.written.push_str(text),
        }
        last = token.end;
    }
    formula.written.push_str(&source[last..]);
    Ok(formula)
}

#[derive(Debug)]
struct Token {
    kind: Kind,
    start: usize,
    end: usize,
}

#[derive(Debug)]
enum Kind {
    /// A number, string or boolean.
    Value,
    Reference(Reference),
    /// `fname [ws] "("`: the uppercase name, and where the name ends in the source.
    Call {
        name: String,
        name_end: usize,
    },
    Open,
    Close,
    Comma,
    /// `+` or `-`: prefix or binary.
    Sign,
    /// `*`, `/`, `^`, `&` and the comparisons.
    Binary,
    Percent,
}

/// `[ sheet "!" ] cell [ ":" [ sheet "!" ] cell ]`.
#[derive(Debug)]
struct Reference {
    first: Endpoint,
    second: Option<Endpoint>,
}

#[derive(Debug)]
struct Endpoint {
    sheet: Option<Sheet>,
    cell: CellRef,
}

#[derive(Debug)]
struct Sheet {
    /// The name, unquoted and unescaped.
    name: String,
    /// Its source text, quotes included.
    span: Range<usize>,
}

impl Reference {
    fn endpoints(&self) -> impl Iterator<Item = &Endpoint> {
        std::iter::once(&self.first).chain(self.second.as_ref())
    }

    /// Writes the reference: a sheet-qualified one unchanged but for uppercase cells, an
    /// unqualified one translated to the placed address. Returns whether every unqualified cell is
    /// inside the table's own grid, and whether a translation left the sheet.
    fn write(&self, source: &str, context: &Context<'_>, out: &mut String) -> (bool, bool) {
        // `Sheet!A1:B2` qualifies both cells; `A1:Sheet!B2` only the second.
        let range_qualified = self.first.sheet.is_some();
        let (mut inside, mut past) = (true, false);
        for (index, endpoint) in self.endpoints().enumerate() {
            if index == 1 {
                out.push(':');
            }
            if let Some(sheet) = &endpoint.sheet {
                out.push_str(&source[sheet.span.clone()]);
                out.push('!');
            }
            let cell = endpoint.cell;
            if range_qualified || endpoint.sheet.is_some() {
                out.push_str(&cell.to_string());
                continue;
            }
            inside &= cell.column < context.columns && cell.row <= 1 + context.data_rows;
            let column = u64::from(cell.column) + u64::from(context.first_column);
            let row = u64::from(cell.row) + u64::from(context.header_row) - 1;
            if column >= u64::from(MAX_COLUMNS) || row > u64::from(MAX_ROW) {
                past = true;
                out.push_str(&cell.to_string());
            } else {
                let placed = CellRef {
                    column: column as u32,
                    row: row as u32,
                    ..cell
                };
                out.push_str(&placed.to_string());
            }
        }
        (inside, past)
    }
}

fn lex(source: &str) -> Result<Vec<Token>, String> {
    let bytes = source.as_bytes();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let start = i;
        let kind = match bytes[i] {
            b' ' | b'\t' => {
                i += 1;
                continue;
            }
            b'0'..=b'9' => {
                i = number(bytes, i);
                Kind::Value
            }
            b'.' if bytes.get(i + 1).is_some_and(u8::is_ascii_digit) => {
                i = number(bytes, i);
                Kind::Value
            }
            b'"' => {
                i = string(bytes, i)?;
                Kind::Value
            }
            b'\'' => {
                let (reference, end) = reference(source, i)?;
                i = end;
                Kind::Reference(reference)
            }
            b'$' | b'_' | b'A'..=b'Z' | b'a'..=b'z' => {
                let end = word_end(bytes, i);
                let word = &source[i..end];
                let after = skip_whitespace(bytes, end);
                if bytes.get(after) == Some(&b'(') {
                    // §9.3: a name followed by `(` is always a function call, even `LOG10(`.
                    if !is_fname(word) {
                        return Err(format!(
                            "calls `{}`, which isn't a function name",
                            excerpt(word)
                        ));
                    }
                    i = after + 1;
                    Kind::Call {
                        name: word.to_ascii_uppercase(),
                        name_end: end,
                    }
                } else if bytes.get(end) == Some(&b'!') || CellRef::parse(word).is_some() {
                    let (reference, end) = reference(source, i)?;
                    i = end;
                    Kind::Reference(reference)
                } else if word.eq_ignore_ascii_case("TRUE") || word.eq_ignore_ascii_case("FALSE") {
                    i = end;
                    Kind::Value
                } else {
                    return Err(format!(
                        "uses `{}`, which isn't a cell reference or function call; defined names, \
                         structured references and whole-column or whole-row ranges aren't supported",
                        excerpt(word)
                    ));
                }
            }
            b'+' | b'-' => {
                i += 1;
                Kind::Sign
            }
            b'*' | b'/' | b'^' | b'&' | b'=' => {
                i += 1;
                Kind::Binary
            }
            b'<' => {
                i += if matches!(bytes.get(i + 1), Some(b'>' | b'=')) {
                    2
                } else {
                    1
                };
                Kind::Binary
            }
            b'>' => {
                i += if bytes.get(i + 1) == Some(&b'=') {
                    2
                } else {
                    1
                };
                Kind::Binary
            }
            b'%' => {
                i += 1;
                Kind::Percent
            }
            b'(' => {
                i += 1;
                Kind::Open
            }
            b')' => {
                i += 1;
                Kind::Close
            }
            b',' => {
                i += 1;
                Kind::Comma
            }
            _ => {
                let c = source[i..].chars().next().unwrap_or_default();
                let why = match c {
                    '#' => "; error literals aren't supported",
                    '{' => "; array constants aren't supported",
                    '[' => "; structured references aren't supported",
                    ':' => {
                        "; ranges are written without spaces, and whole-column or whole-row ranges aren't supported"
                    }
                    '!' => "; a sheet name must come right before `!`",
                    _ => "",
                };
                return Err(format!("has an unexpected `{c}`{why}"));
            }
        };
        tokens.push(Token {
            kind,
            start,
            end: i,
        });
    }
    Ok(tokens)
}

/// `( 1*DIGIT [ "." *DIGIT ] / "." 1*DIGIT ) [ ( "e" / "E" ) [ "+" / "-" ] 1*DIGIT ]`, from `i`.
fn number(bytes: &[u8], mut i: usize) -> usize {
    let digits = |mut i: usize| {
        while bytes.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
        i
    };
    i = digits(i);
    if bytes.get(i) == Some(&b'.') {
        i = digits(i + 1);
    }
    if matches!(bytes.get(i), Some(b'e' | b'E')) {
        let mut j = i + 1;
        if matches!(bytes.get(j), Some(b'+' | b'-')) {
            j += 1;
        }
        if bytes.get(j).is_some_and(u8::is_ascii_digit) {
            i = digits(j);
        }
    }
    i
}

/// `DQUOTE *( any character except DQUOTE / 2DQUOTE ) DQUOTE`, from the opening quote.
fn string(bytes: &[u8], i: usize) -> Result<usize, String> {
    let mut j = i + 1;
    loop {
        match bytes.get(j) {
            None => return Err("has a string with no closing `\"`".into()),
            Some(b'"') if bytes.get(j + 1) == Some(&b'"') => j += 2,
            Some(b'"') => return Ok(j + 1),
            Some(_) => j += 1,
        }
    }
}

fn reference(source: &str, i: usize) -> Result<(Reference, usize), String> {
    let (first, mut i) = endpoint(source, i)?;
    let second = if source.as_bytes().get(i) == Some(&b':') {
        let (second, end) = endpoint(source, i + 1)?;
        i = end;
        Some(second)
    } else {
        None
    };
    Ok((Reference { first, second }, i))
}

/// `[ sheet "!" ] cell`, from `i`.
fn endpoint(source: &str, mut i: usize) -> Result<(Endpoint, usize), String> {
    let bytes = source.as_bytes();
    let mut sheet = None;
    if bytes.get(i) == Some(&b'\'') {
        let (name, end) = quoted_sheet(source, i)?;
        if bytes.get(end) != Some(&b'!') {
            return Err(format!(
                "has the quoted sheet name `{}` without `!` and a cell after it",
                excerpt(&source[i..end])
            ));
        }
        sheet = Some(Sheet { name, span: i..end });
        i = end + 1;
    } else {
        let end = word_end(bytes, i);
        if bytes.get(end) == Some(&b'!') {
            let word = &source[i..end];
            if !is_uname(word) {
                let word = excerpt(word);
                return Err(format!(
                    "uses `{word}` as a sheet name; it must be quoted, as in `'{word}'!A1`"
                ));
            }
            sheet = Some(Sheet {
                name: word.to_string(),
                span: i..end,
            });
            i = end + 1;
        }
    }
    let end = word_end(bytes, i);
    let word = &source[i..end];
    match CellRef::parse(word) {
        Some(cell) => Ok((Endpoint { sheet, cell }, end)),
        None if word.is_empty() => Err("has a reference with no cell".into()),
        None => Err(format!(
            "has `{}` where a cell reference belongs",
            excerpt(word)
        )),
    }
}

/// `"'" 1*( qchar / "''" ) "'"`, from the opening quote: the unescaped name and the end.
fn quoted_sheet(source: &str, i: usize) -> Result<(String, usize), String> {
    let mut name = String::new();
    let mut chars = source[i + 1..].char_indices();
    while let Some((offset, c)) = chars.next() {
        match c {
            '\'' => {
                let after = i + 1 + offset + 1;
                if source.as_bytes().get(after) == Some(&b'\'') {
                    name.push('\'');
                    chars.next();
                } else if name.is_empty() {
                    return Err("has an empty quoted sheet name".into());
                } else {
                    return Ok((name, after));
                }
            }
            ':' | '\\' | '/' | '?' | '*' | '[' | ']' => {
                return Err(format!(
                    "has a quoted sheet name containing `{c}`, which sheet names can't contain"
                ));
            }
            _ => name.push(c),
        }
    }
    Err("has a quoted sheet name with no closing `'`".into())
}

/// The end of a run of name characters: letters, digits, `_`, `.` and `$`.
fn word_end(bytes: &[u8], mut i: usize) -> usize {
    while bytes
        .get(i)
        .is_some_and(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'$'))
    {
        i += 1;
    }
    i
}

fn skip_whitespace(bytes: &[u8], mut i: usize) -> usize {
    while matches!(bytes.get(i), Some(b' ' | b'\t')) {
        i += 1;
    }
    i
}

/// `fname = ALPHA *( ALPHA / DIGIT / "." )`.
fn is_fname(word: &str) -> bool {
    word.starts_with(|c: char| c.is_ascii_alphabetic())
        && word.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.')
}

/// `uname = ( ALPHA / "_" ) *( ALPHA / DIGIT / "_" )`, and not itself a valid cell.
fn is_uname(word: &str) -> bool {
    word.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
        && word.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        && CellRef::parse(word).is_none()
}

/// The grammar over the token stream: operands separated by binary operators, each operand
/// `*( "+" / "-" ) primary *( "%" )`, where a primary is a value, a reference, a parenthesized
/// expression, or a call whose arguments may be empty.
fn recognize(source: &str, tokens: &[Token]) -> Result<(), String> {
    #[derive(Clone, Copy, PartialEq)]
    enum State {
        /// An operand must come next.
        Operand,
        /// Right after a call's `(` or `,`: an operand, or an empty argument.
        Argument,
        /// After a complete operand.
        After,
    }
    #[derive(PartialEq)]
    enum Open {
        Parenthesis,
        Call,
    }
    if tokens.is_empty() {
        return Err("is empty".into());
    }
    let mut stack = Vec::new();
    let mut state = State::Operand;
    for token in tokens {
        state = match (state, &token.kind) {
            (State::Operand | State::Argument, Kind::Sign) => State::Operand,
            (State::Operand | State::Argument, Kind::Value | Kind::Reference(_)) => State::After,
            (State::Operand | State::Argument, Kind::Open) => {
                stack.push(Open::Parenthesis);
                State::Operand
            }
            (State::Operand | State::Argument, Kind::Call { .. }) => {
                stack.push(Open::Call);
                State::Argument
            }
            (State::Argument, Kind::Comma) => State::Argument,
            (State::Argument, Kind::Close) => {
                stack.pop();
                State::After
            }
            (State::After, Kind::Percent) => State::After,
            (State::After, Kind::Sign | Kind::Binary) => State::Operand,
            (State::After, Kind::Close) if !stack.is_empty() => {
                stack.pop();
                State::After
            }
            (State::After, Kind::Comma) if stack.last() == Some(&Open::Call) => State::Argument,
            _ => {
                let text = excerpt(&source[token.start..token.end]);
                return Err(match &token.kind {
                    Kind::Close => "has a `)` with no matching `(`".to_string(),
                    Kind::Comma => "has a `,` outside a function call".to_string(),
                    _ if state == State::After => {
                        format!(
                            "has `{text}` right after an operand, with no operator between them"
                        )
                    }
                    _ => format!("has an unexpected `{text}`"),
                });
            }
        };
    }
    if state != State::After {
        return Err("is incomplete: it ends where an operand belongs".into());
    }
    if !stack.is_empty() {
        return Err("is incomplete: a `(` is never closed".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Translates for a 4-column, 3-row table placed at `first_column` and `header_row`, in a
    /// workbook with the sheets `Revenue`, `Q3 Data` and `It's`. An error is its reason.
    fn at(source: &str, first_column: u32, header_row: u32) -> Result<Formula, String> {
        let sheets: Vec<String> = ["Revenue", "Q3 Data", "It's"].map(fold).to_vec();
        translate(
            source,
            &Context {
                first_column,
                header_row,
                columns: 4,
                data_rows: 3,
                sheets: &sheets,
            },
        )
        .map_err(|error| error.reason)
    }

    fn written(source: &str) -> String {
        at(source, 0, 3).unwrap().written
    }

    #[test]
    fn grammar_accepts() {
        for source in [
            "1",
            "-1",
            "--+1",
            "1%%",
            ".5",
            "5.",
            "1.5e10",
            "2E-3",
            "\"text\"",
            "\"say \"\"hi\"\"\"",
            "TRUE",
            "false",
            "A1",
            "$A$1",
            "A1:B2",
            "(A1)",
            "((1+2)*3)^-2",
            "A2*B2*C2",
            "A1&\" \"&B1",
            "A1<>B1",
            "A1<=B1",
            "A1>=B1",
            "A1=B1",
            "-A1%",
            "SUM()",
            "SUM(A1,)",
            "IF(A1,,1)",
            "SUM ( A1 , B2 )",
            " SUM(A1) ",
            "IFS(A2>1,\"y\")",
            "Revenue!B5",
            "'Q3 Data'!A1:B2",
            "'It''s'!A1",
            "Revenue!A1:Revenue!B2",
            "A1:Revenue!B2",
            "IF(AND(A1>0,NOT(B1)),ROUND(A1/B1,2),\"-\")",
        ] {
            assert!(at(source, 0, 1).is_ok(), "{source}: {:?}", at(source, 0, 1));
        }
    }

    #[test]
    fn grammar_rejects() {
        for source in [
            "",
            " ",
            "SUM(",
            "SUM(A1",
            "(1",
            "1)",
            "()",
            "1+",
            "*1",
            "1 2",
            "A2 B2",
            "A1 (2)",
            "(1)(2)",
            "1,2",
            "(1,2)",
            "A1:",
            "A:B",
            "B:B",
            "2:5",
            "A1 : B2",
            "Sheet1:Sheet3!A1",
            "Table1[Col]",
            "#N/A",
            "{1,2}",
            "total",
            "\"open",
            "'Q3 Data'",
            "'Q3 Data'A1",
            "''!A1",
            "'unterminated!A1",
            "'C:\\x\\[evil.xlsx]Sheet1'!A1",
            "Q3!A1",
            "A1!B2",
            "XFE1",
            "A0",
            "_xlfn.IFS(1)",
            "$SUM(1)",
            "A1.5",
            "1E",
            "=A1",
            "A1;B1",
            "@A1",
        ] {
            assert!(at(source, 0, 1).is_err(), "{source} should not parse");
        }
    }

    #[test]
    fn names_before_parentheses_are_calls() {
        assert_eq!(
            at("LOG10(100)", 0, 1),
            Err("calls `LOG10`, which isn't an allowed function".into())
        );
        assert!(at("LOG10", 0, 1).is_ok(), "without `(`, LOG10 is a cell");
        assert!(at("A1(2)", 0, 1).unwrap_err().contains("`A1`"));
        assert!(at("TRUE()", 0, 1).is_err());
        assert!(at("WEBSERVICE(\"http://x\")", 0, 1).is_err());
    }

    #[test]
    fn sheet_qualifiers_are_checked() {
        assert!(at("Other!A1", 0, 1).unwrap_err().contains("`Other`"));
        assert!(
            at("revenue!A1", 0, 1).is_ok(),
            "sheet names are case-insensitive"
        );
        assert!(at("'q3 data'!A1", 0, 1).is_ok());
        assert!(at("A1:Other!B2", 0, 1).is_err());
    }

    #[test]
    fn written_form() {
        assert_eq!(written("sum(b2:b3)"), "SUM(B4:B5)");
        assert_eq!(written("B2/C2-1"), "B4/C4-1");
        assert_eq!(written("$B$2+B$2+$B2"), "$B$4+B$4+$B4");
        assert_eq!(written("ifs(A2>1,\"y\")"), "_xlfn.IFS(A4>1,\"y\")");
        assert_eq!(
            written("IfNa(1,2)&Concat(\"a\")"),
            "_xlfn.IFNA(1,2)&_xlfn.CONCAT(\"a\")"
        );
        assert_eq!(written("XOR(TRUE,false)"), "_xlfn.XOR(TRUE,false)");
        assert_eq!(written("SUM ( A2 , B3 )"), "SUM ( A4 , B5 )");
        assert_eq!(
            written("\"B2\"&B2"),
            "\"B2\"&B4",
            "strings aren't translated"
        );
        assert_eq!(
            written("revenue!b5+'Q3 Data'!a1:b2"),
            "revenue!B5+'Q3 Data'!A1:B2"
        );
        assert_eq!(written("A2:Revenue!B2"), "A4:Revenue!B2");
        assert_eq!(at("SUM(A2:B3)", 2, 8).unwrap().written, "SUM(C9:D10)");
    }

    #[test]
    fn references_outside_the_table() {
        // The grid: columns A..D, rows 1..4.
        let formula = at("A5+D4+E1+SUM(A1:A9)+Revenue!Z99", 0, 1).unwrap();
        assert_eq!(formula.out_of_range, vec!["A5", "E1", "A1:A9"]);
        assert_eq!(formula.written, "A5+D4+E1+SUM(A1:A9)+Revenue!Z99");
        assert!(formula.past_grid.is_empty());

        let past = at("A2+B1048576", 16_383, 1).unwrap();
        assert_eq!(past.past_grid, vec!["B1048576"]);
        let past = at("A1048576", 0, 3).unwrap();
        assert_eq!(past.past_grid, vec!["A1048576"]);
    }

    #[test]
    fn deep_nesting_does_not_overflow() {
        let source = format!("{}1{}", "(".repeat(100_000), ")".repeat(100_000));
        assert!(at(&source, 0, 1).is_ok());
        let source = format!("{}1", "SUM(".repeat(100_000));
        assert!(at(&source, 0, 1).is_err());
    }
}
