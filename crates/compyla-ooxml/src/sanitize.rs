//! Active and external content removed from the template parts an output keeps (markdown-spec.md
//! §10.4, C58): relationships with an external target other than hyperlinks, OLE objects and
//! ActiveX controls, together with what refers to them. A part with nothing to remove stays byte
//! for byte; in one with something, everything else stays as it was.

use std::collections::{HashMap, HashSet};
use std::ops::Range;

use quick_xml::NsReader;
use quick_xml::events::Event;
use quick_xml::name::ResolveResult;

use crate::dom::{self, Error, MAIN_A, MAIN_P, PACKAGE_RELS, REL_NS};
use crate::template::{self, Invalid, Relationship, Template};

const MC: &str = "http://schemas.openxmlformats.org/markup-compatibility/2006";
/// VML names relationships with `o:relid` as well as `r:id`.
const VML_OFFICE: &str = "urn:schemas-microsoft-com:office:office";

/// Something removed from a part: each is one `template_content_removed`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Removed {
    /// An OLE object, embedded or linked.
    OleObject,
    /// An ActiveX control.
    ActiveX,
    /// A relationship with an external target, other than a hyperlink: its type and target.
    External { kind: String, target: String },
}

/// A template part without its active and external content.
#[derive(Debug, Clone)]
pub struct Part {
    /// The relationships it keeps.
    pub relationships: Vec<Relationship>,
    /// Its new bytes, when something in it was removed.
    pub xml: Option<Vec<u8>>,
    /// Its relationships part's new bytes, when a relationship was removed.
    pub rels: Option<Vec<u8>>,
    /// What was removed: objects in document order, then relationships in their order.
    pub removed: Vec<Removed>,
}

