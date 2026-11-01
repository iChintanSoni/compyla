//! A template's slide layouts (markdown-spec.md §8.5, §8.6): each layout's part, name, OOXML type
//! and placeholders, with geometry inherited from the master where the layout gives none, and the
//! text model compose measures with (C63).

use std::collections::{HashMap, HashSet};

pub use compyla_core::validate::pptx::text::Rect;
use compyla_core::validate::pptx::text::{LayoutText, MAX_COORDINATE, MIN_COORDINATE, TextBox};

use crate::dom::{Element, MAIN_A, MAIN_P, REL_NS};
use crate::template::{Invalid, Template};
use crate::text::{self, PartialBody, PartialLevels, ThemeFonts};

#[derive(Debug, Clone)]
pub struct Placeholder {
    /// ST_PlaceholderType, `obj` when omitted.
    pub kind: String,
    pub index: Option<u32>,
    pub rect: Option<Rect>,
}

impl Placeholder {
    pub fn is_title(&self) -> bool {
        matches!(self.kind.as_str(), "title" | "ctrTitle")
    }

    /// A placeholder content goes into (§8.6): not the title, not a footer, date or number.
    pub fn is_content(&self) -> bool {
        !self.is_title()
            && !matches!(
                self.kind.as_str(),
                "dt" | "ftr" | "sldNum" | "hdr" | "sldImg"
            )
    }
}

#[derive(Debug, Clone)]
pub struct Layout {
    pub part: String,
    pub name: String,
    /// The OOXML type (`p:sldLayout/@type`), which binding falls back to (§8.5).
    pub kind: Option<String>,
    pub placeholders: Vec<Placeholder>,
    /// The master's title and body areas, for a title or content this layout has no placeholder
    /// for (§8.8).
    pub master_title: Option<Rect>,
    pub master_body: Option<Rect>,
    /// What its text is set in (§8.6).
    pub text: LayoutText,
}

impl Layout {
    pub fn title(&self) -> Option<&Placeholder> {
        self.placeholders.iter().find(|p| p.is_title())
    }

    /// The content placeholders, in shape order: slot `i` fills the `i`-th (§8.6).
    pub fn content(&self) -> Vec<&Placeholder> {
        self.placeholders
            .iter()
            .filter(|p| p.is_content())
            .collect()
    }
}

