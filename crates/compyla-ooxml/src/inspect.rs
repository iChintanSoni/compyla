//! `inspect_template` (markdown-spec.md §10.5): what a template offers, as the template report
//! (`spec/template-report.schema.json`), so agents can write markdown against its real layouts and
//! styles. Lengths are in points.

use compyla_core::diagnostic::Diagnostic;
use serde_json::{Map, Value, json};

use crate::dom::{Element, MAIN_A, MAIN_P, MAIN_W, MAIN_X, REL_NS};
use crate::template::{Invalid, Kind, Template};

/// The report for `bytes`. An invalid template's report has `kind: null` and only its issue.
pub fn inspect(bytes: &[u8]) -> Value {
    match Template::read(bytes).and_then(|template| report(&template)) {
        Ok(report) => report,
        Err(Invalid(reason)) => json!({
            "kind": null,
            "theme": { "fonts": {}, "colors": {} },
            "fonts": [],
            "issues": [issue(Diagnostic::new("template_invalid", format!("The template {reason}.")))],
        }),
    }
}

fn issue(diagnostic: Diagnostic) -> Value {
    serde_json::to_value(diagnostic).expect("diagnostics serialize")
}

/// The fonts a template references, in the order first seen, with whether it embeds each.
#[derive(Default)]
struct Fonts {
    list: Vec<(String, bool)>,
    index: std::collections::HashMap<String, usize>,
}

impl Fonts {
    fn add(&mut self, family: &str, embedded: bool) {
        let family = family.trim();
        // `+mj-lt` and friends refer to the theme's fonts, which are listed themselves.
        if family.is_empty() || family.starts_with('+') {
            return;
        }
        match self.index.get(family) {
            Some(&at) => self.list[at].1 |= embedded,
            None => {
                self.index.insert(family.to_string(), self.list.len());
                self.list.push((family.to_string(), embedded));
            }
        }
    }

    fn to_json(&self) -> Value {
        Value::Array(
            self.list
                .iter()
                .map(|(family, embedded)| json!({ "family": family, "embedded": embedded }))
                .collect(),
        )
    }
}

fn report(template: &Template) -> Result<Value, Invalid> {
    let mut issues = Vec::new();
    let mut fonts = Fonts::default();
    let main = template.main.clone();
    let theme_part = match template.related(&main, "theme")? {
        Some(part) => Some(part),
        // pptx: the theme hangs off the first slide master.
        None => match first_master(template)? {
            Some(master) => template.related(&master, "theme")?,
            None => None,
        },
    };
    let theme = match theme_part.as_deref().map(|p| template.xml(p)).transpose()? {
        Some(Some(theme)) => theme_json(&theme, &mut fonts),
        _ => {
            issues.push(issue(Diagnostic::new(
                "no_theme",
                "The template has no theme part; default fonts and colors are used.",
            )));
            json!({ "fonts": {}, "colors": {} })
        }
    };

    let mut report = Map::new();
    report.insert("kind".into(), template.kind.name().into());
    report.insert("theme".into(), theme);
    match template.kind {
        Kind::Docx => docx(template, &mut report, &mut fonts, &mut issues)?,
        Kind::Pptx => pptx(template, &mut report, &mut fonts)?,
        Kind::Xlsx => {
            if let Some(styles) = template.related(&main, "styles")? {
                for font in template.xml(&styles)?.iter().flat_map(|s| s.descendants()) {
                    if font.is(MAIN_X, "name") {
                        fonts.add(font.attr("val").unwrap_or_default(), false);
                    }
                }
            }
        }
    }
    report.insert("fonts".into(), fonts.to_json());
    report.insert("issues".into(), Value::Array(issues));
    // Keys in the schema's order.
    let order = [
        "kind",
        "theme",
        "fonts",
        "styles",
        "page",
        "slideSize",
        "layouts",
        "issues",
    ];
    let mut ordered = Map::new();
    for key in order {
        if let Some(value) = report.remove(key) {
            ordered.insert(key.into(), value);
        }
    }
    Ok(Value::Object(ordered))
}

