//! The xlsx renderer (markdown-spec.md §9): writes a composed workbook as SpreadsheetML. Every
//! decision — placement, values, number formats, formulas, links, Excel tables — was made when the
//! workbook was composed, so lint and convert can't disagree; this crate only serializes.
//!
//! Styles are assigned in the order cells first need them, strings are shared in the order they
//! first appear, and parts are written in a fixed order, so the bytes are reproducible (§13).

use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};

use compyla_core::ir::Run;
use compyla_core::validate::xlsx::{Cell, CellValue, Link, Role, Workbook, builtin_format};
use compyla_ooxml::package::{self, HYPERLINK, Package, Properties, Relationships};
use compyla_ooxml::xml::Writer;

const MAIN: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const RELATIONSHIP_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

const WORKBOOK_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml";
const WORKSHEET_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml";
const STYLES_TYPE: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml";
const STRINGS_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml";
const TABLE_TYPE: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.table+xml";

/// The header row's fill in the house style (§9.2).
const HEADER_FILL: &str = "FFE8ECF3";
/// The color of a hyperlink's text.
const LINK_COLOR: &str = "FF0563C1";

/// Renders `workbook` as xlsx bytes, with the document properties from `meta` (C44).
pub fn render(workbook: &Workbook, properties: &Properties) -> Vec<u8> {
    let mut styles = Styles::default();
    let mut strings = Strings::default();
    let mut package = Package::new();
    package::add_package_parts(&mut package, "xl/workbook.xml", properties);

    let mut workbook_rels = Relationships::new();
    let mut sheet_ids = Vec::new();
    let mut sheet_parts = Vec::new();
    let mut table_number = 0;
    for (index, sheet) in workbook.sheets.iter().enumerate() {
        let part = format!("xl/worksheets/sheet{}.xml", index + 1);
        sheet_ids.push(workbook_rels.add(
            "http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet",
            &format!("worksheets/sheet{}.xml", index + 1),
        ));

        let mut rels = Relationships::new();
        let mut tables = Vec::new();
        let mut table_parts = Vec::new();
        for range in &sheet.ranges {
            if let Some(excel) = &range.excel_table {
                table_number += 1;
                let id = rels.add(
                    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/table",
                    &format!("../tables/table{table_number}.xml"),
                );
                tables.push(id);
                let last_column = range.first_column + range.columns - 1;
                let reference = format!(
                    "{}:{}",
                    address(range.first_column, range.header_row),
                    address(last_column, range.last_row)
                );
                let xml = table_xml(table_number, excel, &reference);
                table_parts.push((
                    format!("xl/tables/table{table_number}.xml"),
                    TABLE_TYPE,
                    xml,
                ));
            }
        }
        // Hyperlink relationships follow the tables'.
        let xml = worksheet(sheet, &mut styles, &mut strings, &mut rels, &tables);
        let rels_part = rels_name(&part);
        sheet_parts.push((part, WORKSHEET_TYPE, xml));
        if !rels.is_empty() {
            sheet_parts.push((rels_part, package::RELATIONSHIPS, rels.to_xml()));
        }
        sheet_parts.extend(table_parts);
    }
    workbook_rels.add(
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles",
        "styles.xml",
    );
    workbook_rels.add(
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/sharedStrings",
        "sharedStrings.xml",
    );

    package.add(
        "xl/workbook.xml",
        WORKBOOK_TYPE,
        workbook_xml(workbook, &sheet_ids),
    );
    package.add_relationships("xl/workbook.xml", &workbook_rels);
    package.add("xl/styles.xml", STYLES_TYPE, styles.to_xml());
    package.add("xl/sharedStrings.xml", STRINGS_TYPE, strings.to_xml());
    for (name, content_type, data) in sheet_parts {
        package.add(name, content_type, data);
    }
    package.to_bytes()
}

fn rels_name(part: &str) -> String {
    let (folder, file) = part.rsplit_once('/').unwrap_or(("", part));
    format!("{folder}/_rels/{file}.rels")
}

fn workbook_xml(workbook: &Workbook, sheet_ids: &[String]) -> Vec<u8> {
    let mut xml = Writer::new();
    xml.open("workbook", &[("xmlns", MAIN), ("xmlns:r", RELATIONSHIP_NS)]);
    xml.open("sheets", &[]);
    for (index, (sheet, id)) in workbook.sheets.iter().zip(sheet_ids).enumerate() {
        let number = (index + 1).to_string();
        xml.empty(
            "sheet",
            &[
                ("name", &xstring(&sheet.name)),
                ("sheetId", &number),
                ("r:id", id),
            ],
        );
    }
    xml.close("sheets");
    // Formula results aren't cached in the file: recalculate on open (§9.5).
    xml.empty("calcPr", &[("calcId", "191029"), ("fullCalcOnLoad", "1")]);
    xml.close("workbook");
    xml.finish()
}

