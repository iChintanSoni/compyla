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
