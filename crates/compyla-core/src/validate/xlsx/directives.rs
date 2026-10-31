//! The xlsx directives' arguments (markdown-spec.md §1.6, §9.4): `anchor`, `table` and `columns`.
//! Each directive's arguments are parsed once, for a directive that took effect; `columns` keys are
//! then resolved against each table it applies to.

use std::collections::{BTreeMap, HashMap, HashSet};

use super::grid::{CellRef, column_index};
use super::names::{fold, valid_table_name};
use super::values::ColumnType;
use super::{TableStyle, excerpt, line_of};
use crate::diagnostic::{Diagnostic, Sink};
use crate::ir::Directive;
use crate::validate::{at, options};

/// A directive's parsed arguments.
pub enum Args {
    /// The cell, or `None` when the argument is invalid (the table is stacked).
    Anchor(Option<CellRef>),
    Table(TableOptions),
    Columns(Columns),
}

pub struct TableOptions {
    /// A name that is a valid Excel table name; uniqueness is checked across the workbook.
    pub name: Option<String>,
    pub style: TableStyle,
    pub autofilter: bool,
}

/// A `columns` entry: `KEY:type` or `KEY:type(param)`.
pub struct Entry {
    pub key: String,
    pub kind: ColumnType,
}

/// A `columns` directive's entries, with distinct keys, indexed for resolution against each table
/// in time proportional to the table's width.
pub struct Columns {
    pub entries: Vec<Entry>,
    /// Case-folded key → entry.
    by_key: HashMap<String, usize>,
    /// Column index → entry, for keys that are column letters.
    by_letter: BTreeMap<u32, usize>,
}

/// A `columns` directive resolved against one table.
pub struct Resolved {
    /// Each column's type, when an entry names it.
    pub typed: Vec<Option<ColumnType>>,
    /// The entries that named a column of this table.
    pub matched: Vec<usize>,
    /// Entries that named a column an earlier entry already named: `(entry, column)`.
    pub repeated: Vec<(usize, usize)>,
}

impl Columns {
    /// A key is a header cell's text (case-insensitive) if one matches, else a column letter
    /// (A = the table's first column). Entries apply in order; a column named again keeps the first.
    pub fn resolve(&self, headers: &[String]) -> Resolved {
        let mut found: Vec<(usize, usize)> = Vec::new();
        let mut by_header = HashSet::new();
        for (column, header) in headers.iter().enumerate() {
            if let Some(&entry) = self.by_key.get(&fold(header))
                && by_header.insert(entry)
            {
                found.push((entry, column));
            }
        }
        for (&column, &entry) in self.by_letter.range(..headers.len() as u32) {
            if !by_header.contains(&entry) {
                found.push((entry, column as usize));
            }
        }
        found.sort_unstable();
        let mut resolved = Resolved {
            typed: vec![None; headers.len()],
            matched: Vec::with_capacity(found.len()),
            repeated: Vec::new(),
        };
        for (entry, column) in found {
            resolved.matched.push(entry);
            if resolved.typed[column].is_some() {
                resolved.repeated.push((entry, column));
            } else {
                resolved.typed[column] = Some(self.entries[entry].kind.clone());
            }
        }
        resolved
    }
}

pub fn parse(directive: &Directive, out: &mut Sink) -> Args {
    match directive.name.as_str() {
        "anchor" => Args::Anchor(anchor(directive, out)),
        "table" => Args::Table(table(directive, out)),
        _ => Args::Columns(columns(directive, out)),
    }
}

fn anchor(directive: &Directive, out: &mut Sink) -> Option<CellRef> {
    let line = line_of(directive.position);
    let args = directive.args.trim();
    if args.is_empty() {
        out.push(at(
            Diagnostic::new(
                "invalid_directive_args",
                "`::: anchor` needs a cell; the table is stacked instead.",
            )
            .hint("For example `::: anchor C5`."),
            line,
        ));
        return None;
    }
    let cell = CellRef::parse(args);
    if cell.is_none() {
        out.push(at(
            Diagnostic::new(
                "invalid_directive_args",
                format!(
                    "`::: anchor {}` isn't a cell in A1:XFD1048576; the table is stacked instead.",
                    excerpt(args)
                ),
            )
            .hint("Give one cell reference, such as `::: anchor C5`."),
            line,
        ));
    }
    cell
}