fn worksheet(
    sheet: &compyla_core::validate::xlsx::Sheet,
    styles: &mut Styles,
    strings: &mut Strings,
    rels: &mut Relationships,
    tables: &[String],
) -> Vec<u8> {
    // Ranges placed side by side share rows; Excel wants rows, and cells in a row, in order.
    let mut cells: BTreeMap<(u32, u32), &Cell> = BTreeMap::new();
    let mut merges = Vec::new();
    for range in &sheet.ranges {
        for cell in &range.cells {
            cells.insert((cell.row, cell.column), cell);
        }
        if range.columns > 1 {
            let last = range.first_column + range.columns - 1;
            for row in [range.title_row, range.note_row].into_iter().flatten() {
                merges.push(format!(
                    "{}:{}",
                    address(range.first_column, row),
                    address(last, row)
                ));
            }
        }
    }

    let mut xml = Writer::new();
    xml.open(
        "worksheet",
        &[("xmlns", MAIN), ("xmlns:r", RELATIONSHIP_NS)],
    );
    let dimension = match (cells.keys().next(), cells.keys().next_back()) {
        (Some(_), Some(_)) => {
            let (top, left, bottom, right) = cells.keys().fold(
                (u32::MAX, u32::MAX, 0, 0),
                |(t, l, b, r), &(row, column)| {
                    (t.min(row), l.min(column), b.max(row), r.max(column))
                },
            );
            format!("{}:{}", address(left, top), address(right, bottom))
        }
        _ => "A1".to_string(),
    };
    xml.empty("dimension", &[("ref", &dimension)]);
    xml.empty("sheetFormatPr", &[("defaultRowHeight", "15")]);
    xml.open("sheetData", &[]);
    let mut hyperlinks = Vec::new();
    let mut current_row = None;
    for (&(row, column), cell) in &cells {
        if current_row != Some(row) {
            if current_row.is_some() {
                xml.close("row");
            }
            xml.open("row", &[("r", &row.to_string())]);
            current_row = Some(row);
        }
        let style = styles.index(&StyleKey::of(cell));
        let reference = address(column, row);
        let style = style.to_string();
        let mut attrs = vec![("r", reference.as_str())];
        if style != "0" {
            attrs.push(("s", style.as_str()));
        }
        match &cell.value {
            CellValue::Empty => xml.empty("c", &attrs),
            CellValue::Text(runs) => {
                let index = strings.index(runs, &StyleKey::of(cell).font).to_string();
                attrs.push(("t", "s"));
                xml.open("c", &attrs);
                xml.element("v", &[], &index);
                xml.close("c");
            }
            CellValue::Number(number) => {
                xml.open("c", &attrs);
                xml.element("v", &[], number);
                xml.close("c");
            }
            CellValue::Boolean(value) => {
                attrs.push(("t", "b"));
                xml.open("c", &attrs);
                xml.element("v", &[], if *value { "1" } else { "0" });
                xml.close("c");
            }
            CellValue::Formula(formula) => {
                xml.open("c", &attrs);
                xml.element("f", &[], &xstring(formula));
                xml.close("c");
            }
        }
        if let Some(link) = &cell.link {
            hyperlinks.push((reference, link));
        }
    }
    if current_row.is_some() {
        xml.close("row");
    }
    xml.close("sheetData");
    if !merges.is_empty() {
        xml.open("mergeCells", &[("count", &merges.len().to_string())]);
        for merge in &merges {
            xml.empty("mergeCell", &[("ref", merge)]);
        }
        xml.close("mergeCells");
    }
    if !hyperlinks.is_empty() {
        xml.open("hyperlinks", &[]);
        for (reference, link) in hyperlinks {
            match link {
                Link::Url(url) => {
                    let id = rels.add_external(HYPERLINK, &package::uri(url));
                    xml.empty("hyperlink", &[("ref", &reference), ("r:id", &id)]);
                }
                Link::Sheet(name) => {
                    let location = format!("'{}'!A1", name.replace('\'', "''"));
                    let location = xstring(&location);
                    xml.empty("hyperlink", &[("ref", &reference), ("location", &location)]);
                }
            }
        }
        xml.close("hyperlinks");
    }
    xml.empty(
        "pageMargins",
        &[
            ("left", "0.7"),
            ("right", "0.7"),
            ("top", "0.75"),
            ("bottom", "0.75"),
            ("header", "0.3"),
            ("footer", "0.3"),
        ],
    );
    if !tables.is_empty() {
        xml.open("tableParts", &[("count", &tables.len().to_string())]);
        for id in tables {
            xml.empty("tablePart", &[("r:id", id)]);
        }
        xml.close("tableParts");
    }
    xml.close("worksheet");
    xml.finish()
}

