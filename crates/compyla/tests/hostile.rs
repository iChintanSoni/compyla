//! Hostile input (markdown-spec.md §12.1): inputs that once took quadratic time. Each runs in well
//! under a second; the bound is generous so that only a return to quadratic behaviour fails.

use std::time::{Duration, Instant};

const BOUND: Duration = Duration::from_secs(10);

fn lint_within_bound(
    name: &str,
    source: &str,
    format: compyla::Format,
) -> Vec<compyla::Diagnostic> {
    let options = compyla::Options {
        format: Some(format),
        strict: false,
        ..Default::default()
    };
    let start = Instant::now();
    let diagnostics = compyla::lint(source, &options, &compyla::no_assets).unwrap();
    let elapsed = start.elapsed();
    assert!(elapsed < BOUND, "{name} took {elapsed:?}");
    diagnostics
}

#[test]
fn repeated_headings_take_linear_time() {
    // Each duplicate's slug once searched its suffixes from -1.
    let source = "# a\n\n".repeat(40_000);
    lint_within_bound("duplicate headings", &source, compyla::Format::Docx);
}

#[test]
fn many_directive_options_take_linear_time() {
    let keys: Vec<String> = (0..80_000).map(|i| format!("k{i}")).collect();
    let source = format!("::: table {}\n| A |\n|---|\n| 1 |\n:::\n", keys.join(" "));
    lint_within_bound("many options", &source, compyla::Format::Xlsx);
}

#[test]
fn a_long_line_of_inline_nodes_takes_linear_time() {
    // Character columns were once recomputed from the start of the line for every node.
    let line: Vec<String> = (0..12_000).map(|i| format!("*é{i}*")).collect();
    let source = format!("{}\n", line.join(" "));
    lint_within_bound("long line", &source, compyla::Format::Docx);
}

#[test]
fn leading_notes_take_linear_time() {
    let source = format!(
        "# T\n\n{}{}",
        "::: notes\n:::\n".repeat(40_000),
        "::: layout Blank\n:::\n".repeat(40_000)
    );
    lint_within_bound("leading notes", &source, compyla::Format::Pptx);
}

#[test]
fn diagnostics_are_capped() {
    let source = format!(
        "::: columns {}\n| A |\n|---|\n| 1 |\n:::\n",
        "X,".repeat(100_000)
    );
    let diagnostics = lint_within_bound("many diagnostics", &source, compyla::Format::Xlsx);
    assert_eq!(diagnostics.len(), 1001);
    assert_eq!(diagnostics.last().unwrap().code, "diagnostics_truncated");
}

#[test]
fn a_huge_ir_position_doesnt_overflow() {
    let ir = serde_json::json!({
        "irVersion": "0.1",
        "meta": {},
        "body": [{
            "type": "table",
            "position": { "line": 4_294_967_295u64, "column": 1 },
            "header": [{ "runs": [{ "text": "A" }], "raw": "A" }],
            "align": [null],
            "rows": [[{ "runs": [{ "text": "=SUM(" }], "raw": "=SUM(" }]]
        }]
    });
    let options = compyla::Options {
        format: Some(compyla::Format::Xlsx),
        strict: false,
        ..Default::default()
    };
    let diagnostics = compyla::lint_ir(&ir.to_string(), &options, &compyla::no_assets).unwrap();
    assert!(diagnostics.iter().any(|d| d.code == "formula_error"));
}

#[test]
#[cfg(feature = "xlsx")]
fn a_blocked_convert_reports_its_error_past_the_cap() {
    // 1,100 warnings sort before the error, which the cap still keeps (§11.2).
    let source = format!(
        "# S\n\n{}| A |\n|---|\n| {} |\n",
        "### h\n\n".repeat(1_100),
        "x".repeat(32_800)
    );
    let options = compyla::Options {
        format: Some(compyla::Format::Xlsx),
        strict: false,
        ..Default::default()
    };
    let result = compyla::convert(&source, &options, &compyla::no_assets).unwrap();
    assert!(result.bytes.is_none());
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.code == "xlsx_limit_exceeded")
    );
    assert_eq!(result.diagnostics.len(), 1001);
    let lint = compyla::lint(&source, &options, &compyla::no_assets).unwrap();
    assert!(lint.iter().any(|d| d.severity == compyla::Severity::Error));
}

#[test]
#[cfg(feature = "xlsx")]
fn many_excel_tables_convert() {
    // Each Excel table is a package part: more than 65,535 parts need Zip64.
    let source = format!(
        "# S\n\n{}",
        "::: table\n| A |\n|---|\n:::\n\n".repeat(70_000)
    );
    let options = compyla::Options {
        format: Some(compyla::Format::Xlsx),
        strict: false,
        ..Default::default()
    };
    let start = Instant::now();
    let result = compyla::convert(&source, &options, &compyla::no_assets).unwrap();
    assert!(start.elapsed() < BOUND * 3, "took {:?}", start.elapsed());
    let bytes = result.bytes.expect("converted");
    let archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    assert!(archive.len() > 70_000);
}