/// The template's layouts in master order, each once.
pub fn read(template: &Template) -> Result<Vec<Layout>, Invalid> {
    let presentation = template.xml(&template.main)?.unwrap_or_default();
    let default_text = text::levels(presentation.child(MAIN_P, "defaultTextStyle"));
    // The table style output uses (§8.8): the template's default, else Medium Style 2. A table
    // style part that can't be read is none, as output treats it.
    let table_styles = template
        .related(&template.main, "tableStyles")?
        .and_then(|part| template.xml(&part).ok().flatten());
    let table_style = table_styles
        .as_ref()
        .and_then(|styles| styles.attr("def"))
        .unwrap_or(text::MEDIUM_STYLE_2)
        .to_string();
    let (header_text, body_text) = text::table_text(table_styles.as_ref(), &table_style);
    let mut layouts = Vec::new();
    let mut seen = HashSet::new();
    for master in listed(
        template,
        &template.main,
        presentation.child(MAIN_P, "sldMasterIdLst"),
        "sldMasterId",
    )? {
        let Some(master_xml) = template.xml(&master)? else {
            continue;
        };
        let theme = template
            .related(&master, "theme")?
            .and_then(|part| template.xml(&part).ok().flatten());
        let fonts = ThemeFonts::of(theme.as_ref());
        let text_styles = master_xml.child(MAIN_P, "txStyles");
        let style = |name: &str| text::levels(text_styles.and_then(|s| s.child(MAIN_P, name)));
        let styles = Styles {
            title: style("titleStyle"),
            body: style("bodyStyle"),
            other: style("otherStyle"),
            default: &default_text,
            fonts: &fonts,
        };
        let master_shapes = shapes(&master_xml);
        let parent = Master::new(&master_shapes);
        let inherited: Vec<Placeholder> = master_shapes
            .iter()
            .map(|shape| shape.placeholder.clone())
            .collect();
        for part in listed(
            template,
            &master,
            master_xml.child(MAIN_P, "sldLayoutIdLst"),
            "sldLayoutId",
        )? {
            if !seen.insert(part.to_ascii_lowercase()) {
                continue;
            }
            let Some(xml) = template.xml(&part)? else {
                continue;
            };
            let name = xml
                .child(MAIN_P, "cSld")
                .and_then(|c| c.attr("name"))
                .unwrap_or_default()
                .to_string();
            let mut own = shapes(&xml);
            for shape in &mut own {
                if shape.placeholder.rect.is_none() {
                    shape.placeholder.rect = parent
                        .of(&shape.placeholder)
                        .and_then(|m| m.placeholder.rect);
                }
            }
            let master_title = master_shapes.iter().find(|s| s.placeholder.is_title());
            let master_body = inherited
                .iter()
                .find(|p| p.kind == "body")
                .and_then(|p| p.rect);
            let boxed = |shape: &Shape| styles.text_box(Some(shape), parent.of(&shape.placeholder));
            let title = match own.iter().find(|s| s.placeholder.is_title()) {
                Some(shape) => boxed(shape),
                None => styles.text_box(master_title, None),
            };
            let content = own
                .iter()
                .filter(|s| s.placeholder.is_content())
                .map(boxed)
                .collect();
            layouts.push(Layout {
                part,
                name,
                kind: xml.attr("type").map(str::to_string),
                master_title: master_title.and_then(|s| s.placeholder.rect),
                master_body,
                placeholders: own.into_iter().map(|s| s.placeholder).collect(),
                text: LayoutText {
                    title,
                    content,
                    master_body,
                    text_box: text::resolve_levels(&[&default_text], &fonts),
                    table_header: text::table_level(
                        &header_text,
                        &styles.other,
                        &default_text,
                        &fonts,
                    ),
                    table_body: text::table_level(&body_text, &styles.other, &default_text, &fonts),
                },
            });
        }
    }
    Ok(layouts)
}

/// A master's text styles, with the presentation's and its theme's fonts.
struct Styles<'a> {
    title: PartialLevels,
    body: PartialLevels,
    other: PartialLevels,
    default: &'a PartialLevels,
    fonts: &'a ThemeFonts,
}

impl Styles<'_> {
    /// The box `shape` sets text in, inheriting from `master`'s placeholder, then the master's
    /// text style for its type, then the presentation's (§8.6).
    fn text_box(&self, shape: Option<&Shape>, master: Option<&Shape>) -> TextBox {
        let placeholder = shape.or(master).map(|s| &s.placeholder);
        let style = match placeholder.map(|p| p.kind.as_str()) {
            Some("title" | "ctrTitle") | None => &self.title,
            Some("dt" | "ftr" | "sldNum" | "hdr") => &self.other,
            Some(_) => &self.body,
        };
        let shapes = [shape, master].into_iter().flatten();
        let bodies: Vec<&PartialBody> = shapes.clone().map(|s| &s.body).collect();
        let mut lists: Vec<&PartialLevels> = shapes.map(|s| &s.levels).collect();
        lists.extend([style, self.default]);
        TextBox {
            rect: placeholder.and_then(|p| p.rect),
            body: text::resolve_body(&bodies),
            levels: text::resolve_levels(&lists, self.fonts),
        }
    }
}

/// A master's placeholders, found by type and by family: each a layout's placeholder inherits
/// from is found at once, however many either has.
struct Master<'a> {
    shapes: &'a [Shape],
    by_kind: HashMap<&'a str, usize>,
    by_family: HashMap<&'static str, usize>,
}

/// A placeholder type's family: title types and body types each share the master's one
/// placeholder; footers only match their own type.
fn family(kind: &str) -> &'static str {
    match kind {
        "title" | "ctrTitle" => "title",
        "dt" | "ftr" | "sldNum" | "hdr" => "footer",
        _ => "body",
    }
}

