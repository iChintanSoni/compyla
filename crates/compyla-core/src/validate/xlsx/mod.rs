//! The xlsx profile (markdown-spec.md §9): a workbook authored as sheets of tables.
//!
//! [`compose`] builds the workbook model (sheets, placed ranges, column types, translated formulas,
//! Excel tables) together with the profile's diagnostics, so `lint` and the compose summary come
//! from the same code and can't disagree.
//!
//! - `walk`: the document-order walk (§9.1) and attachment of titles, notes and directives (§9.4)
//! - `compose`: sheet names (§9.6), placement (§9.2), values (§9.5) and formulas (§9.3) per table
//! - `formula`: the formula grammar, allowlist, translation and written form (§9.3)
//! - `values`: literal tests, inference and conversion (§9.5)
//! - `names`: sheet-name repairs, Excel table names and headers (§9.6)
//! - `directives`: `anchor`, `table` and `columns` arguments (§1.6, §9.4)
//! - `grid`: A1 addressing

mod compose;
mod directives;
mod formula;
mod grid;
mod names;
mod values;
mod walk;

use serde_json::{Map, Value, json};

pub use compose::compose;
pub use values::ColumnType;

use super::{Input, Slugger, fragment_links};
use crate::diagnostic::Sink;
use crate::ir::{Document, Position, Run};

/// The composed workbook.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workbook {
    pub sheets: Vec<Sheet>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sheet {
    /// The repaired name (§9.6).
    pub name: String,
    pub ranges: Vec<Range>,
}

/// A table's placed range (§9.2). Rows are 1-based; columns are 0-based (A is 0).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Range {
    pub title_row: Option<u32>,
    pub note_row: Option<u32>,
    pub header_row: u32,
    pub first_column: u32,
    /// The column count, including columns a long row added (§5).
    pub columns: u32,
    pub last_row: u32,
    pub column_types: Vec<ColumnType>,
    /// Placed address → written formula without the leading `=`, in row order. Cells whose formula
    /// is an error are written as text and are not listed.
    pub formulas: Vec<(String, String)>,
    pub excel_table: Option<ExcelTable>,
    /// Every cell the range writes, in row-major order.
    pub cells: Vec<Cell>,
}

/// A cell, resolved for writing (§9.2–§9.6): a renderer serializes it as it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cell {
    /// 1-based.
    pub row: u32,
    /// 0-based: A is 0.
    pub column: u32,
    pub role: Role,
    pub value: CellValue,
    /// The number format code (§9.4, §9.5).
    pub format: String,
    pub link: Option<Link>,
}

/// What a cell is in its range, which decides its style (§9.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Title,
    Note,
    Header,
    /// A header cell of an Excel table, which the table's style formats (§9.6).
    TableHeader,
    Data,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CellValue {
    /// No value: an empty header cell, written so the header row is styled whole.
    Empty,
    /// Rich text (§9.5): bold, italic and strike runs; `code` runs are plain.
    Text(Vec<Run>),
    /// An exact decimal: a percent already divided by 100, a date as its serial (§9.5).
    Number(String),
    Boolean(bool),
    /// The written formula, without `=` (§9.3).
    Formula(String),
}

/// A cell's hyperlink (§9.5): an external destination, or a fragment resolved to a sheet, whose
/// cell A1 it links to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Link {
    Url(String),
    Sheet(String),
}

/// An Excel table registered with `::: table` (§9.6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExcelTable {
    pub name: String,
    pub style: TableStyle,
    pub autofilter: bool,
    /// The header cells, repaired to be non-empty and unique.
    pub headers: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableStyle {
    Striped,
    Plain,
}

impl TableStyle {
    /// The Excel built-in style it maps to.
    pub fn excel_name(self) -> &'static str {
        match self {
            TableStyle::Striped => "TableStyleMedium2",
            TableStyle::Plain => "TableStyleLight1",
        }
    }
}

pub fn validate(input: &Input<'_>, out: &mut Sink) -> Workbook {
    let title_line = input.sidecar.meta_lines.get("title").copied();
    let workbook = compose(input.document, title_line, out);
    // §9.5: fragment links resolve against the sheet names' slugs.
    let mut slugger = Slugger::new();
    let targets: Vec<String> = workbook
        .sheets
        .iter()
        .map(|sheet| slugger.slug(&sheet.name))
        .collect();
    fragment_links(input, &targets, "sheet name", out);
    workbook
}

