//! Composition (markdown-spec.md §9.2–§9.6) over the sheets the walk found: sheet names, directive
//! arguments, Excel table names, then each table's placement, values and formulas.
//!
//! Work is kept proportional to the input (§12.1): overlap checks, `columns` resolution and name
//! repairs never compare every item with every other.

use std::collections::{BTreeMap, HashMap, HashSet};

use super::directives::{self, Args, Columns};
use super::formula::{self, Context};
use super::grid::{self, MAX_COLUMNS, MAX_ROW};
use super::names::{self, fold, utf16_len};
use super::values::{self, ColumnType, Literal};
use super::walk::{self, Attached, Walk};
use super::{ExcelTable, Range, Sheet, Workbook, excerpt, line_of};
use crate::diagnostic::{Diagnostic, Sink};
use crate::ir::{Document, Run, Table, plain_text};
use crate::validate::{at, row_line};

/// Excel's limits on a cell's text and a formula's length, in UTF-16 code units.
const MAX_CELL_TEXT: usize = 32_767;
const MAX_FORMULA: usize = 8_192;

fn formula_too_long(address: &str, line: Option<usize>) -> Diagnostic {
    at(
        Diagnostic::new(
            "xlsx_limit_exceeded",
            format!("The formula in {address} is longer than Excel's 8,192 characters."),
        )
        .hint("Split the calculation across cells."),
        line,
    )
}

/// Composes the workbook for `document`, collecting every validate-stage diagnostic of the xlsx
/// profile except fragment links (which need the parse sidecar). `title_line` is the line of the
/// `title` frontmatter key, where diagnostics about a default sheet's name point.
pub fn compose(document: &Document, title_line: Option<usize>, out: &mut Sink) -> Workbook {
    let walked = walk::walk(&document.body, out);
    let default_name = document
        .meta
        .get("title")
        .and_then(|t| t.as_str())
        .unwrap_or("Sheet1")
        .to_string();

    if walked.sheets.is_empty() {
        out.push(
            Diagnostic::new(
                "xlsx_empty_workbook",
                "The document yields no sheets; the workbook gets one empty sheet.",
            )
            .hint("Add a table, under a `#` heading for each sheet."),
        );
        let name = names::sheet_name(&default_name, 1, &HashSet::new(), title_line, out);
        let sheets = vec![Sheet {
            name,
            ranges: Vec::new(),
        }];
        return Workbook { sheets };
    }

    let mut sheet_names: Vec<String> = Vec::new();
    let mut taken: HashSet<String> = HashSet::new();
    for (index, sheet) in walked.sheets.iter().enumerate() {
        let (raw, line) = match sheet.heading {
            Some(heading) => (plain_text(&heading.title), line_of(heading.position)),
            None => (default_name.clone(), title_line),
        };
        let name = names::sheet_name(&raw, index + 1, &taken, line, out);
        taken.insert(fold(&name));
        sheet_names.push(name);
    }
    let folded: Vec<String> = sheet_names.iter().map(|n| fold(n)).collect();

    // Arguments are checked once per directive that took effect; a misplaced one is ignored.
    let args: Vec<Option<Args>> = walked
        .directives
        .iter()
        .zip(&walked.applied)
        .map(|(d, applied)| applied.then(|| directives::parse(d, out)))
        .collect();
    let table_names = excel_table_names(&walked, &args, out);

    let mut composer = Composer {
        walked: &walked,
        args: &args,
        table_names: &table_names,
        sheets: &folded,
        matched: HashMap::new(),
        repeated: HashSet::new(),
        out,
    };
    let mut sheets = Vec::with_capacity(walked.sheets.len());
    for (sheet, name) in walked.sheets.iter().zip(sheet_names) {
        let mut placer = Placer::new();
        let ranges = sheet
            .tables
            .iter()
            .filter_map(|attached| composer.table(attached, &mut placer))
            .collect();
        sheets.push(Sheet { name, ranges });
    }
    composer.unmatched_columns();
    Workbook { sheets }
}