impl<'a> Master<'a> {
    fn new(shapes: &'a [Shape]) -> Master<'a> {
        let mut by_kind = HashMap::new();
        let mut by_family = HashMap::new();
        for (i, shape) in shapes.iter().enumerate() {
            let kind = shape.placeholder.kind.as_str();
            by_kind.entry(kind).or_insert(i);
            if family(kind) != "footer" {
                by_family.entry(family(kind)).or_insert(i);
            }
        }
        Master {
            shapes,
            by_kind,
            by_family,
        }
    }

    /// The master placeholder `placeholder` inherits from: the first of its type, else the first
    /// of its family (§8.6).
    fn of(&self, placeholder: &Placeholder) -> Option<&'a Shape> {
        let kind = placeholder.kind.as_str();
        self.by_kind
            .get(kind)
            .or_else(|| {
                (family(kind) != "footer")
                    .then(|| self.by_family.get(family(kind)))
                    .flatten()
            })
            .map(|&i| &self.shapes[i])
    }
}

/// The parts a list of ids names through `source`'s relationships, each once.
fn listed(
    template: &Template,
    source: &str,
    list: Option<&Element>,
    item: &str,
) -> Result<Vec<String>, Invalid> {
    let targets: HashMap<String, String> = template
        .relationships(source)?
        .into_iter()
        .filter(|r| !r.external)
        .map(|r| (r.id, r.target))
        .collect();
    let mut seen = HashSet::new();
    Ok(list
        .into_iter()
        .flat_map(|list| list.children_named(MAIN_P, item))
        .filter_map(|id| targets.get(id.attr_ns(REL_NS, "id")?))
        .filter(|target| seen.insert(target.to_ascii_lowercase()))
        .cloned()
        .collect())
}

/// A placeholder shape: its type, index and geometry, and the body properties and list style its
/// text body sets.
struct Shape {
    placeholder: Placeholder,
    body: PartialBody,
    levels: PartialLevels,
}

fn shapes(part: &Element) -> Vec<Shape> {
    let Some(tree) = part.path(&[(MAIN_P, "cSld"), (MAIN_P, "spTree")]) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for shape in &tree.children {
        if shape.namespace != MAIN_P
            || !matches!(shape.name.as_str(), "sp" | "pic" | "graphicFrame")
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
        let Some(ph) = properties.path(&[(MAIN_P, "nvPr"), (MAIN_P, "ph")]) else {
            continue;
        };
        let rect = shape
            .child(MAIN_P, "spPr")
            .or_else(|| shape.child(MAIN_P, "xfrm"))
            .and_then(|pr| {
                if pr.is(MAIN_P, "xfrm") {
                    Some(pr)
                } else {
                    pr.child(MAIN_A, "xfrm")
                }
            })
            .and_then(rect);
        let text_body = shape.child(MAIN_P, "txBody");
        out.push(Shape {
            placeholder: Placeholder {
                kind: ph.attr("type").unwrap_or("obj").to_string(),
                index: ph.attr("idx").and_then(|i| i.parse().ok()),
                rect,
            },
            body: text::body(text_body.and_then(|t| t.child(MAIN_A, "bodyPr"))),
            levels: text::levels(text_body.and_then(|t| t.child(MAIN_A, "lstStyle"))),
        });
    }
    out
}

