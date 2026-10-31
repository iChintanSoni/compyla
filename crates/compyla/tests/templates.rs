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