/// §9.6: explicit names first, in workbook order (a repeat is invalid), then automatic names
/// `Table1`, `Table2`, … for the rest, skipping names already used. Keyed by directive.
fn excel_table_names(
    walked: &Walk<'_>,
    args: &[Option<Args>],
    out: &mut Sink,
) -> HashMap<usize, String> {
    let ids: Vec<usize> = walked
        .sheets
        .iter()
        .flat_map(|s| &s.tables)
        .filter_map(|t| t.excel)
        .collect();
    let mut used = HashSet::new();
    let mut names = HashMap::new();
    for &id in &ids {
        if let Some(Args::Table(options)) = &args[id]
            && let Some(name) = &options.name
        {
            if used.insert(name.to_ascii_lowercase()) {
                names.insert(id, name.clone());
            } else {
                out.push(at(
                    Diagnostic::new(
                        "invalid_directive_args",
                        format!("Table name `{name}` is already used in this workbook; this table gets an automatic name."),
                    )
                    .hint("Give every `::: table` a distinct name (names are compared case-insensitively)."),
                    line_of(walked.directives[id].position),
                ));
            }
        }
    }
    let mut next = 1;
    for &id in &ids {
        if names.contains_key(&id) {
            continue;
        }
        let name = loop {
            let candidate = format!("Table{next}");
            next += 1;
            if !used.contains(&candidate.to_ascii_lowercase()) {
                break candidate;
            }
        };
        used.insert(name.to_ascii_lowercase());
        names.insert(id, name);
    }
    names
}

/// A sheet's stacking cursor and the ranges placed on it (§9.2). Rows are 1-based, columns 0-based;
/// ranges are `[top, left, bottom, right]`, inclusive, and never overlap one another.
///
/// Ranges placed in the flow (stacked at the cursor, or moved below everything after an overlap)
/// each start below every earlier flow range, since the cursor only moves down: they form a list in
/// row order, row-disjoint. Ranges placed at their anchor are indexed per column, as row intervals
/// (disjoint within a column). Either check costs about the candidate's height or width.
struct Placer {
    cursor: u64,
    lowest: u64,
    flow: Vec<[u64; 4]>,
    /// Column → top → bottom.
    anchored: HashMap<u64, BTreeMap<u64, u64>>,
}

impl Placer {
    fn new() -> Self {
        Self {
            cursor: 1,
            lowest: 0,
            flow: Vec::new(),
            anchored: HashMap::new(),
        }
    }

    fn overlaps(&self, [top, left, bottom, right]: [u64; 4]) -> bool {
        // Flow ranges starting at or above `bottom`; scanning up, those still reaching `top`.
        let end = self.flow.partition_point(|range| range[0] <= bottom);
        let in_flow = self.flow[..end]
            .iter()
            .rev()
            .take_while(|range| range[2] >= top)
            .any(|range| left <= range[3] && range[1] <= right);
        in_flow
            || (left..=right).any(|column| {
                self.anchored.get(&column).is_some_and(|intervals| {
                    intervals
                        .range(..=bottom)
                        .next_back()
                        .is_some_and(|(_, &end)| end >= top)
                })
            })
    }

    fn place(&mut self, range: [u64; 4], anchored: bool) {
        let [top, left, bottom, right] = range;
        if anchored {
            for column in left..=right {
                self.anchored.entry(column).or_default().insert(top, bottom);
            }
        } else {
            self.flow.push(range);
        }
        self.lowest = self.lowest.max(bottom);
    }
}

struct Composer<'w, 'o> {
    walked: &'w Walk<'w>,
    args: &'w [Option<Args>],
    table_names: &'w HashMap<usize, String>,
    /// The workbook's sheet names, case-folded.
    sheets: &'w [String],
    /// Per `columns` directive, which entries named a column of some table it applies to.
    matched: HashMap<usize, Vec<bool>>,
    /// `(directive, entry)` pairs already reported for naming a column an earlier entry named.
    repeated: HashSet<(usize, usize)>,
    out: &'o mut Sink,
}

impl Composer<'_, '_> {
    /// A `columns` entry whose key matches no column in any table the directive applies to is
    /// ignored, reported once at the directive.
    fn unmatched_columns(&mut self) {
        let mut ids: Vec<&usize> = self.matched.keys().collect();
        ids.sort_unstable();
        for &id in ids {
            let Some(Args::Columns(columns)) = &self.args[id] else {
                continue;
            };
            for (entry, matched) in columns.entries.iter().zip(&self.matched[&id]) {
                if !matched {
                    self.out.push(at(
                        Diagnostic::new(
                            "unknown_directive_option",
                            format!("`columns` key `{}` matches no header or column of the table; the entry is ignored.", entry.key),
                        )
                        .hint("Use a header's text, or a column letter counted from the table's first column (A)."),
                        line_of(self.walked.directives[id].position),
                    ));
                }
            }
        }
    }