fn rect(xfrm: &Element) -> Option<Rect> {
    let off = xfrm.child(MAIN_A, "off")?;
    let ext = xfrm.child(MAIN_A, "ext")?;
    // Parsed wider than the bounds, so a value past them is held to them rather than dropped.
    let number = |e: &Element, a: &str, min: i64| {
        let value = e.attr(a)?.parse::<i128>().ok()?;
        i64::try_from(value.clamp(i128::from(min), i128::from(MAX_COORDINATE))).ok()
    };
    Some(Rect {
        x: number(off, "x", MIN_COORDINATE)?,
        y: number(off, "y", MIN_COORDINATE)?,
        cx: number(ext, "cx", 0)?,
        cy: number(ext, "cy", 0)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use compyla_core::validate::pptx::text::{Autofit, Body, Bullet, Insets, Spacing};

    fn template(bytes: &[u8]) -> Template {
        Template::read(bytes).unwrap()
    }

    /// `package` with `edit` applied to its parts.
    fn edited(package: &[u8], edit: impl Fn(&str, String) -> String) -> Vec<u8> {
        let entries: Vec<(String, Vec<u8>)> = crate::read::entries(package)
            .unwrap()
            .into_iter()
            .map(|(name, data)| match String::from_utf8(data) {
                Ok(text) => {
                    let text = edit(&name, text);
                    (name, text.into_bytes())
                }
                Err(bytes) => (name, bytes.into_bytes()),
            })
            .collect();
        crate::zip::write(&entries)
    }

    #[test]
    fn the_house_templates_text_is_its_master_styles() {
        let layouts = read(&template(include_bytes!(
            "../../compyla-pptx/house/house.potx"
        )))
        .unwrap();
        let layout = |name: &str| &layouts.iter().find(|l| l.name == name).unwrap().text;
        let content = layout("Title and Content");
        let body = &content.content[0];
        assert_eq!(body.body, Body::default());
        let first = &body.levels[0];
        assert_eq!((first.margin, first.indent), (228_600, -228_600));
        assert_eq!(first.line_spacing, Spacing::Percent(90_000));
        assert_eq!(first.space_before, Spacing::Points(1_000));
        assert_eq!(first.space_after, Spacing::Points(0));
        assert_eq!(first.bullet, Bullet::Char("\u{2022}".into()));
        assert_eq!(first.bullet_font.as_deref(), Some("Arial"));
        assert_eq!(
            (first.run.size, first.run.latin.as_str(), first.run.kern),
            (2_800, "Calibri", 1_200)
        );
        assert_eq!(
            (body.levels[1].run.size, body.levels[1].margin),
            (2_400, 685_800)
        );
        assert_eq!(body.levels[3].run.size, 1_800);
        // Titles take the master's title style in the theme's major font.
        let title = &content.title.levels[0];
        assert_eq!(
            (title.run.size, title.run.latin.as_str()),
            (4_000, "Calibri Light")
        );
        assert_eq!(
            content.title.rect,
            Some(Rect {
                x: 838_200,
                y: 365_125,
                cx: 10_515_600,
                cy: 1_325_563
            })
        );
        // A subtitle has no bullet; Blank has no title of its own, so its title is the master's.
        let cover = layout("Title Slide");
        assert_eq!(cover.title.levels[0].run.size, 6_000);
        let subtitle = &cover.content[0].levels[0];
        assert_eq!((subtitle.run.size, subtitle.margin), (2_400, 0));
        assert_eq!(subtitle.bullet, Bullet::None);
        let blank = layout("Blank");
        assert!(blank.content.is_empty());
        assert_eq!(blank.title.rect, content.title.rect);
        // Nothing in the house chain sizes text boxes or table cells: 18 pt, the header bold.
        assert_eq!(content.text_box[0].run.size, 1_800);
        assert_eq!(content.text_box[0].run.latin, "Calibri");
        assert!(content.table_header.run.bold && !content.table_body.run.bold);
        assert_eq!(content.table_body.run.size, 1_800);
    }

    #[test]
    fn each_property_comes_from_the_first_that_sets_it() {
        let brand: &[u8] =
            include_bytes!("../../../spec/conformance/lint/139-pptx-template-binding/brand.potx");
        let body = concat!(
            r#"<p:sp><p:nvSpPr><p:cNvPr id="7" name="Body"/><p:cNvSpPr/><p:nvPr><p:ph type="body" idx="1"/></p:nvPr></p:nvSpPr>"#,
            r#"<p:spPr><a:xfrm><a:off x="1" y="2"/><a:ext cx="3" cy="4"/></a:xfrm></p:spPr>"#,
            r#"<p:txBody><a:bodyPr lIns="0" tIns="0"><a:normAutofit/></a:bodyPr>"#,
            r#"<a:lstStyle><a:lvl1pPr marL="100"><a:lnSpc><a:spcPct val="80%"/></a:lnSpc><a:defRPr sz="3000"/></a:lvl1pPr></a:lstStyle><a:p/></p:txBody></p:sp>"#,
        );
        let template = template(&edited(brand, |name, xml| match name {
            "ppt/slideMasters/slideMaster1.xml" => {
                xml.replacen("</p:spTree>", &format!("{body}</p:spTree>"), 1)
            }
            // The content placeholder of Titel und Inhalt: its own top inset, no autofit, and a
            // margin over the master's; its `a:defPPr` doesn't count, so the master's size does.
            "ppt/slideLayouts/slideLayout2.xml" => {
                let at = xml.find(r#"name="Inhaltsplatzhalter 2""#).unwrap();
                let (head, tail) = xml.split_at(at);
                let tail = tail.replacen(
                    "<a:bodyPr/>",
                    concat!(
                        r#"<a:bodyPr tIns="7"><a:noAutofit/></a:bodyPr><a:lstStyle>"#,
                        r#"<a:defPPr><a:defRPr sz="9900"/></a:defPPr><a:lvl1pPr marL="555"/></a:lstStyle>"#
                    ),
                    1,
                );
                format!("{head}{tail}")
            }
            _ => xml,
        }));
        let layouts = read(&template).unwrap();
        let layout = |name: &str| &layouts.iter().find(|l| l.name == name).unwrap().text;

        let content = &layout("Titel und Inhalt").content[0];
        assert_eq!(
            content.rect,
            Some(Rect {
                x: 1,
                y: 2,
                cx: 3,
                cy: 4
            })
        );
        assert_eq!(
            content.body.insets,
            Insets {
                left: 0,
                top: 7,
                ..Insets::default()
            }
        );
        assert_eq!(content.body.autofit, Autofit::None);
        let first = &content.levels[0];
        assert_eq!((first.run.size, first.margin), (3_000, 555));
        assert_eq!(first.line_spacing, Spacing::Percent(80_000));
        // The master's body style names the font.
        assert_eq!(first.run.latin, "Source Serif 4");

        // The subtitle has nothing of its own: all the master's body placeholder, which shrinks.
        let subtitle = &layout("Title Slide").content[0];
        assert_eq!(subtitle.levels[0].run.size, 3_000);
        assert_eq!(subtitle.body.autofit, Autofit::Shrink);
        assert_eq!(subtitle.body.insets.top, 0);

        // Theme references resolve through the master's theme: Brand Display and Brand Sans.
        let titled = layout("Titel und Inhalt");
        assert_eq!(titled.title.levels[0].run.latin, "Brand Display");
        assert_eq!(titled.text_box[0].run.latin, "Brand Sans");
        // No table style part: Medium Style 2, in the minor font, its header bold.
        assert_eq!(titled.table_header.run.latin, "Brand Sans");
        assert!(titled.table_header.run.bold && !titled.table_body.run.bold);
    }

    #[test]
    fn a_templates_table_style_and_default_text_reach_tables_and_text_boxes() {
        // lint/144: its table style's whole table names Brand Table; its default text style,
        // Brand Default.
        let layouts = read(&template(include_bytes!(
            "../../../spec/conformance/lint/144-pptx-fonts-unresolved/brand.potx"
        )))
        .unwrap();
        let text = &layouts[0].text;
        assert_eq!(text.table_body.run.latin, "Brand Table");
        assert_eq!(text.table_header.run.latin, "Brand Table");
        assert_eq!(text.text_box[0].run.latin, "Brand Default");
    }

    #[test]
    fn geometry_is_held_to_the_schemas_bounds() {
        let xfrm = crate::dom::parse(
            br#"<a:xfrm xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><a:off x="92233720368547758070" y="-9223372036854775808"/><a:ext cx="-5" cy="9223372036854775807"/></a:xfrm>"#,
        )
        .unwrap();
        assert_eq!(
            rect(&xfrm),
            Some(Rect {
                x: MAX_COORDINATE,
                y: MIN_COORDINATE,
                cx: 0,
                cy: MAX_COORDINATE,
            })
        );
    }
}
