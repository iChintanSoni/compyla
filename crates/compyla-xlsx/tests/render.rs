//! Rendering details the render suite's projection doesn't show.

use std::io::Read;

fn render(source: &str) -> Vec<u8> {
    let parsed = compyla_core::parse(source);
    let input = compyla_core::validate::Input {
        document: &parsed.document,
        sidecar: &parsed.sidecar,
        assets: &|_| compyla_core::Asset::Exists,
    };
    let mut sink = compyla_core::diagnostic::Sink::new();
    let compyla_core::validate::Composed::Xlsx(workbook) =
        compyla_core::validate::validate(&input, compyla_core::Format::Xlsx, &mut sink)
    else {
        unreachable!("an xlsx validation composes a workbook")
    };
    compyla_xlsx::render(&workbook, &compyla_ooxml::Properties::default())
}

fn part(bytes: &[u8], name: &str) -> String {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut text = String::new();
    archive
        .by_name(name)
        .unwrap()
        .read_to_string(&mut text)
        .unwrap();
    text
}

/// The value of `attribute` in the first tag of `xml` starting at `from`.
fn attribute(xml: &str, from: usize, attribute: &str) -> Option<String> {
    let tag = &xml[from..from + xml[from..].find('>')?];
    let value = tag.split(&format!(" {attribute}=\"")).nth(1)?;
    Some(value[..value.find('"')?].to_string())
}

/// Whether cell `address` of the first sheet is in a bold font.
fn bold(bytes: &[u8], address: &str) -> bool {
    font(bytes, address).starts_with("<b/>")
}

/// The `<font>` contents of cell `address` of the first sheet.
fn font(bytes: &[u8], address: &str) -> String {
    let sheet = part(bytes, "xl/worksheets/sheet1.xml");
    let styles = part(bytes, "xl/styles.xml");
    let cell = sheet.find(&format!("<c r=\"{address}\"")).unwrap();
    let style: usize = attribute(&sheet, cell, "s").map_or(0, |s| s.parse().unwrap());
    let xfs = styles.find("<cellXfs").unwrap();
    let xf = styles[xfs..]
        .match_indices("<xf ")
        .nth(style)
        .map(|(at, _)| xfs + at)
        .unwrap();
    let font: usize = attribute(&styles, xf, "fontId").unwrap().parse().unwrap();
    let fonts = &styles[styles.find("<fonts").unwrap()..styles.find("</fonts>").unwrap()];
    let font = fonts.split("<font>").nth(font + 1).unwrap();
    font[..font.find("</font>").unwrap()].to_string()
}

#[test]
fn a_bold_number_keeps_its_emphasis_as_the_cell_font() {
    let bytes = render("# S\n\n| A |\n|---|\n| **42** |\n| 7 |\n");
    assert!(bold(&bytes, "A2"), "**42** is bold");
    assert!(!bold(&bytes, "A3"), "7 isn't");
}

#[test]
fn rich_runs_carry_the_cells_font() {
    // A title is bold: its italic run must be bold too, since a run's properties replace the cell's.
    let bytes = render("# S\n\n## A *b*\n\n| A |\n|---|\n| 1 |\n");
    let strings = part(&bytes, "xl/sharedStrings.xml");
    assert!(
        strings.contains("<r><rPr><rFont val=\"Calibri\"/><b/><i/><sz val=\"11\"/></rPr><t>b</t>"),
        "{strings}"
    );
}

#[test]
fn a_link_in_an_excel_table_takes_the_tables_text_color() {
    // The table's fills are designed for its own text color; the link color on them is hard to
    // read (Excel's accessibility check). Outside a table, a link keeps the link color.
    let bytes = render(
        "# S\n\n::: table\n| A |\n|---|\n| [x](https://e.example) |\n| [**y** z](https://e.example) |\n:::\n\n\
         | B |\n|---|\n| [w](https://e.example) |\n",
    );
    let inside = font(&bytes, "A2");
    assert!(
        inside.contains("<u/>") && !inside.contains("<color"),
        "{inside}"
    );
    let outside = font(&bytes, "A6");
    assert!(
        outside.contains("<u/>") && outside.contains("<color"),
        "{outside}"
    );
    let strings = part(&bytes, "xl/sharedStrings.xml");
    assert!(
        strings.contains("<r><rPr><rFont val=\"Calibri\"/><b/><sz val=\"11\"/><u/></rPr><t>y</t>"),
        "{strings}"
    );
}