fn theme_json(theme: &Element, fonts: &mut Fonts) -> Value {
    let elements = theme.child(MAIN_A, "themeElements");
    let mut theme_fonts = Map::new();
    if let Some(scheme) = elements.and_then(|e| e.child(MAIN_A, "fontScheme")) {
        for (key, element) in [("major", "majorFont"), ("minor", "minorFont")] {
            let typeface = scheme
                .path(&[(MAIN_A, element), (MAIN_A, "latin")])
                .and_then(|latin| latin.attr("typeface"))
                .unwrap_or_default();
            if !typeface.is_empty() {
                theme_fonts.insert(key.into(), typeface.into());
                fonts.add(typeface, false);
            }
        }
    }
    let mut colors = Map::new();
    if let Some(scheme) = elements.and_then(|e| e.child(MAIN_A, "clrScheme")) {
        for slot in &scheme.children {
            let color = slot
                .child(MAIN_A, "srgbClr")
                .and_then(|c| c.attr("val"))
                .or_else(|| slot.child(MAIN_A, "sysClr").and_then(|c| c.attr("lastClr")));
            if let Some(color) =
                color.filter(|c| c.len() == 6 && c.bytes().all(|b| b.is_ascii_hexdigit()))
            {
                colors.insert(slot.name.clone(), color.to_ascii_uppercase().into());
            }
        }
    }
    json!({ "fonts": theme_fonts, "colors": colors })
}

/// A length as JSON: an integer when it is whole.
fn points(value: f64) -> Value {
    if value.fract() == 0.0 && value.abs() < 1e15 {
        json!(value as i64)
    } else {
        json!(value)
    }
}

fn twips(element: Option<&Element>, attribute: &str, default: f64) -> Value {
    let value = element
        .and_then(|e| e.attr_ns(MAIN_W, attribute))
        .and_then(|v| v.parse::<f64>().ok())
        .filter(|v| v.is_finite() && *v >= 0.0)
        .unwrap_or(default);
    points(value / 20.0)
}

fn docx(
    template: &Template,
    report: &mut Map<String, Value>,
    fonts: &mut Fonts,
    issues: &mut Vec<Value>,
) -> Result<(), Invalid> {
    let main = &template.main;
    let mut styles = Vec::new();
    match template.related(main, "styles")? {
        Some(part) => {
            if let Some(root) = template.xml(&part)? {
                for style in root.children_named(MAIN_W, "style") {
                    let kind = style.attr_ns(MAIN_W, "type").unwrap_or("paragraph");
                    if !matches!(kind, "paragraph" | "character" | "table" | "numbering") {
                        continue;
                    }
                    let id = style.attr_ns(MAIN_W, "styleId").unwrap_or_default();
                    let name = style
                        .child(MAIN_W, "name")
                        .and_then(|n| n.attr_ns(MAIN_W, "val"))
                        .unwrap_or(id);
                    styles.push(json!({ "name": name, "id": id, "type": kind }));
                }
                for fonts_element in root.descendants() {
                    if fonts_element.is(MAIN_W, "rFonts") {
                        for attribute in ["ascii", "hAnsi"] {
                            if let Some(family) = fonts_element.attr_ns(MAIN_W, attribute) {
                                fonts.add(family, false);
                            }
                        }
                    }
                }
            }
        }
        None => issues.push(issue(Diagnostic::new(
            "no_styles_xml",
            "The template has no styles part; the house styles are used.",
        ))),
    }
    if let Some(part) = template.related(main, "fontTable")?
        && let Some(table) = template.xml(&part)?
    {
        for font in table.children_named(MAIN_W, "font") {
            let embedded = font
                .children
                .iter()
                .any(|c| c.namespace == MAIN_W && c.name.starts_with("embed"));
            fonts.add(font.attr_ns(MAIN_W, "name").unwrap_or_default(), embedded);
        }
    }

    // The final section's page setup; Word's defaults (Letter, 1-inch margins) where it is silent.
    let document = template.xml(main)?;
    let section = document
        .as_ref()
        .and_then(|d| d.path(&[(MAIN_W, "body"), (MAIN_W, "sectPr")]));
    let size = section.and_then(|s| s.child(MAIN_W, "pgSz"));
    let margins = section.and_then(|s| s.child(MAIN_W, "pgMar"));
    report.insert("styles".into(), Value::Array(styles));
    report.insert(
        "page".into(),
        json!({
            "width": twips(size, "w", 12_240.0),
            "height": twips(size, "h", 15_840.0),
            "margins": {
                "top": twips(margins, "top", 1_440.0),
                "right": twips(margins, "right", 1_440.0),
                "bottom": twips(margins, "bottom", 1_440.0),
                "left": twips(margins, "left", 1_440.0),
            },
        }),
    );
    Ok(())
}

fn first_master(template: &Template) -> Result<Option<String>, Invalid> {
    Ok(masters(template)?.into_iter().next())
}