/// The compose summary (spec/conformance/compose.schema.json): sheets with their placed ranges,
/// column types and translated formulas.
pub fn summary(document: &Document) -> Value {
    let workbook = compose(document, None, &mut Sink::new());
    let sheets: Vec<Value> = workbook
        .sheets
        .iter()
        .map(|sheet| {
            let ranges: Vec<Value> = sheet.ranges.iter().map(range_summary).collect();
            json!({ "name": sheet.name, "ranges": ranges })
        })
        .collect();
    json!({ "format": "xlsx", "sheets": sheets })
}

fn range_summary(range: &Range) -> Value {
    let formulas: Map<String, Value> = range
        .formulas
        .iter()
        .map(|(address, formula)| (address.clone(), Value::from(formula.as_str())))
        .collect();
    let types: Vec<&str> = range.column_types.iter().map(ColumnType::name).collect();
    json!({
        "titleRow": range.title_row,
        "noteRow": range.note_row,
        "headerRow": range.header_row,
        "firstColumn": grid::column_letters(range.first_column),
        "lastColumn": grid::column_letters(range.first_column + range.columns - 1),
        "lastRow": range.last_row,
        "columnTypes": types,
        "formulas": formulas,
    })
}

fn line_of(position: Option<Position>) -> Option<usize> {
    position.map(|p| p.line)
}

/// Source text quoted in a message, shortened when long.
fn excerpt(text: &str) -> String {
    const MAX: usize = 60;
    match text.char_indices().nth(MAX) {
        Some((end, _)) => format!("{}…", &text[..end]),
        None => text.to_string(),
    }
}