#[test]
#[cfg(feature = "pptx")]
fn a_picture_naming_one_link_in_many_attributes_converts() {
    // Every attribute of one blip names the same external link; each goes, in one linear pass
    // (§10.4, §12.1), not one search of the others per attribute.
    const BRAND: &[u8] =
        include_bytes!("../../../spec/conformance/lint/139-pptx-template-binding/brand.potx");
    const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
    let attributes: String = (0..60_000).map(|i| format!(r#" r:l{i}="rIdX""#)).collect();
    let picture = format!(
        r#"<p:pic><p:nvPicPr><p:cNvPr id="90" name=""/><p:cNvPicPr/><p:nvPr/></p:nvPicPr><p:blipFill><a:blip{attributes}/></p:blipFill><p:spPr/></p:pic>"#
    );
    let link = format!(
        r#"<Relationship Id="rIdX" Type="{R}/image" Target="https://tracker.example/x.png" TargetMode="External"/></Relationships>"#
    );
    let entries: Vec<(String, Vec<u8>)> = compyla_ooxml::read::entries(BRAND)
        .unwrap()
        .into_iter()
        .map(|(name, data)| {
            let text = String::from_utf8_lossy(&data);
            let data = match name.as_str() {
                "ppt/slideMasters/slideMaster1.xml" => text
                    .replacen(
                        "</p:spTree></p:cSld>",
                        &format!("{picture}</p:spTree></p:cSld>"),
                        1,
                    )
                    .into_bytes(),
                "ppt/slideMasters/_rels/slideMaster1.xml.rels" => {
                    text.replacen("</Relationships>", &link, 1).into_bytes()
                }
                _ => data,
            };
            (name, data)
        })
        .collect();
    let options = compyla::Options {
        format: Some(compyla::Format::Pptx),
        template: Some(compyla_ooxml::zip::write(&entries)),
        ..Default::default()
    };
    let start = Instant::now();
    let result = compyla::convert("# A\n", &options, &compyla::no_assets).unwrap();
    assert!(start.elapsed() < BOUND, "took {:?}", start.elapsed());
    assert!(result.bytes.is_some());
    assert_eq!(
        result
            .diagnostics
            .iter()
            .filter(|d| d.code == "template_content_removed")
            .count(),
        1
    );
}

#[test]
#[cfg(feature = "pptx")]
fn a_template_full_of_active_content_converts() {
    // Each linked picture and OLE object is found, removed and reported in one pass (§10.4); the
    // reports are capped like any others (§11.2).
    const BRAND: &[u8] =
        include_bytes!("../../../spec/conformance/lint/139-pptx-template-binding/brand.potx");
    const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
    let n = 8_000;
    let mut shapes = String::new();
    let mut rels = String::new();
    for i in 0..n {
        shapes.push_str(&format!(
            r#"<p:pic><p:nvPicPr><p:cNvPr id="{}" name=""/><p:cNvPicPr/><p:nvPr/></p:nvPicPr><p:blipFill><a:blip r:link="rIdL{i}"/></p:blipFill><p:spPr/></p:pic><p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id="{}" name=""/><p:cNvGraphicFramePr/><p:nvPr/></p:nvGraphicFramePr><p:xfrm/><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/presentationml/2006/ole"><p:oleObj r:id="rIdO{i}"><p:embed/></p:oleObj></a:graphicData></a:graphic></p:graphicFrame>"#,
            100 + 2 * i,
            101 + 2 * i
        ));
        rels.push_str(&format!(
            r#"<Relationship Id="rIdL{i}" Type="{R}/image" Target="https://tracker.example/{i}.png" TargetMode="External"/><Relationship Id="rIdO{i}" Type="{R}/oleObject" Target="../embeddings/o{i}.bin"/>"#
        ));
    }
    let entries: Vec<(String, Vec<u8>)> = compyla_ooxml::read::entries(BRAND)
        .unwrap()
        .into_iter()
        .map(|(name, data)| {
            let text = String::from_utf8_lossy(&data);
            let data = match name.as_str() {
                "ppt/slideMasters/slideMaster1.xml" => text
                    .replacen(
                        "</p:spTree></p:cSld>",
                        &format!("{shapes}</p:spTree></p:cSld>"),
                        1,
                    )
                    .into_bytes(),
                "ppt/slideMasters/_rels/slideMaster1.xml.rels" => text
                    .replacen("</Relationships>", &format!("{rels}</Relationships>"), 1)
                    .into_bytes(),
                _ => data,
            };
            (name, data)
        })
        .collect();
    let options = compyla::Options {
        format: Some(compyla::Format::Pptx),
        template: Some(compyla_ooxml::zip::write(&entries)),
        ..Default::default()
    };
    let start = Instant::now();
    let result = compyla::convert(
        "# A\n\n---\n\n## B\n\nText.\n",
        &options,
        &compyla::no_assets,
    )
    .unwrap();
    assert!(start.elapsed() < BOUND, "took {:?}", start.elapsed());
    assert!(result.bytes.is_some());
    let codes: Vec<&str> = result.diagnostics.iter().map(|d| d.code.as_str()).collect();
    assert_eq!(codes.len(), 1_001);
    assert!(
        codes[..1_000]
            .iter()
            .all(|&c| c == "template_content_removed")
    );
    assert_eq!(codes[1_000], "diagnostics_truncated");
}
