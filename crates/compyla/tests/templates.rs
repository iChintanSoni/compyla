//! Supplied templates through the facade (markdown-spec.md §8.5, §10.2–§10.4): what the
//! conformance suites' fixtures don't reach.

const BRAND: &[u8] =
    include_bytes!("../../../spec/conformance/lint/139-pptx-template-binding/brand.potx");

fn pptx(template: Option<&[u8]>, aliases: &[(&str, &str)]) -> compyla::Options {
    compyla::Options {
        format: Some(compyla::Format::Pptx),
        template: template.map(<[u8]>::to_vec),
        layout_aliases: aliases
            .iter()
            .map(|(name, layout)| (name.to_string(), layout.to_string()))
            .collect(),
        ..Default::default()
    }
}

fn codes(source: &str, options: &compyla::Options) -> Vec<String> {
    compyla::lint(source, options, &compyla::no_assets)
        .unwrap()
        .into_iter()
        .map(|d| d.code)
        .collect()
}

#[test]
fn aliases_need_a_supplied_template_and_must_name_its_layouts() {
    let source = "# A\n\n## B\n\nText.\n";
    let error = |options| {
        compyla::lint(source, &options, &compyla::no_assets)
            .unwrap_err()
            .code
    };
    assert_eq!(error(pptx(None, &[("Quote", "Blank")])), "option_invalid");
    assert_eq!(
        error(pptx(Some(BRAND), &[("Quote", "Nowhere")])),
        "option_invalid"
    );
}

#[test]
fn an_alias_binds_a_house_layout_the_template_lacks() {
    let source = "# A\n\n---\n\n## B\n\n> A quote.\n";
    assert_eq!(
        codes(source, &pptx(Some(BRAND), &[])),
        ["layout_not_in_template"]
    );
    assert!(codes(source, &pptx(Some(BRAND), &[("Quote", "titel UND inhalt")])).is_empty());
}

#[test]
fn the_option_comes_before_the_frontmatters_template() {
    // `meta.template` names nothing the host can resolve, but the option is used first (§10.2).
    let source = "---\ntemplate: missing.potx\n---\n\n# A\n\n## B\n\nText.\n";
    assert!(codes(source, &pptx(Some(BRAND), &[])).is_empty());
    assert_eq!(codes(source, &pptx(None, &[])), ["template_unresolved"]);
}

