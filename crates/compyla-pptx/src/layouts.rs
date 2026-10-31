//! A template's slide layouts (markdown-spec.md §8.5, §8.6): each layout's part, name, OOXML type
//! and placeholders, with geometry inherited from the master where the layout gives none.

use std::collections::{HashMap, HashSet};

use compyla_ooxml::dom::{Element, MAIN_A, MAIN_P, REL_NS};
use compyla_ooxml::template::{Invalid, Template};

/// A rectangle in EMU.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i64,
    pub y: i64,
    pub cx: i64,
    pub cy: i64,
}

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
    pub placeholders: Vec<Placeholder>,
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
        let inherited = placeholders(&master_xml);
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
            let mut own = placeholders(&xml);
            for placeholder in &mut own {
                if placeholder.rect.is_none() {
                    placeholder.rect = inherit(placeholder, &inherited);
                }
            }
            layouts.push(Layout {
                part,
                name,
                placeholders: own,
            });
        }
    }
    Ok(layouts)
}

/// A master's placeholder the layout's inherits from: by type, where title types and body types
/// each share the master's one placeholder.
fn inherit(placeholder: &Placeholder, master: &[Placeholder]) -> Option<Rect> {
    let family = |kind: &str| match kind {
        "title" | "ctrTitle" => "title",
        "dt" | "ftr" | "sldNum" | "hdr" => "footer",
        _ => "body",
    };
    master
        .iter()
        .find(|m| m.kind == placeholder.kind)
        .or_else(|| {
            master.iter().find(|m| {
                family(&m.kind) == family(&placeholder.kind) && family(&m.kind) != "footer"
            })
        })
        .and_then(|m| m.rect)
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

fn placeholders(part: &Element) -> Vec<Placeholder> {
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
        out.push(Placeholder {
            kind: ph.attr("type").unwrap_or("obj").to_string(),
            index: ph.attr("idx").and_then(|i| i.parse().ok()),
            rect,
        });
    }
    out
}

fn rect(xfrm: &Element) -> Option<Rect> {
    let off = xfrm.child(MAIN_A, "off")?;
    let ext = xfrm.child(MAIN_A, "ext")?;
    let number = |e: &Element, a: &str| e.attr(a)?.parse::<i64>().ok();
    Some(Rect {
        x: number(off, "x")?,
        y: number(off, "y")?,
        cx: number(ext, "cx")?.max(0),
        cy: number(ext, "cy")?.max(0),
    })
}