/// End-to-end behavior the conformance fixtures don't pin.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::validate::{Asset, Format};

    /// The validate-stage diagnostics (shared rules included) as sorted `(code, line)` pairs.
    fn lint(source: &str) -> Vec<(String, Option<usize>)> {
        let parsed = crate::parse::parse(source);
        let input = Input {
            document: &parsed.document,
            sidecar: &parsed.sidecar,
            assets: &|_| Asset::Exists,
        };
        let mut sink = Sink::new();
        crate::validate::validate(&input, Format::Xlsx, &mut sink);
        let mut out = sink.into_vec();
        crate::diagnostic::sort(&mut out);
        out.into_iter().map(|d| (d.code, d.line)).collect()
    }

    fn workbook(source: &str) -> Workbook {
        compose(
            &crate::parse::parse(source).document,
            None,
            &mut Sink::new(),
        )
    }

    fn diagnostics(pairs: &[(&str, usize)]) -> Vec<(String, Option<usize>)> {
        pairs
            .iter()
            .map(|(code, line)| (code.to_string(), Some(*line)))
            .collect()
    }

    /// `(headerRow, firstColumn)` of every range on the first sheet.
    fn placements(source: &str) -> Vec<(u32, u32)> {
        workbook(source).sheets[0]
            .ranges
            .iter()
            .map(|r| (r.header_row, r.first_column))
            .collect()
    }

    #[test]
    fn overlap_relocates_below_everything_and_moves_the_cursor() {
        let source = "# S\n\n::: anchor A1\n:::\n\n| A | B |\n|---|---|\n| 1 | 2 |\n\n\
                      ::: anchor B2\n:::\n\n| C |\n|---|\n| 3 |\n\n| D |\n|---|\n| 4 |\n";
        assert_eq!(placements(source), vec![(1, 0), (4, 0), (7, 0)]);
        assert_eq!(lint(source), diagnostics(&[("xlsx_range_overlap", 13)]));
    }

    #[test]
    fn stacked_table_overlapping_an_anchored_one_is_relocated() {
        let source =
            "# S\n\n::: anchor A3\n:::\n\n| A |\n|---|\n| 1 |\n\n| B |\n|---|\n| 1 |\n| 2 |\n";
        assert_eq!(placements(source), vec![(3, 0), (6, 0)]);
        assert_eq!(lint(source), diagnostics(&[("xlsx_range_overlap", 10)]));
    }

    #[test]
    fn an_excel_table_without_data_rows_gets_an_empty_one() {
        let source = "# S\n\n::: table\n| A | B |\n|---|---|\n:::\n\n| C |\n|---|\n| 1 |\n";
        let ranges = &workbook(source).sheets[0].ranges;
        assert_eq!((ranges[0].header_row, ranges[0].last_row), (1, 2));
        // The next table stacks below the empty row, with the usual blank row between.
        assert_eq!(ranges[1].header_row, 4);
        assert_eq!(lint(source), vec![]);
    }

    #[test]
    fn anchored_tables_leave_the_cursor() {
        let source = "# S\n\n::: anchor D2\n| A |\n|---|\n| 1 |\n:::\n\n| B |\n|---|\n| 1 |\n";
        assert_eq!(placements(source), vec![(2, 3), (1, 0)]);
        assert_eq!(lint(source), vec![]);
    }

    #[test]
    fn wrapping_anchor_applies_to_its_first_table_only() {
        let source = "# S\n\n::: anchor D1\n| A |\n|---|\n| 1 |\n\n| B |\n|---|\n| 1 |\n:::\n";
        assert_eq!(placements(source), vec![(1, 3), (1, 0)]);
        assert_eq!(lint(source), diagnostics(&[("directive_misplaced", 3)]));
    }

    #[test]
    fn wrapping_columns_overrides_a_waiting_empty_one() {
        let source = "# S\n\n::: columns A:text\n:::\n\n::: columns A:boolean\n| A |\n|---|\n| true |\n:::\n";
        let sheet = &workbook(source).sheets[0];
        assert_eq!(sheet.ranges[0].column_types, vec![ColumnType::Boolean]);
        // The overridden one has no effect (§9.4).
        assert_eq!(lint(source), diagnostics(&[("directive_misplaced", 3)]));
    }

    #[test]
    fn columns_wrapping_several_tables_types_each() {
        let source =
            "# S\n\n::: columns A:currency(EUR)\n| A |\n|---|\n| 1 |\n\n| B |\n|---|\n| x |\n:::\n";
        let types: Vec<_> = workbook(source).sheets[0]
            .ranges
            .iter()
            .map(|r| r.column_types.clone())
            .collect();
        let euro = ColumnType::Currency { code: "EUR".into() };
        assert_eq!(types, vec![vec![euro.clone()], vec![euro]]);
        assert_eq!(lint(source), diagnostics(&[("typed_value_invalid", 10)]));
    }

    #[test]
    fn columns_keys_are_reported_once_per_directive() {
        // `Growth` names a column of the first table only; `Nope` names none in either.
        let source = "# S\n\n::: columns Growth:percent, Nope:date, A:text, Region:number\n\
                      | Region | Growth |\n|---|---|\n| x | 1% |\n\n| Region |\n|---|\n| y |\n:::\n";
        let types: Vec<_> = workbook(source).sheets[0]
            .ranges
            .iter()
            .map(|r| r.column_types.clone())
            .collect();
        assert_eq!(
            types,
            vec![
                vec![ColumnType::Text, ColumnType::Percent { decimals: Some(0) }],
                vec![ColumnType::Text]
            ]
        );
        // `Region` names column A again (after `A:text`): reported once, not per table.
        assert_eq!(
            lint(source),
            diagnostics(&[
                ("invalid_directive_args", 3),
                ("unknown_directive_option", 3)
            ])
        );
    }

    #[test]
    fn many_tables_place_in_linear_time() {
        // Built as IR, so the test measures placement rather than parsing: 50,000 anchored
        // one-cell tables down column Z, each followed by a stacked two-row table.
        use crate::ir::{Block, Cell, Directive, Run, Section, Table};
        let table = |rows: usize| {
            let cell = Cell {
                runs: vec![Run {
                    text: "1".into(),
                    ..Run::default()
                }],
                raw: "1".into(),
            };
            Block::Table(Table {
                header: vec![cell.clone()],
                align: vec![None],
                rows: vec![vec![cell]; rows],
                position: None,
            })
        };
        let mut children = Vec::new();
        for i in 0..50_000 {
            children.push(Block::Directive(Directive {
                name: "anchor".into(),
                args: format!("Z{}", 2 * i + 1),
                children: Vec::new(),
                position: None,
            }));
            children.push(table(0));
            children.push(table(1));
        }
        let document = Document {
            ir_version: crate::ir::IR_VERSION.into(),
            meta: Map::new(),
            body: vec![Block::Section(Section {
                level: 1,
                title: Vec::new(),
                children,
                position: None,
            })],
        };
        let mut diagnostics = Sink::new();
        let workbook = compose(&document, None, &mut diagnostics);
        let ranges = &workbook.sheets[0].ranges;
        assert_eq!(ranges.len(), 100_000);
        assert_eq!(
            (ranges[99_999].header_row, ranges[99_999].first_column),
            (149_998, 0)
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "only the empty sheet name: {diagnostics:?}"
        );
    }

    #[test]
    fn h1_ends_an_annotation() {
        let source = "# A\n\n::: columns A:text\n# B\n\n| x |\n|---|\n| 1 |\n:::\n";
        let workbook = workbook(source);
        assert_eq!(workbook.sheets.len(), 2);
        assert_eq!(
            workbook.sheets[1].ranges[0].column_types,
            vec![ColumnType::Number { decimals: None }]
        );
        assert_eq!(lint(source), diagnostics(&[("directive_misplaced", 3)]));
    }

    #[test]
    fn empty_directives_and_waiting_annotations() {
        let source = "# S\n\n::: anchor C1\n:::\n\n::: anchor D1\n:::\n\nNote.\n\n## Title\n\n\
                      | A |\n|---|\n| 1 |\n\n::: table\n:::\n";
        let range = &workbook(source).sheets[0].ranges[0];
        assert_eq!(
            (
                range.title_row,
                range.note_row,
                range.header_row,
                range.first_column
            ),
            (Some(1), Some(2), 3, 3)
        );
        assert_eq!(
            lint(source),
            diagnostics(&[("directive_misplaced", 3), ("directive_misplaced", 17)])
        );
    }

    #[test]
    fn default_sheet_exists_only_with_a_table_or_ignored_content() {
        let named = |source: &str| -> Vec<String> {
            workbook(source)
                .sheets
                .into_iter()
                .map(|s| s.name)
                .collect()
        };
        assert_eq!(
            named("---\ntitle: Book\n---\n\n***\n\n# Data\n"),
            vec!["Data"]
        );
        assert_eq!(named("Intro.\n\n# Data\n"), vec!["Sheet1", "Data"]);
        assert_eq!(
            named("---\ntitle: Book\n---\n\n| A |\n|---|\n| 1 |\n\n# Data\n"),
            vec!["Book", "Data"]
        );
        assert_eq!(
            lint("Intro.\n\n# Data\n"),
            diagnostics(&[("xlsx_ignored_content", 1)])
        );
    }

    #[test]
    fn empty_workbook_name_is_repaired_at_the_title_key() {
        assert_eq!(workbook("---\ntitle: Q1/Q2\n---\n").sheets[0].name, "Q1-Q2");
        // An H1 is a sheet even without a table; an empty one is named by its position.
        let source = "---\ntitle: Q1/Q2\n---\n\n# \n";
        assert_eq!(workbook(source).sheets[0].name, "Sheet1");
        assert_eq!(lint(source), diagnostics(&[("sheet_name_invalid", 5)]));
        assert_eq!(
            lint("---\ntitle: Q1/Q2\n---\n"),
            vec![
                ("xlsx_empty_workbook".to_string(), None),
                ("sheet_name_invalid".to_string(), Some(2)),
            ]
        );
    }

    #[test]
    fn excel_table_names() {
        let source = "# S\n\n::: table\n:::\n\n| A |\n|---|\n| 1 |\n\n\
                      ::: table name=Table1 style=plain\n:::\n\n| B |\n|---|\n| 1 |\n\n\
                      ::: table name=table1\n:::\n\n| C |  |\n|---|---|\n| 1 | 2 |\n";
        let tables: Vec<_> = workbook(source).sheets[0]
            .ranges
            .iter()
            .map(|r| r.excel_table.clone().unwrap())
            .collect();
        let names: Vec<_> = tables.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, vec!["Table2", "Table1", "Table3"]);
        assert_eq!(tables[1].style.excel_name(), "TableStyleLight1");
        assert_eq!(tables[2].headers, vec!["C", "Column2"]);
        assert_eq!(
            lint(source),
            diagnostics(&[("invalid_directive_args", 17), ("table_header_invalid", 17)])
        );
    }

    #[test]
    fn fragment_links_resolve_to_sheet_names() {
        let source = "# Sales/Q3\n\n| Link |\n|---|\n| [costs](#costs) |\n| [sales](#sales-q3) |\n\
                      | [none](#nope) |\n\n# Costs\n\n| A |\n|---|\n| 1 |\n";
        assert_eq!(
            lint(source),
            diagnostics(&[("sheet_name_invalid", 1), ("link_anchor_unresolved", 7)])
        );
        // A note is one (merged) cell, so it keeps one link too.
        let note = "# S\n\nSee [a](https://a.example) and [b](https://b.example).\n\n| A |\n|---|\n| 1 |\n";
        assert_eq!(lint(note), diagnostics(&[("xlsx_ignored_content", 3)]));
    }

    #[test]
    fn long_rows_widen_the_table_and_its_grid() {
        let source = "# S\n\n| A |\n|---|\n| 1 | =A2*2 |\n";
        let range = &workbook(source).sheets[0].ranges[0];
        assert_eq!(range.columns, 2);
        assert_eq!(range.formulas, vec![("B2".to_string(), "A2*2".to_string())]);
        assert_eq!(lint(source), diagnostics(&[("table_not_rectangular", 5)]));
    }

    #[test]
    fn cells_keep_one_link() {
        let source = "# S\n\n| A |\n|---|\n| [a](https://a.example) [b](https://b.example) |\n";
        assert_eq!(lint(source), diagnostics(&[("xlsx_ignored_content", 5)]));
    }

    #[test]
    fn qualified_references_reach_other_sheets() {
        let source = "| A |\n|---|\n| 1 |\n\n# Data\n\n| A |\n|---|\n| =Sheet1!A2+'sheet1'!A2 |\n";
        assert_eq!(
            workbook(source).sheets[1].ranges[0].formulas,
            vec![("A2".to_string(), "Sheet1!A2+'sheet1'!A2".to_string())]
        );
        assert_eq!(lint(source), vec![]);
    }

    #[test]
    fn formula_errors_are_text_and_not_listed() {
        let source = "# S\n\n| A | B |\n|---|---|\n| 1 | =SUM( |\n| 2 | =a2 |\n";
        let range = &workbook(source).sheets[0].ranges[0];
        assert_eq!(range.formulas, vec![("B3".to_string(), "A2".to_string())]);
        assert_eq!(range.column_types[1], ColumnType::General);
        assert_eq!(lint(source), diagnostics(&[("formula_error", 5)]));
    }

    #[test]
    fn grid_limits() {
        // The second table fits where it is anchored but overlaps the first, and two rows below
        // the lowest placed row is past the grid.
        let past_last_row = "# S\n\n::: anchor A1048575\n:::\n\n| A |\n|---|\n| 1 |\n\n\
                             ::: anchor A1048575\n:::\n\n| B |\n|---|\n| 1 |\n";
        assert_eq!(placements(past_last_row), vec![(1_048_575, 0)]);
        assert_eq!(
            lint(past_last_row),
            diagnostics(&[("xlsx_limit_exceeded", 13), ("xlsx_range_overlap", 13)])
        );

        let off_grid = "# S\n\n::: anchor A1048576\n:::\n\n| A |\n|---|\n| 1 |\n";
        assert_eq!(placements(off_grid), vec![(1, 0)]);
        assert_eq!(
            lint(off_grid),
            diagnostics(&[("invalid_directive_args", 3)])
        );

        let past_xfd = "# S\n\n::: anchor XFD1\n:::\n\n| A |\n|---|\n| =B1 |\n";
        assert!(workbook(past_xfd).sheets[0].ranges[0].formulas.is_empty());
        assert_eq!(
            lint(past_xfd),
            diagnostics(&[("formula_ref_out_of_range", 8), ("xlsx_limit_exceeded", 8)])
        );
    }

    #[test]
    fn ignored_directives_and_containers() {
        let source = "# S\n\n::: layout Title Only\n| A |\n|---|\n| 1 |\n:::\n\n::: notes\n| B |\n|---|\n| 1 |\n:::\n";
        assert_eq!(placements(source), vec![(1, 0)]);
        assert_eq!(lint(source), vec![]);
    }
}