#[test]
fn convert_renders_into_the_supplied_template() {
    let source = "# A\n\n---\n\n## B\n\n::: layout Titel und Inhalt\n:::\n\nText.\n";
    let result = compyla::convert(source, &pptx(Some(BRAND), &[]), &compyla::no_assets).unwrap();
    let bytes = result.bytes.expect("rendered");
    let layouts = compyla::inspect_template(&bytes)["layouts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["name"].as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    assert_eq!(layouts, ["Title Slide", "Titel und Inhalt", "Blank"]);
}

/// The brand template with `edit` applied to its slide master.
fn brand_with_master(edit: impl Fn(&str) -> String) -> Vec<u8> {
    let entries: Vec<(String, Vec<u8>)> = compyla_ooxml::read::entries(BRAND)
        .unwrap()
        .into_iter()
        .map(|(name, data)| {
            if name == "ppt/slideMasters/slideMaster1.xml" {
                let xml = edit(std::str::from_utf8(&data).unwrap());
                (name, xml.into_bytes())
            } else {
                (name, data)
            }
        })
        .collect();
    compyla_ooxml::zip::write(&entries)
}

#[test]
fn a_template_without_layouts_is_invalid_not_a_panic() {
    let empty = brand_with_master(|xml| {
        let start = xml.find("<p:sldLayoutIdLst>").unwrap();
        let end = xml.find("</p:sldLayoutIdLst>").unwrap();
        format!("{}<p:sldLayoutIdLst>{}", &xml[..start], &xml[end..])
    });
    let source = "# A\n\n## B\n\nText.\n";
    assert_eq!(
        codes(source, &pptx(Some(&empty), &[])),
        ["template_invalid"]
    );
    // Asked for and unusable: its error is reported, not the aliases'.
    assert_eq!(
        codes(source, &pptx(Some(&empty), &[("Quote", "Blank")])),
        ["template_invalid"]
    );
}

#[test]
fn aliases_wait_for_a_template_that_was_asked_for() {
    let source = "---\ntemplate: missing.potx\n---\n\n# A\n\n## B\n\nText.\n";
    assert_eq!(
        codes(source, &pptx(None, &[("Quote", "Blank")])),
        ["template_unresolved"]
    );
    // Other formats ignore `layoutAliases`.
    let xlsx = compyla::Options {
        format: Some(compyla::Format::Xlsx),
        layout_aliases: vec![("Quote".into(), "Blank".into())],
        ..Default::default()
    };
    assert!(codes("# S\n\n| A |\n|---|\n| 1 |\n", &xlsx).is_empty());
}

#[test]
fn a_workbook_takes_a_templates_theme() {
    let source = "# S\n\n## Title\n\n| A |\n|---|\n| 1 |\n\n::: table\n| B |\n|---|\n| 2 |\n:::\n";
    let xlsx = |template: &[u8]| compyla::Options {
        format: Some(compyla::Format::Xlsx),
        template: Some(template.to_vec()),
        ..Default::default()
    };
    // A pptx template's theme becomes the workbook's: its fonts and colors (§9.2).
    let result = compyla::convert(source, &xlsx(BRAND), &compyla::no_assets).unwrap();
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    let parts: std::collections::HashMap<String, Vec<u8>> =
        compyla_ooxml::read::entries(&result.bytes.unwrap())
            .unwrap()
            .into_iter()
            .collect();
    let theme = String::from_utf8(parts["xl/theme/theme1.xml"].clone()).unwrap();
    assert!(
        theme.contains(r#"<a:accent1><a:srgbClr val="2F5BEA"/></a:accent1>"#),
        "{theme}"
    );
    assert!(
        theme.contains(r#"<a:minorFont><a:latin typeface="Brand Sans"/>"#),
        "{theme}"
    );
    let styles = String::from_utf8(parts["xl/styles.xml"].clone()).unwrap();
    assert!(
        styles.contains(r#"<name val="Brand Sans"/><scheme val="minor"/>"#),
        "{styles}"
    );
    assert!(
        styles.contains(r#"<b/><sz val="11"/><name val="Brand Display"/><scheme val="major"/>"#),
        "{styles}"
    );
    // White reads on this accent: the striped table keeps its accent header.
    let table = String::from_utf8(parts["xl/tables/table1.xml"].clone()).unwrap();
    assert!(table.contains(r#"name="TableStyleMedium2""#), "{table}");

    // A docx template without a theme leaves the house style, and says so; `no_styles_xml` is for
    // docx documents only.
    let docx =
        include_bytes!("../../../spec/conformance/lint/143-docx-template-without-theme/brand.docx");
    let result = compyla::convert(source, &xlsx(docx), &compyla::no_assets).unwrap();
    let codes: Vec<&str> = result.diagnostics.iter().map(|d| d.code.as_str()).collect();
    assert_eq!(codes, ["no_theme"]);
    let parts: Vec<String> = compyla_ooxml::read::entries(&result.bytes.unwrap())
        .unwrap()
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    assert!(!parts.iter().any(|p| p.contains("theme")), "{parts:?}");
}

#[test]
fn a_theme_part_that_isnt_a_theme_is_no_theme() {
    let entries: Vec<(String, Vec<u8>)> = compyla_ooxml::read::entries(BRAND)
        .unwrap()
        .into_iter()
        .map(|(name, data)| match name.as_str() {
            "ppt/theme/theme1.xml" => (name, br#"<foo xmlns="urn:x"/>"#.to_vec()),
            _ => (name, data),
        })
        .collect();
    let template = compyla_ooxml::zip::write(&entries);
    assert_eq!(
        codes("# A\n\n## B\n\nText.\n", &pptx(Some(&template), &[])),
        ["no_theme"]
    );
}

const ACTIVE: &[u8] =
    include_bytes!("../../../spec/conformance/render/121-pptx-template-active-content/brand.potx");

/// render/121's template with the part `name` edited.
fn active_with(name: &str, edit: impl Fn(&str) -> String) -> Vec<u8> {
    let entries: Vec<(String, Vec<u8>)> = compyla_ooxml::read::entries(ACTIVE)
        .unwrap()
        .into_iter()
        .map(|(part, data)| {
            if part == name {
                let xml = edit(std::str::from_utf8(&data).unwrap());
                (part, xml.into_bytes())
            } else {
                (part, data)
            }
        })
        .collect();
    compyla_ooxml::zip::write(&entries)
}

fn converted(source: &str, options: &compyla::Options) -> compyla::ConvertResult {
    compyla::convert(source, options, &compyla::no_assets).unwrap()
}

#[test]
fn removals_are_reported_in_order_by_convert_and_not_by_lint() {
    // Render-stage diagnostics join the others in §11.2 order: without a line, by code.
    let source = "# A\n\n---\n\n## B\n\n![Logo](missing.png)\n";
    let options = pptx(Some(ACTIVE), &[]);
    assert_eq!(codes(source, &options), ["layout_not_in_template"]);
    let convert: Vec<String> = converted(source, &options)
        .diagnostics
        .into_iter()
        .map(|d| d.code)
        .collect();
    let removed = convert
        .iter()
        .filter(|code| *code == "template_content_removed")
        .count();
    assert!(removed > 0);
    let mut expected = vec!["layout_not_in_template"];
    expected.extend(vec!["template_content_removed"; removed]);
    expected.push("image_unresolved");
    assert_eq!(convert, expected);
}

#[test]
fn removals_join_a_list_the_cap_already_cut() {
    // Lint's diagnostics already fill the cap; the removals, without a line, sort before the
    // images and push the last of them out, and the list still ends with one truncation (§11.2).
    let mut source = String::from("# A\n\n---\n\n## B\n\n");
    for i in 0..1100 {
        source.push_str(&format!("![Logo](missing{i}.png)\n\n"));
    }
    let result = converted(&source, &pptx(Some(ACTIVE), &[]));
    let codes: Vec<&str> = result.diagnostics.iter().map(|d| d.code.as_str()).collect();
    let removed = codes
        .iter()
        .filter(|code| **code == "template_content_removed")
        .count();
    assert!(removed > 0);
    assert_eq!(codes.len(), 1001);
    assert!(
        codes[..removed]
            .iter()
            .all(|c| *c == "template_content_removed")
    );
    assert!(
        codes[removed..1000]
            .iter()
            .all(|c| *c != "template_content_removed" && *c != "diagnostics_truncated")
    );
    assert_eq!(codes[1000], "diagnostics_truncated");
    assert!(result.bytes.is_some());
}

#[test]
fn strict_doesnt_promote_removals() {
    // `template_content_removed` is advisory: strict mode still renders (§11.1).
    let options = compyla::Options {
        strict: true,
        ..pptx(Some(ACTIVE), &[])
    };
    let result = converted("# Active content\n\n---\n\n## Kept\n\nText.\n", &options);
    assert!(result.bytes.is_some());
    assert!(!result.diagnostics.is_empty());
    assert!(
        result
            .diagnostics
            .iter()
            .all(|d| d.code == "template_content_removed"
                && d.severity == compyla_core::diagnostic::Severity::Warning)
    );
}

#[test]
fn a_templates_text_reaches_messages_escaped() {
    // A target written to move a terminal's cursor, color the line and forge another.
    let template = active_with("ppt/_rels/presentation.xml.rels", |xml| {
        xml.replace(
            "</Relationships>",
            r#"<Relationship Id="rIdE" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="https://x.example/&#x1B;[31m&#x202E;red&#10;in.md: 0 errors" TargetMode="External"/></Relationships>"#,
        )
    });
    let result = converted("# A\n", &pptx(Some(&template), &[]));
    let message = result
        .diagnostics
        .iter()
        .find(|d| d.message.contains("x.example"))
        .map(|d| d.message.clone())
        .expect("the removal is reported");
    assert!(
        !message.contains(['\u{1b}', '\n', '\u{202e}']),
        "{message:?}"
    );
    assert!(
        message.contains(r"\u{1b}[31m\u{202e}red\u{a}in.md"),
        "{message}"
    );
}

#[test]
fn an_invalid_templates_reason_is_escaped() {
    // An entry name written to clear the screen and retitle the terminal (§10.4).
    let mut entries: Vec<(String, Vec<u8>)> = compyla_ooxml::read::entries(BRAND).unwrap();
    entries.push((
        "ppt/\u{1b}]0;pwned\u{7}\u{1b}[2J.xml".into(),
        b"<x/>".to_vec(),
    ));
    let template = compyla_ooxml::zip::write(&entries);
    let diagnostics =
        compyla::lint("# A\n", &pptx(Some(&template), &[]), &compyla::no_assets).unwrap();
    let invalid = diagnostics
        .iter()
        .find(|d| d.code == "template_invalid")
        .expect("the template is refused");
    assert!(
        !invalid.message.contains(['\u{1b}', '\u{7}']),
        "{:?}",
        invalid.message
    );
    assert!(
        invalid.message.contains(r"\u{1b}]0;pwned\u{7}"),
        "{}",
        invalid.message
    );
}