    /// The column types a `columns` directive gives one table.
    fn column_types(
        &mut self,
        id: usize,
        columns: &Columns,
        headers: &[String],
    ) -> Vec<Option<ColumnType>> {
        let resolved = columns.resolve(headers);
        let matched = self
            .matched
            .entry(id)
            .or_insert_with(|| vec![false; columns.entries.len()]);
        for &entry in &resolved.matched {
            matched[entry] = true;
        }
        for &(entry, column) in &resolved.repeated {
            if self.repeated.insert((id, entry)) {
                self.out.push(at(
                    Diagnostic::new(
                        "invalid_directive_args",
                        format!(
                            "`columns` entry `{}` names the column `{}`, which an earlier entry already names; the first entry is used.",
                            columns.entries[entry].key, headers[column]
                        ),
                    )
                    .hint("Give each column one entry."),
                    line_of(self.walked.directives[id].position),
                ));
            }
        }
        resolved.typed
    }

    /// Places one table and composes its cells. `None` when it can't be placed on the grid.
    fn table(&mut self, attached: &Attached<'_>, placer: &mut Placer) -> Option<Range> {
        let table = attached.table;
        let line = line_of(table.position);
        let columns = table
            .rows
            .iter()
            .map(Vec::len)
            .chain([table.header.len(), 1])
            .max()
            .unwrap_or(1) as u64;
        let data_rows = table.rows.len() as u64;
        let above = u64::from(attached.title.is_some()) + u64::from(attached.note.is_some());
        let height = above + 1 + data_rows;

        // §9.2 steps 1 and 5, the off-grid rule, then the overlap rule.
        let mut anchor = attached.anchor.and_then(|id| match &self.args[id] {
            Some(Args::Anchor(cell)) => cell.map(|c| (id, c)),
            _ => None,
        });
        if let Some((id, cell)) = anchor {
            let bottom = u64::from(cell.row) + height - 1;
            let right = u64::from(cell.column) + columns - 1;
            if bottom > u64::from(MAX_ROW) || right >= u64::from(MAX_COLUMNS) {
                let edge = if right >= u64::from(MAX_COLUMNS) {
                    "column XFD"
                } else {
                    "row 1,048,576"
                };
                self.out.push(at(
                    Diagnostic::new(
                        "invalid_directive_args",
                        format!("`::: anchor {cell}` would place this table past {edge}; the anchor is ignored and the table is stacked."),
                    )
                    .hint("Anchor the table where all of it fits on the sheet."),
                    line_of(self.walked.directives[id].position),
                ));
                anchor = None;
            }
        }
        let (mut top, mut left) = match anchor {
            Some((_, cell)) => (u64::from(cell.row), u64::from(cell.column)),
            None => (placer.cursor, 0),
        };
        let mut moves_cursor = anchor.is_none();
        if placer.overlaps([top, left, top + height - 1, left + columns - 1]) {
            let moved = placer.lowest + 2;
            self.out.push(at(
                Diagnostic::new(
                    "xlsx_range_overlap",
                    format!(
                        "This table's range would overlap an earlier one on the sheet; it is placed at A{moved} instead."
                    ),
                )
                .hint("Anchor tables where their ranges don't overlap."),
                line,
            ));
            (top, left, moves_cursor) = (moved, 0, true);
        }
        let header_row = top + above;
        let last_row = header_row + data_rows;
        if moves_cursor {
            placer.cursor = last_row + 2;
        }
        if last_row > u64::from(MAX_ROW) {
            let past = if header_row > u64::from(MAX_ROW) {
                line
            } else {
                table
                    .position
                    .map(|p| row_line(p, (u64::from(MAX_ROW) - header_row) as usize))
            };
            self.out.push(at(
                Diagnostic::new(
                    "xlsx_limit_exceeded",
                    "This table would extend past row 1,048,576, Excel's last row.",
                )
                .hint("Split the data across sheets."),
                past,
            ));
            return None;
        }
        placer.place([top, left, last_row, left + columns - 1], !moves_cursor);

        // Every value below fits the grid, so u32 is exact.
        let (top, left, header_row) = (top as u32, left as u32, header_row as u32);
        let columns = columns as usize;
        let title_row = attached.title.map(|_| top);
        let note_row = attached
            .note
            .map(|_| top + u32::from(attached.title.is_some()));
        if let (Some(title), Some(row)) = (attached.title, title_row) {
            self.text_cell(
                &title.title,
                line_of(title.position),
                grid::address(left, row),
            );
        }
        if let (Some(note), Some(row)) = (attached.note, note_row) {
            self.text_cell(&note.runs, line_of(note.position), grid::address(left, row));
        }
        let headers: Vec<String> = (0..columns)
            .map(|c| {
                table
                    .header
                    .get(c)
                    .map(|cell| plain_text(&cell.runs).trim().to_string())
                    .unwrap_or_default()
            })
            .collect();
        for (c, cell) in table.header.iter().enumerate() {
            self.text_cell(&cell.runs, line, grid::address(left + c as u32, header_row));
        }

        let args = self.args;
        let typed: Vec<Option<ColumnType>> = match attached.columns.map(|id| (id, &args[id])) {
            Some((id, Some(Args::Columns(columns)))) => self.column_types(id, columns, &headers),
            _ => vec![None; columns],
        };
        let (column_types, formulas) = self.cells(table, &typed, left, header_row, columns as u32);

        let excel_table = attached.excel.and_then(|id| {
            let Some(Args::Table(options)) = &args[id] else {
                return None;
            };
            let repaired = names::repair_headers(&headers);
            let renamed: Vec<String> = headers
                .iter()
                .zip(&repaired)
                .filter(|(before, after)| before != after)
                .map(|(before, after)| format!("`{before}` → `{after}`"))
                .take(5)
                .collect();
            if !renamed.is_empty() {
                self.out.push(at(
                    Diagnostic::new(
                        "table_header_invalid",
                        format!(
                            "An Excel table needs non-empty, distinct headers (compared case-insensitively); renamed: {}.",
                            renamed.join(", ")
                        ),
                    )
                    .hint("Give every header cell a distinct, non-empty name."),
                    line_of(self.walked.directives[id].position),
                ));
            }
            Some(ExcelTable {
                name: self.table_names[&id].clone(),
                style: options.style,
                autofilter: options.autofilter,
                headers: repaired,
            })
        });

        Some(Range {
            title_row,
            note_row,
            header_row,
            first_column: left,
            columns: columns as u32,
            last_row: last_row as u32,
            column_types,
            formulas,
            excel_table,
        })
    }