/// `name` from `template`, without active or external content (§10.4).
pub fn part(template: &Template, name: &str) -> Result<Part, Invalid> {
    let relationships = template.relationships(name)?;
    let unchanged = |relationships| {
        Ok(Part {
            relationships,
            xml: None,
            rels: None,
            removed: Vec::new(),
        })
    };
    // Without relationships, nothing in a part can refer to anything outside it.
    if relationships.is_empty() {
        return unchanged(relationships);
    }
    let removable: HashSet<&str> = relationships
        .iter()
        .filter(|r| removable(r))
        .map(|r| r.id.as_str())
        .collect();
    let text = if is_xml(template, name) {
        let data = template.part(name).unwrap_or_default();
        Some(dom::utf8_document(data).map_err(|error| invalid(name, error))?)
    } else {
        None
    };
    let objects_named = text
        .as_deref()
        .is_some_and(|text| contains(text, b"oleObj") || contains(text, b"control"));
    if removable.is_empty() && !objects_named {
        return unchanged(relationships);
    }
    let nodes = match &text {
        Some(text) => nodes(text).map_err(|error| invalid(name, error))?,
        None => Vec::new(),
    };

    // Every pass below is linear: a hostile part may have as many elements as §12.1 allows.
    let extents = Extents::new(&nodes);

    // OLE objects (the graphic frame holding one) and ActiveX controls, each once.
    let mut objects: Vec<(usize, Removed)> = Vec::new();
    let mut roots: HashSet<usize> = HashSet::new();
    for (index, node) in nodes.iter().enumerate() {
        let object = if node.is(MAIN_P, "oleObj") {
            let frame = extents.frame[index].unwrap_or(index);
            Some((extents.of(frame), Removed::OleObject))
        } else if node.is(MAIN_P, "control") {
            Some((extents.of(index), Removed::ActiveX))
        } else {
            None
        };
        if let Some((root, what)) = object
            && roots.insert(root)
        {
            if nodes[root].parent.is_none() {
                return Err(Invalid(format!(
                    "has an OLE object or ActiveX control as the whole of `{name}`"
                )));
            }
            objects.push((root, what));
        }
    }
    let in_object = mark(&nodes, &roots);

    // What refers to a removed relationship: a picture's blip loses the attribute naming it (so
    // does the root, which can't go); anything else is removed.
    let mut strip: HashMap<usize, Vec<String>> = HashMap::new();
    for (index, node) in nodes.iter().enumerate() {
        if in_object[index] {
            continue;
        }
        let named: Vec<String> = node
            .refs
            .iter()
            .filter(|(_, id)| removable.contains(id.as_str()))
            .map(|(key, _)| key.clone())
            .collect();
        if named.is_empty() {
            continue;
        }
        let root = extents.of(index);
        if node.is(MAIN_A, "blip") || nodes[root].parent.is_none() {
            strip.insert(index, named);
        } else {
            roots.insert(root);
        }
    }

    // Animations of a removed shape go too.
    let inside = mark(&nodes, &roots);
    let shapes: HashSet<&str> = nodes
        .iter()
        .enumerate()
        .filter(|(index, node)| inside[*index] && node.name == "cNvPr")
        .filter_map(|(_, node)| node.key.as_deref())
        .collect();
    for (index, node) in nodes.iter().enumerate() {
        if !inside[index]
            && node.is(MAIN_P, "spTgt")
            && node.key.as_deref().is_some_and(|id| shapes.contains(id))
        {
            roots.insert(extents.of(index));
        }
    }

    // A control list left empty goes too.
    let inside = mark(&nodes, &roots);
    let (mut children, mut gone) = (vec![0usize; nodes.len()], vec![0usize; nodes.len()]);
    for (index, node) in nodes.iter().enumerate() {
        if let Some(parent) = node.parent {
            children[parent] += 1;
            gone[parent] += usize::from(inside[index]);
        }
    }
    for (index, node) in nodes.iter().enumerate() {
        if node.is(MAIN_P, "controls")
            && !inside[index]
            && children[index] > 0
            && gone[index] == children[index]
        {
            roots.insert(index);
        }
    }
    let inside = mark(&nodes, &roots);
    strip.retain(|index, _| !inside[*index]);

    // Relationships only removed content used go with it.
    let (mut within, mut outside) = (HashSet::new(), HashSet::new());
    // Whether every reference to a relationship is inside an object, which reports it.
    let mut objects_own: HashMap<&str, bool> = HashMap::new();
    for (index, node) in nodes.iter().enumerate() {
        let stripped = strip.get(&index).map_or(&[][..], Vec::as_slice);
        for (key, id) in &node.refs {
            if inside[index] {
                within.insert(id.as_str());
            } else if !stripped.contains(key) {
                outside.insert(id.as_str());
            }
            *objects_own.entry(id.as_str()).or_insert(true) &= in_object[index];
        }
    }
    let gone_ids: HashSet<String> = removable
        .iter()
        .copied()
        .chain(within.difference(&outside).copied())
        .map(str::to_string)
        .collect();

    // One report per object, and one per removed relationship that isn't an object's.
    let mut removed: Vec<Removed> = objects
        .iter()
        .filter(|(root, _)| !nodes[*root].parent.is_some_and(|p| in_object[p]))
        .map(|(_, what)| what.clone())
        .collect();
    for relationship in &relationships {
        if !removable.contains(relationship.id.as_str())
            || objects_own.get(relationship.id.as_str()) == Some(&true)
        {
            continue;
        }
        let kind = suffix(&relationship.kind);
        removed.push(if kind.eq_ignore_ascii_case("oleObject") {
            Removed::OleObject
        } else if kind.eq_ignore_ascii_case("control") {
            Removed::ActiveX
        } else {
            Removed::External {
                kind: kind.to_string(),
                target: relationship.target.clone(),
            }
        });
    }

    let xml = match &text {
        Some(text) if !roots.is_empty() || !strip.is_empty() => {
            let mut edits: Vec<(Range<usize>, Vec<u8>)> = roots
                .iter()
                .filter(|&&root| !nodes[root].parent.is_some_and(|p| inside[p]))
                .map(|&root| (nodes[root].span.clone(), Vec::new()))
                .collect();
            for (index, keys) in &strip {
                let open = nodes[*index].open.clone();
                let tag = without_attributes(&text[open.clone()], keys)
                    .map_err(|error| invalid(name, error))?;
                edits.push((open, tag));
            }
            Some(splice(text, edits))
        }
        _ => None,
    };
    let rels = if gone_ids.is_empty() {
        None
    } else {
        let part = template::relationships_part(name);
        let data = dom::utf8_document(template.part(&part).unwrap_or_default())
            .map_err(|error| invalid(&part, error))?;
        let edits = nodes_of(&data, &part)?
            .iter()
            .filter(|node| node.is(PACKAGE_RELS, "Relationship"))
            .filter(|node| node.key.as_ref().is_some_and(|id| gone_ids.contains(id)))
            .map(|node| (node.span.clone(), Vec::new()))
            .collect();
        Some(splice(&data, edits))
    };
    Ok(Part {
        relationships: relationships
            .into_iter()
            .filter(|r| !gone_ids.contains(&r.id))
            .collect(),
        xml,
        rels,
        removed,
    })
}