/// The slide masters, in `sldMasterIdLst` order.
fn masters(template: &Template) -> Result<Vec<String>, Invalid> {
    if template.kind != Kind::Pptx {
        return Ok(Vec::new());
    }
    let rels = template.relationships(&template.main)?;
    let Some(presentation) = template.xml(&template.main)? else {
        return Ok(Vec::new());
    };
    let ids = presentation
        .child(MAIN_P, "sldMasterIdLst")
        .map(|list| {
            list.children_named(MAIN_P, "sldMasterId")
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Ok(ids
        .into_iter()
        .filter_map(|id| id.attr_ns(REL_NS, "id"))
        .filter_map(|id| rels.iter().find(|r| r.id == id && !r.external))
        .map(|r| r.target.clone())
        .collect())
}

fn pptx(
    template: &Template,
    report: &mut Map<String, Value>,
    fonts: &mut Fonts,
) -> Result<(), Invalid> {
    let presentation = template.xml(&template.main)?.unwrap_or_default();
    let size = presentation.child(MAIN_P, "sldSz");
    let emu = |attribute: &str, default: f64| {
        let value = size
            .and_then(|s| s.attr(attribute))
            .and_then(|v| v.parse::<f64>().ok())
            .filter(|v| v.is_finite() && *v > 0.0)
            .unwrap_or(default);
        points(value / 12_700.0)
    };
    // PowerPoint's default when the size is missing: 16:9 at 13.333 × 7.5 inches.
    report.insert(
        "slideSize".into(),
        json!({ "width": emu("cx", 12_192_000.0), "height": emu("cy", 6_858_000.0) }),
    );
    if let Some(list) = presentation.child(MAIN_P, "embeddedFontLst") {
        for embedded in list.children_named(MAIN_P, "embeddedFont") {
            if let Some(font) = embedded.child(MAIN_P, "font") {
                fonts.add(font.attr("typeface").unwrap_or_default(), true);
            }
        }
    }

    let mut layouts = Vec::new();
    for master in masters(template)? {
        let Some(master_xml) = template.xml(&master)? else {
            continue;
        };
        add_typefaces(&master_xml, fonts);
        let rels = template.relationships(&master)?;
        let ids = master_xml
            .child(MAIN_P, "sldLayoutIdLst")
            .map(|list| {
                list.children_named(MAIN_P, "sldLayoutId")
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        for id in ids {
            let Some(part) = id
                .attr_ns(REL_NS, "id")
                .and_then(|id| rels.iter().find(|r| r.id == id && !r.external))
            else {
                continue;
            };
            let Some(layout) = template.xml(&part.target)? else {
                continue;
            };
            add_typefaces(&layout, fonts);
            layouts.push(layout_json(&layout));
        }
    }
    report.insert("layouts".into(), Value::Array(layouts));
    Ok(())
}

fn add_typefaces(part: &Element, fonts: &mut Fonts) {
    for element in part.descendants() {
        if element.namespace == MAIN_A && matches!(element.name.as_str(), "latin" | "ea" | "cs") {
            fonts.add(element.attr("typeface").unwrap_or_default(), false);
        }
    }
}

fn layout_json(layout: &Element) -> Value {
    let name = layout
        .child(MAIN_P, "cSld")
        .and_then(|c| c.attr("name"))
        .unwrap_or_default();
    let mut placeholders = Vec::new();
    if let Some(tree) = layout.path(&[(MAIN_P, "cSld"), (MAIN_P, "spTree")]) {
        for shape in tree.descendants() {
            if shape.namespace != MAIN_P
                || !matches!(shape.name.as_str(), "sp" | "pic" | "graphicFrame" | "cxnSp")
            {
                continue;
            }
            let Some(properties) = shape
                .children
                .iter()
                .find(|c| c.namespace == MAIN_P && c.name.starts_with("nv"))
            else {
                continue;
            };
            let Some(placeholder) = properties.path(&[(MAIN_P, "nvPr"), (MAIN_P, "ph")]) else {
                continue;
            };
            let shape_name = properties
                .children
                .iter()
                .find(|c| {
                    c.namespace == MAIN_P
                        && c.name.starts_with("cNv")
                        && c.name.ends_with("Pr")
                        && c.attr("name").is_some()
                })
                .and_then(|c| c.attr("name"))
                .unwrap_or_default();
            let mut entry = Map::new();
            entry.insert(
                "type".into(),
                placeholder.attr("type").unwrap_or("obj").into(),
            );
            if let Some(index) = placeholder.attr("idx").and_then(|i| i.parse::<u64>().ok()) {
                entry.insert("index".into(), index.into());
            }
            entry.insert("name".into(), shape_name.into());
            placeholders.push(Value::Object(entry));
        }
    }
    let mut entry = Map::new();
    entry.insert("name".into(), name.into());
    if let Some(kind) = layout.attr("type") {
        entry.insert("type".into(), kind.into());
    }
    entry.insert("placeholders".into(), Value::Array(placeholders));
    Value::Object(entry)
}