    /// The data cells (§9.3, §9.5): formulas checked and translated; literals tested against their
    /// column's type, or collected for inference. Returns the column types and the translated
    /// formulas keyed by placed address.
    fn cells(
        &mut self,
        table: &Table,
        typed: &[Option<ColumnType>],
        left: u32,
        header_row: u32,
        columns: u32,
    ) -> (Vec<ColumnType>, Vec<(String, String)>) {
        let context = Context {
            first_column: left,
            header_row,
            columns,
            data_rows: table.rows.len() as u32,
            sheets: self.sheets,
        };
        let mut literals: Vec<Vec<Literal>> = vec![Vec::new(); typed.len()];
        // The decimals a percent format needs per column (§9.4).
        let mut decimals = vec![0u8; typed.len()];
        let mut formulas = Vec::new();
        for (r, row) in table.rows.iter().enumerate() {
            let line = table.position.map(|p| row_line(p, r));
            for (c, cell) in row.iter().enumerate() {
                let address = grid::address(left + c as u32, header_row + 1 + r as u32);
                let text = plain_text(&cell.runs);
                self.links(&cell.runs, line, &address);
                if let Some(source) = cell.raw.strip_prefix('=') {
                    // Checked before lexing too, so a formula of any size costs little (§12.1).
                    if utf16_len(source) > MAX_FORMULA {
                        self.out.push(formula_too_long(&address, line));
                        continue;
                    }
                    match formula::translate(source, &context) {
                        Ok(formula) => {
                            self.formula_problems(&formula, line, &address);
                            if formula.past_grid.is_empty()
                                && utf16_len(&formula.written) <= MAX_FORMULA
                            {
                                formulas.push((address, formula.written));
                            }
                        }
                        Err(error) => {
                            self.out.push(at(
                                Diagnostic::new(
                                    "formula_error",
                                    format!(
                                        "The formula in {address} {}; the cell is written as text.",
                                        error.reason
                                    ),
                                )
                                .hint(error.hint),
                                line,
                            ));
                            self.length(&text, line, &address);
                        }
                    }
                    continue;
                }
                self.length(&text, line, &address);
                if text.is_empty() {
                    continue;
                }
                let literal = values::classify(&text);
                match &typed[c] {
                    Some(kind) if !kind.accepts(literal) => self.out.push(at(
                        Diagnostic::new(
                            "typed_value_invalid",
                            format!("`{}` in {address} isn't a valid {} value; it is written as text.", excerpt(&text), kind.name()),
                        )
                        .hint(match kind {
                            ColumnType::Date => "Write dates as YYYY-MM-DD.",
                            ColumnType::Boolean => "Write true or false.",
                            ColumnType::Percent { .. } => "Write a plain number (0.14) or a percent (14%), without grouping.",
                            _ => "Write a plain number such as 120000 or -12.5, without symbols or grouping.",
                        }),
                        line,
                    )),
                    Some(_) => {}
                    None => literals[c].push(literal),
                }
                decimals[c] = decimals[c].max(values::percent_decimals(&text, literal));
            }
        }
        let column_types = typed
            .iter()
            .zip(literals)
            .enumerate()
            .map(|(c, (typed, literals))| match typed {
                Some(kind) => kind.clone(),
                None => {
                    let (kind, mixed) = values::infer(&literals);
                    if let Some(kinds) = mixed {
                        let letter = grid::column_letters(left + c as u32);
                        self.out.push(at(
                            Diagnostic::new(
                                "mixed_column_type",
                                format!("Column {letter} mixes {kinds}; each cell keeps its own type."),
                            )
                            .hint(format!(
                                "If the column has one type, make the values agree or set it with `::: columns {}:<type>`.",
                                grid::column_letters(c as u32)
                            )),
                            table.position.map(|p| p.line),
                        ));
                    }
                    kind
                }
            })
            .zip(decimals)
            .map(|(kind, needed)| match kind {
                ColumnType::Percent { decimals: None } => ColumnType::Percent {
                    decimals: Some(needed),
                },
                kind => kind,
            })
            .collect();
        (column_types, formulas)
    }