/// A relationship §10.4 removes: an external target other than a hyperlink, or an OLE object or
/// ActiveX control wherever it is. Types compare without case, by their last segment, so the
/// Strict namespace counts too.
fn removable(relationship: &Relationship) -> bool {
    let kind = suffix(&relationship.kind);
    (relationship.external && !kind.eq_ignore_ascii_case("hyperlink"))
        || kind.eq_ignore_ascii_case("oleObject")
        || kind.eq_ignore_ascii_case("control")
}

fn suffix(kind: &str) -> &str {
    kind.rsplit('/').next().unwrap_or_default()
}

fn is_xml(template: &Template, name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".xml")
        || lower.ends_with(".vml")
        || template.content_type(name).is_some_and(|t| {
            t.to_ascii_lowercase()
                .split(';')
                .next()
                .unwrap_or("")
                .trim()
                .ends_with("xml")
        })
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

fn invalid(name: &str, error: Error) -> Invalid {
    match error {
        Error::Dtd => Invalid(format!("has a DTD in `{name}`")),
        Error::TooLarge => Invalid(format!(
            "has more than {} elements in `{name}`",
            dom::MAX_ELEMENTS
        )),
        Error::Malformed(why) => Invalid(format!("has malformed XML in `{name}`: {why}")),
    }
}

/// An element of a part, in document order: every parent comes before its children.
struct Node {
    namespace: String,
    name: String,
    /// The whole element, and its start tag.
    span: Range<usize>,
    open: Range<usize>,
    parent: Option<usize>,
    /// The attributes naming a relationship: each one's qualified name, and the id.
    refs: Vec<(String, String)>,
    /// A shape's id (`cNvPr`), an animation target's (`p:spTgt`), or a relationship's (`Id`).
    key: Option<String>,
}

impl Node {
    fn is(&self, namespace: &str, name: &str) -> bool {
        self.namespace == namespace && self.name == name
    }
}

fn nodes_of(text: &[u8], name: &str) -> Result<Vec<Node>, Invalid> {
    nodes(text).map_err(|error| invalid(name, error))
}

