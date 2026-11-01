//! A template's slide layouts (markdown-spec.md §8.5, §8.6): each layout's part, master, name,
//! OOXML type and placeholders, with geometry inherited from the master where the layout gives
//! none. What their text is set in is `text::Model`'s, read only for the boxes a deck uses (C63).

use std::collections::{HashMap, HashSet};

pub use compyla_core::validate::pptx::text::Rect;
use compyla_core::validate::pptx::text::{MAX_COORDINATE, MIN_COORDINATE};

use crate::dom::{Element, MAIN_A, MAIN_P, REL_NS};
use crate::template::{Invalid, Template};

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
    /// The slide master it belongs to.
    pub master: String,
    pub name: String,
    /// The OOXML type (`p:sldLayout/@type`), which binding falls back to (§8.5).
    pub kind: Option<String>,
    pub placeholders: Vec<Placeholder>,
    /// The master's title and body areas, for a title or content this layout has no placeholder
    /// for (§8.8).
    pub master_title: Option<Rect>,
    pub master_body: Option<Rect>,
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
        let inherited: Vec<Placeholder> = placeholders(&master_xml)
            .into_iter()
            .map(|(_, placeholder)| placeholder)
            .collect();
        let index = Index::new(inherited.iter().map(|p| p.kind.as_str()));
        let master_title = inherited.iter().find(|p| p.is_title()).and_then(|p| p.rect);
        let master_body = inherited
            .iter()
            .find(|p| p.kind == "body")
            .and_then(|p| p.rect);
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
            let mut own: Vec<Placeholder> = placeholders(&xml)
                .into_iter()
                .map(|(_, placeholder)| placeholder)
                .collect();
            for placeholder in &mut own {
                if placeholder.rect.is_none() {
                    placeholder.rect = index.of(&placeholder.kind).and_then(|i| inherited[i].rect);
                }
            }
            layouts.push(Layout {
                part,
                master: master.clone(),
                name,
                kind: xml.attr("type").map(str::to_string),
                placeholders: own,
                master_title,
                master_body,
            });
        }
    }
    Ok(layouts)
}

/// A master's placeholders by type and by family, so the one each of a layout's inherits from is
/// found at once, however many either has (§8.6).
pub(crate) struct Index {
    by_kind: HashMap<String, usize>,
    by_family: HashMap<&'static str, usize>,
}

/// A placeholder type's family: title types share the master's title placeholder and content
/// types its body placeholder; dates, footers, headers and numbers only match their own type.
pub(crate) fn family(kind: &str) -> &'static str {
    match kind {
        "title" | "ctrTitle" => "title",
        "dt" | "ftr" | "sldNum" | "hdr" => "footer",
        _ => "body",
    }
}

impl Index {
    /// The index of the master's placeholders, by their types in shape order.
    pub(crate) fn new<'a>(kinds: impl IntoIterator<Item = &'a str>) -> Index {
        let mut by_kind = HashMap::new();
        let mut by_family = HashMap::new();
        for (i, kind) in kinds.into_iter().enumerate() {
            by_kind.entry(kind.to_string()).or_insert(i);
            if family(kind) != "footer" {
                by_family.entry(family(kind)).or_insert(i);
            }
        }
        Index { by_kind, by_family }
    }

    /// The position of the master placeholder one of type `kind` inherits from: the first of its
    /// type, else the first of its family.
    pub(crate) fn of(&self, kind: &str) -> Option<usize> {
        self.by_kind.get(kind).copied().or_else(|| {
            (family(kind) != "footer")
                .then(|| self.by_family.get(family(kind)).copied())
                .flatten()
        })
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

/// A part's shape tree (`p:cSld/p:spTree`).
pub(crate) fn tree(part: &Element) -> Option<&Element> {
    part.path(&[(MAIN_P, "cSld"), (MAIN_P, "spTree")])
}

/// A part's placeholders in shape order, each with its shape's position in the shape tree.
pub(crate) fn placeholders(part: &Element) -> Vec<(usize, Placeholder)> {
    let Some(tree) = tree(part) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (position, shape) in tree.children.iter().enumerate() {
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
        out.push((
            position,
            Placeholder {
                kind: ph.attr("type").unwrap_or("obj").to_string(),
                index: ph.attr("idx").and_then(|i| i.parse().ok()),
                rect,
            },
        ));
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

    #[test]
    fn a_placeholder_inherits_from_the_first_of_its_type_else_of_its_family() {
        let index = Index::new(["dt", "obj", "title", "body", "pic", "ctrTitle", "body"]);
        // Its own type first, the first of it where there are several.
        assert_eq!(index.of("pic"), Some(4));
        assert_eq!(index.of("ctrTitle"), Some(5));
        assert_eq!(index.of("body"), Some(3));
        // Else the first of its family, whatever its type.
        assert_eq!(index.of("tbl"), Some(1));
        assert_eq!(index.of("subTitle"), Some(1));
        // Dates, footers, headers and numbers only by type.
        assert_eq!(index.of("dt"), Some(0));
        assert_eq!(index.of("ftr"), None);
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