    fn formula_problems(&mut self, formula: &formula::Formula, line: Option<usize>, address: &str) {
        for reference in &formula.out_of_range {
            self.out.push(at(
                Diagnostic::new(
                    "formula_ref_out_of_range",
                    format!("The formula in {address} refers to `{reference}`, outside its table; it is still translated."),
                )
                .hint("References are table-relative: the header row is row 1 and the table's first column is A. Use a sheet-qualified reference (Sheet!B5) for other cells."),
                line,
            ));
        }
        for reference in &formula.past_grid {
            self.out.push(at(
                Diagnostic::new(
                    "xlsx_limit_exceeded",
                    format!("The formula in {address} refers to `{reference}`, which translates past the sheet's last row or column."),
                )
                .hint("Move the table, or fix the reference."),
                line,
            ));
        }
        if formula.past_grid.is_empty() && utf16_len(&formula.written) > MAX_FORMULA {
            self.out.push(formula_too_long(address, line));
        }
    }

    /// A text cell: a range title, note or header cell, checked for length and links.
    fn text_cell(&mut self, runs: &[Run], line: Option<usize>, address: String) {
        self.length(&plain_text(runs), line, &address);
        self.links(runs, line, &address);
    }

    fn length(&mut self, text: &str, line: Option<usize>, address: &str) {
        if utf16_len(text) > MAX_CELL_TEXT {
            self.out.push(at(
                Diagnostic::new(
                    "xlsx_limit_exceeded",
                    format!("The text of {address} is longer than Excel's 32,767 characters."),
                )
                .hint("Shorten the text, or split it across cells."),
                line,
            ));
        }
    }

    /// §9.5: a cell gets one hyperlink; further distinct links are ignored.
    fn links(&mut self, runs: &[Run], line: Option<usize>, address: &str) {
        let mut links: Vec<&str> = Vec::new();
        for link in runs.iter().filter_map(|r| r.link.as_deref()) {
            if !links.contains(&link) {
                links.push(link);
            }
        }
        if links.len() > 1 {
            self.out.push(at(
                Diagnostic::new(
                    "xlsx_ignored_content",
                    format!(
                        "{address} has {} links; a cell keeps only its first (`{}`).",
                        links.len(),
                        links[0]
                    ),
                )
                .hint("Put each link in its own cell."),
                line,
            ));
        }
    }
}