/// A part's elements, with the limits `dom` holds every part to (§12.1).
fn nodes(text: &[u8]) -> Result<Vec<Node>, Error> {
    let mut reader = NsReader::from_reader(text);
    reader.config_mut().trim_text(false);
    reader
        .resolver_mut()
        .set_max_namespace_bindings(dom::MAX_BINDINGS);
    let mut nodes: Vec<Node> = Vec::new();
    let mut open: Vec<usize> = Vec::new();
    let mut buffer = Vec::new();
    loop {
        let before = reader.buffer_position() as usize;
        let event = reader
            .read_event_into(&mut buffer)
            .map_err(|e| Error::Malformed(e.to_string()))?;
        let after = reader.buffer_position() as usize;
        match &event {
            Event::Start(start) | Event::Empty(start) => {
                if nodes.len() >= dom::MAX_ELEMENTS {
                    return Err(Error::TooLarge);
                }
                if open.len() >= dom::MAX_DEPTH {
                    return Err(Error::Malformed(format!(
                        "nests elements deeper than {} levels",
                        dom::MAX_DEPTH
                    )));
                }
                let (namespace, name) = dom::names(&reader, start);
                let mut refs = Vec::new();
                let mut key = None;
                for attribute in start.attributes() {
                    let attribute = attribute.map_err(|e| Error::Malformed(e.to_string()))?;
                    let qualified = attribute.key.as_ref();
                    if qualified.starts_with("xmlns") {
                        continue;
                    }
                    let (resolved, local) = reader.resolver().resolve_attribute(attribute.key);
                    let attribute_namespace = match resolved {
                        ResolveResult::Bound(ns) => ns.as_ref().to_string(),
                        _ => String::new(),
                    };
                    let local = local.as_ref();
                    let value = || -> Result<String, Error> {
                        Ok(attribute
                            .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                            .map_err(|e| Error::Malformed(e.to_string()))?
                            .into_owned())
                    };
                    if attribute_namespace == REL_NS
                        || (attribute_namespace == VML_OFFICE && local == "relid")
                    {
                        refs.push((qualified.to_string(), value()?));
                    } else if attribute_namespace.is_empty()
                        && matches!(
                            (namespace.as_str(), name.as_str(), local),
                            (_, "cNvPr", "id")
                                | (MAIN_P, "spTgt", "spid")
                                | (PACKAGE_RELS, "Relationship", "Id")
                        )
                    {
                        key = Some(value()?);
                    }
                }
                nodes.push(Node {
                    namespace,
                    name,
                    span: before..after,
                    open: before..after,
                    parent: open.last().copied(),
                    refs,
                    key,
                });
                if matches!(event, Event::Start(_)) {
                    open.push(nodes.len() - 1);
                }
            }
            Event::End(_) => {
                let index = open
                    .pop()
                    .ok_or_else(|| Error::Malformed("unbalanced end tag".into()))?;
                nodes[index].span.end = after;
            }
            Event::DocType(_) => return Err(Error::Dtd),
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }
    if !open.is_empty() {
        return Err(Error::Malformed("unclosed element".into()));
    }
    Ok(nodes)
}

/// What removing each element takes with it, found in one pass (parents come before children):
/// the animations or transition it is part of; then each `mc:AlternateContent` it is a branch of,
/// whose other branches are the same content, and each extension it is in.
struct Extents {
    /// The nearest `p:timing` or `p:transition`, the element included.
    animation: Vec<Option<usize>>,
    /// The nearest `p:graphicFrame`, the element included.
    frame: Vec<Option<usize>>,
    /// What an element takes with it through alternates and extensions.
    climb: Vec<usize>,
}

impl Extents {
    fn new(nodes: &[Node]) -> Extents {
        let mut extents = Extents {
            animation: vec![None; nodes.len()],
            frame: vec![None; nodes.len()],
            climb: vec![0; nodes.len()],
        };
        for (index, node) in nodes.iter().enumerate() {
            let parent = node.parent;
            extents.animation[index] = if node.is(MAIN_P, "timing") || node.is(MAIN_P, "transition")
            {
                Some(index)
            } else {
                parent.and_then(|p| extents.animation[p])
            };
            extents.frame[index] = if node.is(MAIN_P, "graphicFrame") {
                Some(index)
            } else {
                parent.and_then(|p| extents.frame[p])
            };
            extents.climb[index] = match parent {
                Some(p) if nodes[p].is(MC, "Choice") || nodes[p].is(MC, "Fallback") => nodes[p]
                    .parent
                    .map_or(index, |alternate| extents.climb[alternate]),
                Some(p) if nodes[p].is(MAIN_P, "ext") || nodes[p].is(MAIN_A, "ext") => {
                    extents.climb[p]
                }
                _ => index,
            };
        }
        extents
    }

    /// What removing `index` takes with it.
    fn of(&self, index: usize) -> usize {
        self.climb[self.animation[index].unwrap_or(index)]
    }
}

/// Which elements are `roots` or inside one.
fn mark(nodes: &[Node], roots: &HashSet<usize>) -> Vec<bool> {
    let mut inside = vec![false; nodes.len()];
    for (index, node) in nodes.iter().enumerate() {
        inside[index] = roots.contains(&index) || node.parent.is_some_and(|p| inside[p]);
    }
    inside
}

/// A start tag without the attributes `keys` names; the others keep their order and text.
fn without_attributes(tag: &[u8], keys: &[String]) -> Result<Vec<u8>, Error> {
    let mut reader = quick_xml::Reader::from_reader(tag);
    let malformed = |e: &dyn std::fmt::Display| Error::Malformed(e.to_string());
    let event = reader.read_event().map_err(|e| malformed(&e))?;
    let (start, empty) = match &event {
        Event::Start(start) => (start, false),
        Event::Empty(start) => (start, true),
        _ => return Err(Error::Malformed("not a start tag".into())),
    };
    let mut out = format!("<{}", start.name().as_ref()).into_bytes();
    for attribute in start.attributes() {
        let attribute = attribute.map_err(|e| malformed(&e))?;
        let key = attribute.key.as_ref();
        if keys.iter().any(|k| k == key) {
            continue;
        }
        // The value as written, escaped; quoted as it can be, since it came quoted.
        let value = attribute.value.as_ref();
        let quote = if value.contains('"') { '\'' } else { '"' };
        out.extend(format!(" {key}={quote}{value}{quote}").into_bytes());
    }
    out.extend(if empty { &b"/>"[..] } else { &b">"[..] });
    Ok(out)
}

/// `text` with each range replaced; the ranges don't overlap.
fn splice(text: &[u8], mut edits: Vec<(Range<usize>, Vec<u8>)>) -> Vec<u8> {
    edits.sort_by_key(|(range, _)| range.start);
    let mut out = Vec::with_capacity(text.len());
    let mut cursor = 0;
    for (range, replacement) in edits {
        out.extend(&text[cursor..range.start]);
        out.extend(replacement);
        cursor = range.end;
    }
    out.extend(&text[cursor..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const NS: &str = r#"xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006""#;
    const LAYOUT: &str = "ppt/slideLayouts/slideLayout1.xml";
    const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

    /// A package whose layout is `xml` with `rels` (each id, type's last segment, target, external).
    fn template(xml: &[u8], rels: &[(&str, &str, &str, bool)]) -> Template {
        let relationships: String = rels
            .iter()
            .map(|(id, kind, target, external)| {
                let mode = if *external {
                    r#" TargetMode="External""#
                } else {
                    ""
                };
                format!(r#"<Relationship Id="{id}" Type="{R}/{kind}" Target="{target}"{mode}/>"#)
            })
            .collect();
        let entries: Vec<(String, Vec<u8>)> = [
            ("[Content_Types].xml", r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/ppt/presentation.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"/></Types>"#.as_bytes().to_vec()),
            ("_rels/.rels", format!(r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{R}/officeDocument" Target="ppt/presentation.xml"/></Relationships>"#).into_bytes()),
            ("ppt/presentation.xml", format!("<p:presentation {NS}/>").into_bytes()),
            (LAYOUT, xml.to_vec()),
            ("ppt/slideLayouts/_rels/slideLayout1.xml.rels", format!(r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{relationships}</Relationships>"#).into_bytes()),
        ]
        .into_iter()
        .map(|(name, data)| (name.to_string(), data))
        .collect();
        Template::read(&crate::zip::write(&entries)).unwrap()
    }

    fn layout(tree: &str, after: &str) -> String {
        format!(
            r#"<p:sldLayout {NS}><p:cSld><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>{tree}</p:spTree>{after}</p:cSld></p:sldLayout>"#
        )
    }

    fn ids(part: &Part) -> Vec<&str> {
        part.relationships.iter().map(|r| r.id.as_str()).collect()
    }

    fn text(bytes: &Option<Vec<u8>>) -> String {
        String::from_utf8(bytes.clone().expect("rewritten")).unwrap()
    }

    const MASTER: (&str, &str, &str, bool) = (
        "rId1",
        "slideMaster",
        "../slideMasters/slideMaster1.xml",
        false,
    );

    #[test]
    fn a_part_with_nothing_to_remove_stays_as_it_is() {
        let xml = layout(
            r#"<p:sp><p:nvSpPr><p:cNvPr id="2" name="a"><a:hlinkClick r:id="rId2"/></p:cNvPr><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr/></p:sp>"#,
            "",
        );
        let template = template(
            xml.as_bytes(),
            &[MASTER, ("rId2", "hyperlink", "https://example.com/", true)],
        );
        let part = part(&template, LAYOUT).unwrap();
        assert!(part.xml.is_none() && part.rels.is_none() && part.removed.is_empty());
        assert_eq!(ids(&part), ["rId1", "rId2"]);
    }

    #[test]
    fn an_ole_object_goes_with_its_alternates_preview_and_animations() {
        let ole = r#"<mc:AlternateContent><mc:Choice Requires="v"><p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id="4" name="Object 3"/><p:cNvGraphicFramePr/><p:nvPr/></p:nvGraphicFramePr><p:xfrm/><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/presentationml/2006/ole"><p:oleObj r:id="rId2" progId="Word.Document.12"><p:embed/></p:oleObj></a:graphicData></a:graphic></p:graphicFrame></mc:Choice><mc:Fallback><p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id="4" name="Object 3"/><p:cNvGraphicFramePr/><p:nvPr/></p:nvGraphicFramePr><p:xfrm/><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/presentationml/2006/ole"><p:oleObj r:id="rId2"><p:embed/><p:pic><p:blipFill><a:blip r:embed="rId3"/></p:blipFill></p:pic></p:oleObj></a:graphicData></a:graphic></p:graphicFrame></mc:Fallback></mc:AlternateContent>"#;
        let kept = r#"<p:sp><p:nvSpPr><p:cNvPr id="5" name="b"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr/></p:sp>"#;
        let timing = r#"<p:timing><p:tnLst><p:par><p:cTn id="1"><p:childTnLst><p:set><p:cBhvr><p:tgtEl><p:spTgt spid="4"/></p:tgtEl></p:cBhvr></p:set></p:childTnLst></p:cTn></p:par></p:tnLst></p:timing>"#;
        let xml = format!(
            r#"<p:sldLayout {NS}><p:cSld><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>{ole}{kept}</p:spTree></p:cSld>{timing}</p:sldLayout>"#
        );
        let template = template(
            xml.as_bytes(),
            &[
                MASTER,
                ("rId2", "package", "../embeddings/doc.docx", false),
                ("rId3", "image", "../media/preview.png", false),
            ],
        );
        let part = part(&template, LAYOUT).unwrap();
        assert_eq!(part.removed, [Removed::OleObject]);
        assert_eq!(ids(&part), ["rId1"]);
        let out = text(&part.xml);
        assert!(
            !out.contains("AlternateContent") && !out.contains("oleObj") && !out.contains("timing")
        );
        assert!(out.contains(kept));
        let rels = text(&part.rels);
        assert!(
            rels.contains("slideMaster1.xml") && !rels.contains("rId2") && !rels.contains("rId3")
        );
    }

    #[test]
    fn a_linked_ole_object_is_one_removal_not_two() {
        let frame = r#"<p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id="4" name="Linked"/><p:cNvGraphicFramePr/><p:nvPr/></p:nvGraphicFramePr><p:xfrm/><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/presentationml/2006/ole"><p:oleObj r:id="rId2"><p:link/></p:oleObj></a:graphicData></a:graphic></p:graphicFrame>"#;
        let template = template(
            layout(frame, "").as_bytes(),
            &[
                MASTER,
                ("rId2", "oleObject", "file:///C:/budget.xlsx", true),
            ],
        );
        let part = part(&template, LAYOUT).unwrap();
        assert_eq!(part.removed, [Removed::OleObject]);
        assert!(!text(&part.xml).contains("graphicFrame"));
    }

    #[test]
    fn an_activex_control_takes_an_emptied_control_list() {
        let controls = r#"<p:controls><mc:AlternateContent><mc:Choice Requires="v"><p:control spid="1025" name="Button" r:id="rId2"/></mc:Choice><mc:Fallback><p:control name="Button" r:id="rId2"><p:pic><p:blipFill><a:blip r:embed="rId3"/></p:blipFill></p:pic></p:control></mc:Fallback></mc:AlternateContent></p:controls>"#;
        let template = template(
            layout("", controls).as_bytes(),
            &[
                MASTER,
                ("rId2", "control", "../activeX/activeX1.xml", false),
                ("rId3", "image", "../media/button.emf", false),
            ],
        );
        let part = part(&template, LAYOUT).unwrap();
        assert_eq!(part.removed, [Removed::ActiveX]);
        assert_eq!(ids(&part), ["rId1"]);
        assert_eq!(text(&part.xml), layout("", ""));
    }

    #[test]
    fn a_linked_picture_keeps_its_embedded_copy() {
        let pic = |blip: &str| {
            format!(
                r#"<p:pic><p:nvPicPr><p:cNvPr id="2" name="Logo"/><p:cNvPicPr/><p:nvPr/></p:nvPicPr><p:blipFill>{blip}</p:blipFill><p:spPr/></p:pic>"#
            )
        };
        let xml = layout(
            &pic(r#"<a:blip r:embed="rId3" r:link='rId2' cstate="print"/>"#),
            "",
        );
        let template = template(
            xml.as_bytes(),
            &[
                MASTER,
                ("rId2", "image", "https://tracker.example/logo.png", true),
                ("rId3", "image", "../media/logo.png", false),
            ],
        );
        let part = part(&template, LAYOUT).unwrap();
        assert_eq!(
            part.removed,
            [Removed::External {
                kind: "image".into(),
                target: "https://tracker.example/logo.png".into()
            }]
        );
        assert_eq!(ids(&part), ["rId1", "rId3"]);
        assert_eq!(
            text(&part.xml),
            layout(&pic(r#"<a:blip r:embed="rId3" cstate="print"/>"#), "")
        );
    }

    #[test]
    fn an_external_reference_takes_its_extension_and_alternates() {
        // Linked media inside an extension, and a transition's linked sound inside a branch.
        let pic = r#"<p:pic><p:nvPicPr><p:cNvPr id="2" name="Video"/><p:cNvPicPr/><p:nvPr><a:videoFile r:link="rId2"/><p:extLst><p:ext uri="{DAA4B4D4-6D71-4841-9C94-3DE7FCFB9230}"><p14:media xmlns:p14="http://schemas.microsoft.com/office/powerpoint/2010/main" r:link="rId2"/></p:ext></p:extLst></p:nvPr></p:nvPicPr><p:blipFill><a:blip r:embed="rId3"/></p:blipFill><p:spPr/></p:pic>"#;
        let transition = r#"<mc:AlternateContent><mc:Choice Requires="p14"><p:transition><p:sndAc><p:stSnd><p:snd r:embed="rId4" name="chime"/></p:stSnd></p:sndAc></p:transition></mc:Choice><mc:Fallback><p:transition/></mc:Fallback></mc:AlternateContent>"#;
        let xml = format!(
            r#"<p:sldLayout {NS}><p:cSld><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>{pic}</p:spTree></p:cSld>{transition}</p:sldLayout>"#
        );
        let template = template(
            xml.as_bytes(),
            &[
                MASTER,
                ("rId2", "video", "https://example.com/v.mp4", true),
                ("rId3", "image", "../media/poster.png", false),
                ("rId4", "audio", "\\\\server\\chime.wav", true),
            ],
        );
        let part = part(&template, LAYOUT).unwrap();
        assert_eq!(part.removed.len(), 2);
        let out = text(&part.xml);
        assert!(
            !out.contains("videoFile") && !out.contains("<p:ext ") && !out.contains("transition")
        );
        assert!(out.contains(r#"<p:extLst></p:extLst>"#) && out.contains(r#"r:embed="rId3""#));
        assert_eq!(ids(&part), ["rId1", "rId3"]);
    }

    #[test]
    fn unreferenced_external_and_activex_relationships_go_too() {
        let template = template(
            layout("", "").as_bytes(),
            &[
                MASTER,
                (
                    "rId2",
                    "attachedTemplate",
                    "https://example.com/t.dotx",
                    true,
                ),
                ("rId3", "control", "../activeX/activeX1.xml", false),
                ("rId4", "hyperlink", "https://example.com/", true),
            ],
        );
        let part = part(&template, LAYOUT).unwrap();
        assert_eq!(
            part.removed,
            [
                Removed::External {
                    kind: "attachedTemplate".into(),
                    target: "https://example.com/t.dotx".into()
                },
                Removed::ActiveX,
            ]
        );
        assert_eq!(ids(&part), ["rId1", "rId4"]);
        assert!(part.xml.is_none());
    }

    #[test]
    fn prefixes_are_read_by_namespace() {
        let xml = r#"<q:sldLayout xmlns:q="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:z="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:d="http://schemas.openxmlformats.org/drawingml/2006/main"><q:cSld><q:spTree><q:pic><d:blip z:link="rId2"/></q:pic></q:spTree></q:cSld></q:sldLayout>"#;
        let template = template(
            xml.as_bytes(),
            &[MASTER, ("rId2", "image", "https://x.example/a.png", true)],
        );
        let out = text(&part(&template, LAYOUT).unwrap().xml);
        assert!(out.contains("<d:blip/>"), "{out}");
    }

    #[test]
    fn utf16_parts_come_back_as_utf8() {
        let xml = format!(
            r#"<?xml version="1.0" encoding="UTF-16"?>{}"#,
            layout("", r#"<p:controls><p:control r:id="rId2"/></p:controls>"#)
        );
        let mut utf16 = vec![0xFF, 0xFE];
        utf16.extend(xml.encode_utf16().flat_map(u16::to_le_bytes));
        let template = template(
            &utf16,
            &[
                MASTER,
                ("rId2", "control", "../activeX/activeX1.xml", false),
            ],
        );
        let out = text(&part(&template, LAYOUT).unwrap().xml);
        assert!(
            out.starts_with(r#"<?xml version="1.0" encoding="UTF-8"?>"#)
                && !out.contains("control")
        );
    }

    #[test]
    fn a_part_that_is_only_an_object_or_malformed_is_invalid() {
        let lone = format!(r#"<p:oleObj {NS} r:id="rId2"/>"#);
        let whole = template(
            lone.as_bytes(),
            &[MASTER, ("rId2", "oleObject", "../embeddings/o.bin", false)],
        );
        assert!(part(&whole, LAYOUT).is_err());
        let broken = template(
            b"<p:sldLayout><p:oleObj>",
            &[MASTER, ("rId2", "oleObject", "../embeddings/o.bin", false)],
        );
        assert!(
            part(&broken, LAYOUT)
                .unwrap_err()
                .0
                .contains("malformed XML")
        );
    }
}