#[test]
fn columns_are_as_wide_as_what_they_show() {
    let bytes = render(
        "# S\n\n::: columns B:currency\n:::\n\n| Region | Revenue | When | Notes |\n|---|---|---|---|\n\
         | APAC | 120000 | 2026-01-31 | short |\n| EMEA | 98000 | 2026-02-28 | 日本語のメモ |\n\
         | Total | =SUM(B2:B3) | | |\n",
    );
    let sheet = part(&bytes, "xl/worksheets/sheet1.xml");
    let cols = &sheet[sheet.find("<cols>").unwrap()..sheet.find("</cols>").unwrap()];
    // Region needs no more than the default. Revenue: `$120,000.00` (11), and its total may grow
    // by 2, plus 2 of padding. When: a date (10) plus padding. Notes: six wide characters.
    assert_eq!(
        cols,
        "<cols><col min=\"2\" max=\"2\" width=\"15\" customWidth=\"1\"/>\
         <col min=\"3\" max=\"3\" width=\"12\" customWidth=\"1\"/>\
         <col min=\"4\" max=\"4\" width=\"14\" customWidth=\"1\"/>"
    );
    assert!(sheet.find("<cols>").unwrap() < sheet.find("<sheetData>").unwrap());
}

#[test]
fn titles_widen_nothing_and_widths_stop_at_60() {
    let long = "x".repeat(100);
    let bytes = render(&format!(
        "# S\n\n## A title much longer than any column it sits over\n\n| A |\n|---|\n| {long} |\n"
    ));
    let sheet = part(&bytes, "xl/worksheets/sheet1.xml");
    assert!(
        sheet.contains("<cols><col min=\"1\" max=\"1\" width=\"60\" customWidth=\"1\"/></cols>"),
        "{sheet}"
    );
    let bytes = render(
        "# S\n\n## A title much longer than any column it sits over\n\n| A |\n|---|\n| 1 |\n",
    );
    assert!(!part(&bytes, "xl/worksheets/sheet1.xml").contains("<cols>"));
}

#[test]
fn text_that_reads_as_a_number_isnt_flagged() {
    // Leading zeros and a `text` column keep numbers as text (§9.5); Excel would flag each one as
    // a number stored as text, and offer to convert it.
    let bytes = render(
        "# S\n\n::: columns B:text\n:::\n\n| Code | Id | Name |\n|---|---|---|\n| 007 | 42 | Ana |\n| 1.5 | 7 | 12a |\n",
    );
    let sheet = part(&bytes, "xl/worksheets/sheet1.xml");
    assert!(
        sheet.contains("<ignoredErrors><ignoredError sqref=\"A2 B2:B3\" numberStoredAsText=\"1\"/></ignoredErrors>"),
        "{sheet}"
    );
    assert!(sheet.find("<pageMargins").unwrap() < sheet.find("<ignoredErrors>").unwrap());
}

#[test]
fn a_title_or_note_above_an_excel_table_keeps_the_link_color() {
    // The table's range starts at its header row: the title and note above it aren't in it.
    let bytes = render(
        "# S\n\n## See [the site](https://e.example)\n\nA note with [a link](https://n.example).\n\n\
         ::: table autofilter\n| A | B |\n|---|---|\n| [x](https://e.example) | y |\n:::\n",
    );
    for title_or_note in ["A1", "A2"] {
        let font = font(&bytes, title_or_note);
        assert!(font.contains("<color"), "{title_or_note}: {font}");
    }
    assert!(!font(&bytes, "A4").contains("<color"));
}

#[test]
fn a_column_of_formulas_is_as_wide_as_what_they_may_show() {
    // C and D hold only formulas: C takes its table's widest number (`$120,000.00`) plus 2, and D,
    // a date, 10.
    let bytes = render(
        "# S\n\n::: columns B:currency, C:currency, D:date\n:::\n\n| Item | Price | Total | Due |\n|---|---|---|---|\n\
         | Tea | 120000 | =B2*10 | =DATE(2026,1,31) |\n",
    );
    let sheet = part(&bytes, "xl/worksheets/sheet1.xml");
    let cols = &sheet[sheet.find("<cols>").unwrap()..sheet.find("</cols>").unwrap()];
    assert_eq!(
        cols,
        "<cols><col min=\"2\" max=\"2\" width=\"13\" customWidth=\"1\"/>\
         <col min=\"3\" max=\"3\" width=\"15\" customWidth=\"1\"/>\
         <col min=\"4\" max=\"4\" width=\"12\" customWidth=\"1\"/>"
    );
}

#[test]
fn a_formula_takes_widths_from_its_own_table() {
    // A number in a table stacked above doesn't stand for this table's formulas.
    let alone =
        "# S\n\n::: columns B:currency\n:::\n\n| Total | Price |\n|---|---|\n| =B2*10 | 120000 |\n";
    let stacked = "# S\n\n| Code |\n|---|\n| 7 |\n\n::: columns B:currency\n:::\n\n| Total | Price |\n|---|---|\n| =B2*10 | 120000 |\n";
    let width = |source: &str| {
        let sheet = part(&render(source), "xl/worksheets/sheet1.xml");
        let at = sheet.find("<col min=\"1\"").unwrap();
        attribute(&sheet, at, "width").unwrap()
    };
    assert_eq!(width(alone), "15");
    assert_eq!(width(stacked), "15");
}