fn table_xml(
    number: usize,
    excel: &compyla_core::validate::xlsx::ExcelTable,
    reference: &str,
) -> Vec<u8> {
    let mut xml = Writer::new();
    let id = number.to_string();
    xml.open(
        "table",
        &[
            ("xmlns", MAIN),
            ("id", &id),
            ("name", &xstring(&excel.name)),
            ("displayName", &xstring(&excel.name)),
            ("ref", reference),
            ("totalsRowShown", "0"),
        ],
    );
    if excel.autofilter {
        xml.empty("autoFilter", &[("ref", reference)]);
    }
    xml.open(
        "tableColumns",
        &[("count", &excel.headers.len().to_string())],
    );
    for (index, header) in excel.headers.iter().enumerate() {
        xml.empty(
            "tableColumn",
            &[("id", &(index + 1).to_string()), ("name", &xstring(header))],
        );
    }
    xml.close("tableColumns");
    xml.empty(
        "tableStyleInfo",
        &[
            ("name", excel.style.excel_name()),
            ("showFirstColumn", "0"),
            ("showLastColumn", "0"),
            ("showRowStripes", "1"),
            ("showColumnStripes", "0"),
        ],
    );
    xml.close("table");
    xml.finish()
}

/// `A1` notation for a 0-based column and a 1-based row.
fn address(column: u32, row: u32) -> String {
    let mut letters = Vec::new();
    let mut n = column + 1;
    while n > 0 {
        let rem = (n - 1) % 26;
        letters.push(b'A' + rem as u8);
        n = (n - 1) / 26;
    }
    letters.reverse();
    format!("{}{row}", String::from_utf8(letters).unwrap_or_default())
}

