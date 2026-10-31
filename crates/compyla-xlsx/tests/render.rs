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
    fonts
        .split("<font>")
        .nth(font + 1)
        .unwrap()
        .starts_with("<b/>")
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