fn table(directive: &Directive, out: &mut Sink) -> TableOptions {
    let line = line_of(directive.position);
    let mut push = |code: &str, message: String, hint: &str| {
        out.push(at(Diagnostic::new(code, message).hint(hint), line));
    };
    let parsed = options(&directive.args);
    if let Some(key) = &parsed.repeated {
        push(
            "invalid_directive_args",
            format!("`::: table` gives `{key}` more than once; the first is used."),
            "Give each option once.",
        );
    }
    let mut result = TableOptions {
        name: None,
        style: TableStyle::Striped,
        autofilter: false,
    };
    let mut seen = std::collections::HashSet::new();
    for (key, value) in &parsed.entries {
        if !seen.insert(key.as_str()) {
            continue;
        }
        match (key.as_str(), value.as_deref()) {
            ("name", Some(name)) if valid_table_name(name) => result.name = Some(name.to_string()),
            ("name", value) => push(
                "invalid_directive_args",
                format!(
                    "`name={}` isn't a valid Excel table name; the table gets an automatic name.",
                    value.unwrap_or_default()
                ),
                "Start with a letter or `_`, use only letters, digits, `_` and `.`, and don't use a cell reference such as `A1`.",
            ),
            ("style", Some(style)) if style.eq_ignore_ascii_case("striped") => {
                result.style = TableStyle::Striped;
            }
            ("style", Some(style)) if style.eq_ignore_ascii_case("plain") => {
                result.style = TableStyle::Plain;
            }
            ("style", Some(style)) => push(
                "unknown_directive_option",
                format!("`style={style}` isn't a table style; `striped` is used."),
                "Use `style=striped` or `style=plain`.",
            ),
            ("style", None) => push(
                "invalid_directive_args",
                "`style` needs a value; `striped` is used.".to_string(),
                "Use `style=striped` or `style=plain`.",
            ),
            ("autofilter", None) => result.autofilter = true,
            ("autofilter", Some(value)) => push(
                "invalid_directive_args",
                format!("`autofilter` is a flag and takes no value (`={value}`); it is ignored."),
                "Write `autofilter` on its own.",
            ),
            (other, _) => push(
                "unknown_directive_option",
                format!("`::: table` has no option `{other}`; it is ignored."),
                "Options: `name=…`, `style=striped|plain` and `autofilter`.",
            ),
        }
    }
    result
}

fn columns(directive: &Directive, out: &mut Sink) -> Columns {
    let line = line_of(directive.position);
    let mut push = |code: &str, message: String, hint: &str| {
        out.push(at(Diagnostic::new(code, message).hint(hint), line));
    };
    const TYPES: &str = "Types: number, currency, percent, date, text, boolean.";
    let mut columns = Columns {
        entries: Vec::new(),
        by_key: HashMap::new(),
        by_letter: BTreeMap::new(),
    };
    if directive.args.trim().is_empty() {
        push(
            "invalid_directive_args",
            "`::: columns` needs at least one entry; column types are inferred.".to_string(),
            "For example `::: columns B:currency, Growth:percent`.",
        );
        return columns;
    }
    for text in directive.args.split(',').map(str::trim) {
        let Some((key, kind, param)) = entry_parts(text) else {
            push(
                "invalid_directive_args",
                format!(
                    "`{}` isn't a `columns` entry; it is ignored.",
                    excerpt(text)
                ),
                "Write entries as `KEY:type` or `KEY:type(param)`, separated by commas.",
            );
            continue;
        };
        let Some((kind, param_check)) = column_type(kind, param) else {
            push(
                "unknown_directive_option",
                format!(
                    "`{}` isn't a column type; the entry is ignored.",
                    excerpt(kind)
                ),
                TYPES,
            );
            continue;
        };
        let code = match param_check {
            Param::Fine => None,
            Param::Unknown => Some("unknown_directive_option"),
            Param::Invalid => Some("invalid_directive_args"),
        };
        if let Some(code) = code {
            push(
                code,
                format!(
                    "`{}` isn't a valid parameter of `{}`; its default is used.",
                    excerpt(param.unwrap_or_default()),
                    kind.name()
                ),
                "number and percent take decimal places 0–10, currency a three-letter code, date `iso`; text and boolean take none.",
            );
        }
        // The same key always names the same column, so a repeated key is a column named twice.
        let index = columns.entries.len();
        let folded = fold(key);
        if columns.by_key.contains_key(&folded) {
            push(
                "invalid_directive_args",
                format!("`columns` names `{key}` more than once; the first entry is used."),
                "Give each column one entry.",
            );
            continue;
        }
        columns.by_key.insert(folded, index);
        if let Some(letter) = column_index(key) {
            columns.by_letter.insert(letter, index);
        }
        columns.entries.push(Entry {
            key: key.to_string(),
            kind,
        });
    }
    columns
}