// ---------------------------------------------------------------------------------------------
// Styles: number formats, fonts and fills, combined into cell formats in first-use order.

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Font {
    bold: bool,
    italic: bool,
    strike: bool,
    link: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct StyleKey {
    format: String,
    font: Font,
    header_fill: bool,
    wrap: bool,
}

impl StyleKey {
    /// The house style (§9.2): a bold title, an italic note, and a bold header row on a light fill,
    /// except in an Excel table, whose style formats its header.
    fn of(cell: &Cell) -> StyleKey {
        let house_header = cell.role == Role::Header;
        let wrap = matches!(&cell.value, CellValue::Text(runs) if runs.iter().any(|r| r.text.contains('\n')));
        StyleKey {
            format: cell.format.clone(),
            font: Font {
                bold: cell.role == Role::Title || house_header || cell.marks.bold,
                italic: cell.role == Role::Note || cell.marks.italic,
                strike: cell.marks.strike,
                link: cell.link.is_some(),
            },
            header_fill: house_header,
            wrap,
        }
    }
}

struct Styles {
    keys: Vec<StyleKey>,
    by_key: HashMap<StyleKey, usize>,
    fonts: Vec<Font>,
    font_ids: HashMap<Font, usize>,
    formats: Vec<String>,
    format_ids: HashMap<String, u32>,
}

impl Default for Styles {
    fn default() -> Self {
        let plain = StyleKey {
            format: "General".into(),
            font: Font {
                bold: false,
                italic: false,
                strike: false,
                link: false,
            },
            header_fill: false,
            wrap: false,
        };
        let mut styles = Styles {
            keys: Vec::new(),
            by_key: HashMap::new(),
            fonts: Vec::new(),
            font_ids: HashMap::new(),
            formats: Vec::new(),
            format_ids: HashMap::new(),
        };
        styles.index(&plain);
        styles
    }
}

impl Styles {
    fn index(&mut self, key: &StyleKey) -> usize {
        if let Some(&index) = self.by_key.get(key) {
            return index;
        }
        if !self.font_ids.contains_key(&key.font) {
            self.font_ids.insert(key.font.clone(), self.fonts.len());
            self.fonts.push(key.font.clone());
        }
        if builtin_format(&key.format).is_none() && !self.format_ids.contains_key(&key.format) {
            let id = 164 + self.formats.len() as u32;
            self.format_ids.insert(key.format.clone(), id);
            self.formats.push(key.format.clone());
        }
        self.keys.push(key.clone());
        self.by_key.insert(key.clone(), self.keys.len() - 1);
        self.keys.len() - 1
    }

    fn format_id(&self, code: &str) -> u32 {
        builtin_format(code).unwrap_or_else(|| self.format_ids[code])
    }

    fn to_xml(&self) -> Vec<u8> {
        let mut xml = Writer::new();
        xml.open("styleSheet", &[("xmlns", MAIN)]);
        if !self.formats.is_empty() {
            xml.open("numFmts", &[("count", &self.formats.len().to_string())]);
            for (index, code) in self.formats.iter().enumerate() {
                xml.empty(
                    "numFmt",
                    &[
                        ("numFmtId", &(164 + index).to_string()),
                        ("formatCode", code),
                    ],
                );
            }
            xml.close("numFmts");
        }
        xml.open("fonts", &[("count", &self.fonts.len().to_string())]);
        for font in &self.fonts {
            xml.open("font", &[]);
            if font.bold {
                xml.empty("b", &[]);
            }
            if font.italic {
                xml.empty("i", &[]);
            }
            if font.strike {
                xml.empty("strike", &[]);
            }
            if font.link {
                xml.empty("u", &[]);
            }
            xml.empty("sz", &[("val", "11")]);
            if font.link {
                xml.empty("color", &[("rgb", LINK_COLOR)]);
            }
            xml.empty("name", &[("val", "Calibri")]);
            xml.empty("family", &[("val", "2")]);
            xml.close("font");
        }
        xml.close("fonts");
        xml.open("fills", &[("count", "3")]);
        for pattern in ["none", "gray125"] {
            xml.open("fill", &[]);
            xml.empty("patternFill", &[("patternType", pattern)]);
            xml.close("fill");
        }
        xml.open("fill", &[]);
        xml.open("patternFill", &[("patternType", "solid")]);
        xml.empty("fgColor", &[("rgb", HEADER_FILL)]);
        xml.empty("bgColor", &[("indexed", "64")]);
        xml.close("patternFill");
        xml.close("fill");
        xml.close("fills");
        xml.open("borders", &[("count", "1")]);
        xml.open("border", &[]);
        for side in ["left", "right", "top", "bottom", "diagonal"] {
            xml.empty(side, &[]);
        }
        xml.close("border");
        xml.close("borders");
        xml.open("cellStyleXfs", &[("count", "1")]);
        xml.empty(
            "xf",
            &[
                ("numFmtId", "0"),
                ("fontId", "0"),
                ("fillId", "0"),
                ("borderId", "0"),
            ],
        );
        xml.close("cellStyleXfs");
        xml.open("cellXfs", &[("count", &self.keys.len().to_string())]);
        for key in &self.keys {
            let format = self.format_id(&key.format).to_string();
            let font = self.font_ids[&key.font].to_string();
            let fill = if key.header_fill { "2" } else { "0" };
            let mut attrs = vec![
                ("numFmtId", format.as_str()),
                ("fontId", font.as_str()),
                ("fillId", fill),
                ("borderId", "0"),
                ("xfId", "0"),
            ];
            if format != "0" {
                attrs.push(("applyNumberFormat", "1"));
            }
            if font != "0" {
                attrs.push(("applyFont", "1"));
            }
            if key.header_fill {
                attrs.push(("applyFill", "1"));
            }
            if key.wrap {
                attrs.push(("applyAlignment", "1"));
                xml.open("xf", &attrs);
                xml.empty("alignment", &[("wrapText", "1")]);
                xml.close("xf");
            } else {
                xml.empty("xf", &attrs);
            }
        }
        xml.close("cellXfs");
        xml.open("cellStyles", &[("count", "1")]);
        xml.empty(
            "cellStyle",
            &[("name", "Normal"), ("xfId", "0"), ("builtinId", "0")],
        );
        xml.close("cellStyles");
        xml.close("styleSheet");
        xml.finish()
    }
}

// ---------------------------------------------------------------------------------------------
// Shared strings: one entry per distinct text. Rich text keeps bold, italic and strike (§9.5); since
// a run's properties replace the cell's font, every run of a rich string carries the cell's font
// too (a bold title, an italic note, a link's underline and color).

/// A run as written: its text, then bold, italic, strike and link styling.
type RichRun = (String, bool, bool, bool, bool);

#[derive(Default)]
struct Strings {
    items: Vec<Vec<RichRun>>,
    by_text: HashMap<Vec<RichRun>, usize>,
    /// String cells written, for `count`.
    references: usize,
}

impl Strings {
    fn index(&mut self, runs: &[Run], cell: &Font) -> usize {
        self.references += 1;
        let marked = runs.iter().any(|r| r.bold || r.italic || r.strike);
        let mut rich: Vec<RichRun> = Vec::new();
        for run in runs {
            let marks = if marked {
                (
                    run.bold || cell.bold,
                    run.italic || cell.italic,
                    run.strike || cell.strike,
                    cell.link,
                )
            } else {
                (false, false, false, false)
            };
            match rich.last_mut() {
                Some(last) if (last.1, last.2, last.3, last.4) == marks => {
                    last.0.push_str(&run.text);
                }
                _ => rich.push((run.text.clone(), marks.0, marks.1, marks.2, marks.3)),
            }
        }
        if let Some(&index) = self.by_text.get(&rich) {
            return index;
        }
        self.items.push(rich.clone());
        self.by_text.insert(rich, self.items.len() - 1);
        self.items.len() - 1
    }

    fn to_xml(&self) -> Vec<u8> {
        let mut xml = Writer::new();
        let (count, unique) = (self.references.to_string(), self.items.len().to_string());
        xml.open(
            "sst",
            &[("xmlns", MAIN), ("count", &count), ("uniqueCount", &unique)],
        );
        for item in &self.items {
            xml.open("si", &[]);
            if item.iter().all(|&(_, b, i, s, l)| !(b || i || s || l)) {
                let text: String = item.iter().map(|(t, ..)| t.as_str()).collect();
                text_element(&mut xml, &text);
            } else {
                for (text, bold, italic, strike, link) in item {
                    xml.open("r", &[]);
                    xml.open("rPr", &[]);
                    xml.empty("rFont", &[("val", "Calibri")]);
                    if *bold {
                        xml.empty("b", &[]);
                    }
                    if *italic {
                        xml.empty("i", &[]);
                    }
                    if *strike {
                        xml.empty("strike", &[]);
                    }
                    if *link {
                        xml.empty("color", &[("rgb", LINK_COLOR)]);
                    }
                    xml.empty("sz", &[("val", "11")]);
                    if *link {
                        xml.empty("u", &[]);
                    }
                    xml.close("rPr");
                    text_element(&mut xml, text);
                    xml.close("r");
                }
            }
            xml.close("si");
        }
        xml.close("sst");
        xml.finish()
    }
}

/// `<t>`, preserving whitespace a parser would otherwise strip.
fn text_element(xml: &mut Writer, text: &str) {
    let preserve = text.trim() != text || text.contains(['\n', '\t']);
    let attrs: &[(&str, &str)] = if preserve {
        &[("xml:space", "preserve")]
    } else {
        &[]
    };
    xml.element("t", attrs, &xstring(text));
}

/// Text for an ST_Xstring value (ECMA-376 Part 1, §22.9.2.19), where `_xHHHH_` stands for the
/// character U+HHHH: a literal one is written with its underscore escaped as `_x005F_`, so the text
/// reads back as written (markdown-spec.md §9.5, §12.2).
fn xstring(text: &str) -> Cow<'_, str> {
    let bytes = text.as_bytes();
    let escaped = |i: usize| {
        bytes.get(i..i + 7).is_some_and(|w| {
            w[0] == b'_'
                && w[1] == b'x'
                && w[2..6].iter().all(u8::is_ascii_hexdigit)
                && w[6] == b'_'
        })
    };
    if !(0..bytes.len()).any(escaped) {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len() + 7);
    for (i, c) in text.char_indices() {
        if escaped(i) {
            out.push_str("_x005F");
        }
        out.push(c);
    }
    Cow::Owned(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hyperlink_targets_are_uris() {
        assert_eq!(
            package::uri("https://example.com/a?b=c#d"),
            "https://example.com/a?b=c#d"
        );
        assert_eq!(package::uri("a b<c>\"|é"), "a%20b%3Cc%3E%22%7Cé");
    }

    #[test]
    fn literal_escapes_are_escaped() {
        assert_eq!(xstring("plain"), "plain");
        assert_eq!(xstring("_x0041_"), "_x005F_x0041_");
        assert_eq!(xstring("a_x003A_b_x12_"), "a_x005F_x003A_b_x12_");
        assert_eq!(xstring("_x005F_"), "_x005F_x005F_");
        assert_eq!(xstring("é_x00e9_"), "é_x005F_x00e9_");
    }

    #[test]
    fn addresses() {
        assert_eq!(address(0, 1), "A1");
        assert_eq!(address(25, 3), "Z3");
        assert_eq!(address(26, 1), "AA1");
        assert_eq!(address(16_383, 1_048_576), "XFD1048576");
    }
}