/// Splits `KEY:type` or `KEY:type(param)` at its last colon (a header may contain colons).
fn entry_parts(text: &str) -> Option<(&str, &str, Option<&str>)> {
    let (key, spec) = text.rsplit_once(':')?;
    let (key, spec) = (key.trim(), spec.trim());
    let name_len = spec.bytes().take_while(u8::is_ascii_alphabetic).count();
    let (kind, rest) = spec.split_at(name_len);
    let rest = rest.trim_start();
    let param = if rest.is_empty() {
        None
    } else {
        Some(rest.strip_prefix('(')?.strip_suffix(')')?.trim())
    };
    (!key.is_empty() && !kind.is_empty()).then_some((key, kind, param))
}

/// What is wrong with an entry's parameter, as §1.6 classifies it: a value outside an enumeration,
/// or a parameter for a type that takes none, is unknown; a malformed or out-of-range one is
/// invalid. Either way the type's default applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Param {
    Fine,
    Unknown,
    Invalid,
}

/// The type an entry names, with its parameter checked. `None` for an unknown type.
fn column_type(kind: &str, param: Option<&str>) -> Option<(ColumnType, Param)> {
    let decimals = |p: &str| {
        (!p.is_empty() && p.len() <= 2 && p.bytes().all(|b| b.is_ascii_digit()))
            .then(|| p.parse::<u8>().ok())
            .flatten()
            .filter(|d| *d <= 10)
    };
    let check = |fine: bool, problem: Param| if fine { Param::Fine } else { problem };
    Some(match kind.to_ascii_lowercase().as_str() {
        "number" => {
            let d = param.and_then(decimals);
            (
                ColumnType::Number { decimals: d },
                check(param.is_none() || d.is_some(), Param::Invalid),
            )
        }
        "percent" => {
            let d = param.and_then(decimals);
            (
                ColumnType::Percent { decimals: d },
                check(param.is_none() || d.is_some(), Param::Invalid),
            )
        }
        "currency" => {
            let code = param
                .filter(|c| c.len() == 3 && c.bytes().all(|b| b.is_ascii_alphabetic()))
                .map(str::to_ascii_uppercase);
            (
                ColumnType::Currency {
                    code: code.clone().unwrap_or_else(|| "USD".into()),
                },
                check(param.is_none() || code.is_some(), Param::Invalid),
            )
        }
        "date" => (
            ColumnType::Date,
            check(
                param.is_none_or(|p| p.eq_ignore_ascii_case("iso")),
                Param::Unknown,
            ),
        ),
        "text" => (ColumnType::Text, check(param.is_none(), Param::Unknown)),
        "boolean" => (ColumnType::Boolean, check(param.is_none(), Param::Unknown)),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn directive(name: &str, args: &str) -> Directive {
        Directive {
            name: name.into(),
            args: args.into(),
            children: Vec::new(),
            position: Some(crate::ir::Position::new(3, 1)),
        }
    }

    fn codes(out: &[Diagnostic]) -> Vec<&str> {
        out.iter().map(|d| d.code.as_str()).collect()
    }

    #[test]
    fn columns_entries() {
        let mut out = Sink::new();
        let entries = columns(
            &directive(
                "columns",
                "B:currency, Q3 Actual:currency(eur), Ratio: a/b:percent(2), C:number(4), D:date(iso), E:Boolean",
            ),
            &mut out,
        );
        assert!(out.is_empty(), "{out:?}");
        let kinds: Vec<_> = entries
            .entries
            .iter()
            .map(|e| (e.key.as_str(), e.kind.clone()))
            .collect();
        assert_eq!(
            kinds,
            vec![
                ("B", ColumnType::Currency { code: "USD".into() }),
                ("Q3 Actual", ColumnType::Currency { code: "EUR".into() }),
                ("Ratio: a/b", ColumnType::Percent { decimals: Some(2) }),
                ("C", ColumnType::Number { decimals: Some(4) }),
                ("D", ColumnType::Date),
                ("E", ColumnType::Boolean),
            ]
        );
    }

    #[test]
    fn columns_problems() {
        let mut out = Sink::new();
        let entries = columns(
            &directive(
                "columns",
                "X, :number, A:, A:money, A:number(11), B:currency(EURO), C:text(x), D:date(us), E:number(2, a:text",
            ),
            &mut out,
        );
        assert_eq!(
            codes(&out),
            vec![
                "invalid_directive_args",
                "invalid_directive_args",
                "invalid_directive_args",
                "unknown_directive_option",
                "invalid_directive_args",
                "invalid_directive_args",
                "unknown_directive_option",
                "unknown_directive_option",
                "invalid_directive_args",
                "invalid_directive_args",
            ]
        );
        let kinds: Vec<_> = entries.entries.iter().map(|e| e.kind.clone()).collect();
        assert_eq!(
            kinds,
            vec![
                ColumnType::Number { decimals: None },
                ColumnType::Currency { code: "USD".into() },
                ColumnType::Text,
                ColumnType::Date,
            ]
        );
        let mut out = Sink::new();
        assert!(
            columns(&directive("columns", ""), &mut out)
                .entries
                .is_empty()
        );
        assert_eq!(codes(&out), vec!["invalid_directive_args"]);
    }

    #[test]
    fn columns_resolution() {
        let parsed = |args: &str| columns(&directive("columns", args), &mut Sink::new());
        let headers: Vec<String> = ["Region", "B", "Growth"].map(String::from).to_vec();
        let resolved = parsed("growth:percent, b:number, A:text").resolve(&headers);
        assert_eq!(
            resolved.typed,
            vec![
                Some(ColumnType::Text),
                Some(ColumnType::Number { decimals: None }),
                Some(ColumnType::Percent { decimals: None }),
            ]
        );
        assert_eq!(resolved.matched, vec![0, 1, 2]);
        assert!(resolved.repeated.is_empty());

        // `D` is past the table, `Nope` matches nothing, and `A` names Region's column again.
        let resolved = parsed("D:number, Region:text, A:number, Nope:date").resolve(&headers);
        assert_eq!(resolved.typed, vec![Some(ColumnType::Text), None, None]);
        assert_eq!(resolved.matched, vec![1, 2]);
        assert_eq!(resolved.repeated, vec![(2, 0)]);

        // A header named like a letter wins over the letter.
        let headers: Vec<String> = ["X", "A"].map(String::from).to_vec();
        let resolved = parsed("A:number").resolve(&headers);
        assert_eq!(
            resolved.typed,
            vec![None, Some(ColumnType::Number { decimals: None })]
        );
    }

    #[test]
    fn table_options() {
        let mut out = Sink::new();
        let options = table(
            &directive("table", "name=sales_q3, style=Plain, AutoFilter"),
            &mut out,
        );
        assert!(out.is_empty());
        assert_eq!(options.name.as_deref(), Some("sales_q3"));
        assert_eq!(options.style, TableStyle::Plain);
        assert!(options.autofilter);

        let mut out = Sink::new();
        let options = table(
            &directive("table", "name=A1 style=fancy sorted autofilter=yes style"),
            &mut out,
        );
        assert_eq!(
            codes(&out),
            vec![
                "invalid_directive_args",
                "invalid_directive_args",
                "unknown_directive_option",
                "unknown_directive_option",
                "invalid_directive_args",
            ]
        );
        assert_eq!(options.name, None);
        assert_eq!(options.style, TableStyle::Striped);
        assert!(!options.autofilter);
    }

    #[test]
    fn anchors() {
        let mut out = Sink::new();
        assert_eq!(
            anchor(&directive("anchor", "$c$5"), &mut out).map(|c| (c.column, c.row)),
            Some((2, 5))
        );
        for invalid in ["", "Z", "C5 D6", "XFE1", "A0"] {
            assert_eq!(anchor(&directive("anchor", invalid), &mut out), None);
        }
        assert_eq!(out.len(), 5);
    }
}
