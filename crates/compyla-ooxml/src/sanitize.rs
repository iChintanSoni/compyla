//! Active and external content removed from the template parts an output keeps (markdown-spec.md
//! §10.4, C58): relationships with an external target other than hyperlinks, hyperlinks §4.5
//! doesn't allow and actions that run something, OLE objects and ActiveX controls, together with
//! what refers to them. A part with nothing to remove stays byte for byte; in one with something,
//! everything else stays as it was. Relationships the output drops for its own reasons (a deck's,
//! to the template's slides, C60) go the same way, unreported.

use std::collections::{HashMap, HashSet};
use std::ops::Range;

use compyla_core::diagnostic::CAP;
use compyla_core::parse::destination_allowed;
use quick_xml::NsReader;
use quick_xml::events::Event;
use quick_xml::name::ResolveResult;

use crate::dom::{self, Error, MAIN_A, MAIN_P, PACKAGE_RELS, REL_NS};
use crate::template::{self, Invalid, Relationship, Template};

const MC: &str = "http://schemas.openxmlformats.org/markup-compatibility/2006";
/// VML names relationships with `o:relid` as well as `r:id`.
const VML_OFFICE: &str = "urn:schemas-microsoft-com:office:office";
/// Strict OOXML's namespaces for PresentationML, DrawingML and relationship attributes: read like
/// the Transitional ones, so nothing in a Strict part goes unseen.
const STRICT_P: &str = "http://purl.oclc.org/ooxml/presentationml/main";
const STRICT_A: &str = "http://purl.oclc.org/ooxml/drawingml/main";
const STRICT_REL_NS: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships";
/// The hyperlink relationship type, Transitional and Strict, compared exactly: a type that only
/// ends like one isn't one.
const HYPERLINK: [&str; 2] = [
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink",
    "http://purl.oclc.org/ooxml/officeDocument/relationships/hyperlink",
];
/// The hyperlink actions that only navigate; any other runs something (a program, a macro, an
/// object's verb).
const NAVIGATION: [&str; 7] = [
    "noaction",
    "hlinkshowjump",
    "hlinksldjump",
    "hlinkpres",
    "hlinkfile",
    "customshow",
    "media",
];
/// The most removals one part reports: the diagnostic cap's worth and one more, so the cap still
/// applies (§11.2) without holding what it would drop (§12.1).
pub const REPORTED: usize = CAP + 1;

/// Something removed from a part: each is one `template_content_removed`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Removed {
    /// An OLE object, embedded or linked.
    OleObject,
    /// An ActiveX control.
    ActiveX,
    /// An Office add-in (web extension), which runs code from wherever its manifest names.
    AddIn,
    /// A relationship with an external target, other than a hyperlink: its type (its last
    /// segment, or the whole type when that only ends like the hyperlink type) and target.
    External { kind: String, target: String },
    /// A hyperlink relationship that something other than a hyperlink refers to: its target.
    Misused { target: String },
    /// A hyperlink whose destination §4.5 doesn't allow.
    Hyperlink { target: String },
    /// A hyperlink action that runs something rather than navigating.
    Action { action: String },
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
    /// What was removed: objects and actions in document order, then relationships in their
    /// order; at most `REPORTED`.
    pub removed: Vec<Removed>,
}

/// `name` from `template`, without active or external content (§10.4), and without the
/// relationships `dropped` names, which the output removes for its own reasons, with what refers to
/// them as with external content. Those aren't reported unless they are external content too.
pub fn part(
    template: &Template,
    name: &str,
    dropped: &dyn Fn(&Relationship) -> bool,
) -> Result<Part, Invalid> {
    let relationships = template.relationships(name)?;
    let unchanged = |relationships| {
        Ok(Part {
            relationships,
            xml: None,
            rels: None,
            removed: Vec::new(),
        })
    };
    // Relationships that go whatever refers to them: by type, or because the output drops them,
    // which goes the same way but unreported. Then hyperlinks, which go if anything but a hyperlink
    // refers to them; and embedded data, which goes unless something kept refers to it.
    let by_type: Vec<bool> = relationships.iter().map(removable).collect();
    let by_output: Vec<bool> = relationships
        .iter()
        .zip(&by_type)
        .map(|(r, &by_type)| !by_type && dropped(r))
        .collect();
    let mut removable = ids(&relationships, &by_type);
    let silent = ids(&relationships, &by_output);
    let hyperlinks: HashSet<&str> = relationships
        .iter()
        .filter(|r| {
            let id = r.id.trim();
            is_hyperlink(r) && !removable.contains(id) && !silent.contains(id)
        })
        .map(|r| r.id.trim())
        .collect();
    let text = if is_xml(template, name) {
        let data = template.part(name).unwrap_or_default();
        Some(dom::utf8_document(data).map_err(|error| invalid(name, error))?)
    } else {
        None
    };
    // Every kept XML part is read: an object, a control or an action is found by what it is,
    // however it is written (character references and all), whatever it refers to.
    if text.is_none()
        && !by_type.contains(&true)
        && !by_output.contains(&true)
        && !relationships.iter().any(is_embedding)
    {
        return unchanged(relationships);
    }
    let nodes = match &text {
        Some(text) => nodes(text).map_err(|error| invalid(name, error))?,
        None => Vec::new(),
    };

    // Every pass below is linear: a hostile part may have as many elements as §12.1 allows.
    let extents = Extents::new(&nodes);

    // A relationship of the hyperlink type is a hyperlink only where hyperlinks refer to it.
    for node in nodes.iter().filter(|node| !node.is_hyperlink()) {
        for (_, id) in &node.refs {
            if let Some(&id) = hyperlinks.get(id.as_str()) {
                removable.insert(id);
            }
        }
    }

    // OLE objects (the graphic frame holding one), ActiveX controls and actions that run
    // something, each once: these are reported, and what they refer to goes with them.
    let mut reported: Vec<(usize, Removed)> = Vec::new();
    let mut roots: HashSet<usize> = HashSet::new();
    for (index, node) in nodes.iter().enumerate() {
        let found = if node.is_p("oleObj") {
            let frame = extents.frame[index].unwrap_or(index);
            Some((extents.of(frame), Removed::OleObject))
        } else if node.is_p("control") {
            Some((extents.of(index), Removed::ActiveX))
        } else if let Some(action) = node.action.as_deref().filter(|a| !navigates(a)) {
            let action = action.to_string();
            Some((extents.of(index), Removed::Action { action }))
        } else {
            None
        };
        if let Some((root, what)) = found
            && roots.insert(root)
        {
            reported.push((root, what));
        }
    }
    let in_reported = mark(&nodes, &roots);

    // What refers to a removed relationship: a picture's blip loses the attribute naming it (so
    // does the root, which can't go); anything else is removed.
    let mut strip: HashMap<usize, HashSet<String>> = HashMap::new();
    for (index, node) in nodes.iter().enumerate() {
        if in_reported[index] {
            continue;
        }
        let keys: HashSet<String> = node
            .refs
            .iter()
            .filter(|(_, id)| removable.contains(id.as_str()) || silent.contains(id.as_str()))
            .map(|(key, _)| key.clone())
            .collect();
        if keys.is_empty() {
            continue;
        }
        let root = extents.of(index);
        if node.is_a("blip") || nodes[root].parent.is_none() {
            strip.insert(index, keys);
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
            && node.is_p("spTgt")
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
        if node.is_p("controls")
            && !inside[index]
            && children[index] > 0
            && gone[index] == children[index]
        {
            roots.insert(index);
        }
    }
    // The root element can't go: a part that is only active content isn't a usable template part.
    if roots.iter().any(|&root| nodes[root].parent.is_none()) {
        return Err(Invalid(format!(
            "has an OLE object, ActiveX control or action as the whole of `{name}`"
        )));
    }
    let inside = mark(&nodes, &roots);
    strip.retain(|index, _| !inside[*index]);

    // Relationships only removed content used go with it, and so does embedded data nothing kept
    // refers to. A relationship something reported refers to is that report's.
    let (mut within, mut outside, mut owned) = (HashSet::new(), HashSet::new(), HashSet::new());
    for (index, node) in nodes.iter().enumerate() {
        let stripped = strip.get(&index);
        for (key, id) in &node.refs {
            if inside[index] {
                within.insert(id.as_str());
            } else if !stripped.is_some_and(|keys| keys.contains(key)) {
                outside.insert(id.as_str());
            }
            if in_reported[index] {
                owned.insert(id.as_str());
            }
        }
    }
    // Removed as active or external content, which is reported, or dropped by the output.
    let removed_here = |index: usize, r: &Relationship| {
        by_type[index] || (!r.id.trim().is_empty() && removable.contains(r.id.trim()))
    };
    let dropped_here = |index: usize, r: &Relationship| {
        by_output[index] || (!r.id.trim().is_empty() && silent.contains(r.id.trim()))
    };
    // Which relationships go, by position: ids needn't be there, or unique.
    let gone: Vec<bool> = relationships
        .iter()
        .enumerate()
        .map(|(index, r)| {
            let id = r.id.trim();
            removed_here(index, r)
                || dropped_here(index, r)
                || (within.contains(id) && !outside.contains(id))
                || (is_embedding(r) && !outside.contains(id))
        })
        .collect();

    // One report per object or action, and one per removed relationship that isn't one's.
    let mut removed: Vec<Removed> = reported
        .into_iter()
        .filter(|(root, _)| !nodes[*root].parent.is_some_and(|p| in_reported[p]))
        .map(|(_, what)| what)
        .take(REPORTED)
        .collect();
    for (index, relationship) in relationships.iter().enumerate() {
        if removed.len() >= REPORTED {
            break;
        }
        let id = relationship.id.trim();
        if !removed_here(index, relationship) || by_output[index] || owned.contains(id) {
            continue;
        }
        let kind = suffix(&relationship.kind);
        let target = || relationship.target.clone();
        removed.push(if kind.eq_ignore_ascii_case("oleObject") {
            Removed::OleObject
        } else if kind.eq_ignore_ascii_case("control") {
            Removed::ActiveX
        } else if kind.eq_ignore_ascii_case("webextension") {
            Removed::AddIn
        } else if kind.eq_ignore_ascii_case("vmlDrawing") && !relationship.external {
            // A legacy drawing of the objects and controls, which are reported themselves.
            continue;
        } else if is_hyperlink(relationship) && destination_allowed(&relationship.target) {
            Removed::Misused { target: target() }
        } else if is_hyperlink(relationship) {
            Removed::Hyperlink { target: target() }
        } else {
            // A type that only ends like the hyperlink type is shown whole.
            let kind = if kind.eq_ignore_ascii_case("hyperlink") {
                relationship.kind.as_str()
            } else {
                kind
            };
            Removed::External {
                kind: kind.to_string(),
                target: target(),
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
    let rels = if !gone.contains(&true) {
        None
    } else {
        let part = template::relationships_part(name);
        let data = dom::utf8_document(template.part(&part).unwrap_or_default())
            .map_err(|error| invalid(&part, error))?;
        // The relationships are the root's `Relationship` children, in order, as
        // template::Template::relationships reads them.
        let edits = nodes_of(&data, &part)?
            .iter()
            .filter(|node| node.parent == Some(0) && node.is(PACKAGE_RELS, "Relationship"))
            .zip(&gone)
            .filter(|(_, gone)| **gone)
            .map(|(node, _)| (node.span.clone(), Vec::new()))
            .collect();
        Some(splice(&data, edits))
    };
    Ok(Part {
        relationships: relationships
            .into_iter()
            .zip(gone)
            .filter_map(|(r, gone)| (!gone).then_some(r))
            .collect(),
        xml,
        rels,
        removed,
    })
}

/// A relationship §10.4 removes whatever refers to it: an external target other than a
/// hyperlink's; a hyperlink to a destination §4.5 doesn't allow; an OLE object, an ActiveX control,
/// the legacy VML drawing that only draws them, or an Office add-in, wherever it is. Types other
/// than the hyperlink compare without case, by their last segment, so the Strict namespace counts
/// too.
fn removable(relationship: &Relationship) -> bool {
    let kind = suffix(&relationship.kind);
    if is_hyperlink(relationship) {
        return !destination_allowed(&relationship.target);
    }
    relationship.external
        || ["oleObject", "control", "vmlDrawing", "webextension"]
            .iter()
            .any(|k| kind.eq_ignore_ascii_case(k))
}

/// The ids of the relationships `flags` marks, trimmed, as references compare them. An Id-less
/// relationship has the id "", which nothing can refer to, so it's left out.
fn ids<'r>(relationships: &'r [Relationship], flags: &[bool]) -> HashSet<&'r str> {
    relationships
        .iter()
        .zip(flags)
        .filter(|(r, flag)| **flag && !r.id.trim().is_empty())
        .map(|(r, _)| r.id.trim())
        .collect()
}

fn is_hyperlink(relationship: &Relationship) -> bool {
    HYPERLINK.contains(&relationship.kind.as_str())
}

/// Embedded data: an embedded package (an OLE object's or a chart's), or an ActiveX control's.
fn is_embedding(relationship: &Relationship) -> bool {
    let kind = suffix(&relationship.kind);
    kind.eq_ignore_ascii_case("package") || kind.eq_ignore_ascii_case("activeXControlBinary")
}

/// Whether a hyperlink `action` only navigates: none, or `ppaction://` and a navigation verb, up
/// to its query, compared without case.
fn navigates(action: &str) -> bool {
    let action = action.to_ascii_lowercase();
    action.is_empty()
        || action.strip_prefix("ppaction://").is_some_and(|rest| {
            let verb = rest.split('?').next().unwrap_or_default();
            NAVIGATION.contains(&verb)
        })
}

fn suffix(kind: &str) -> &str {
    kind.rsplit('/').next().unwrap_or_default()
}

/// Whether `name` is read as XML: by its name or content type, or by what it holds, since a layout
/// is parsed as one whatever it is called. A part that doesn't decode as UTF-8 or UTF-16, or
/// doesn't start with `<` past byte order marks and whitespace, is data.
fn is_xml(template: &Template, name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let declared = lower.ends_with(".xml")
        || lower.ends_with(".vml")
        || template.content_type(name).is_some_and(|t| {
            t.to_ascii_lowercase()
                .split(';')
                .next()
                .unwrap_or("")
                .trim()
                .ends_with("xml")
        });
    declared || template.part(name).is_some_and(looks_like_xml)
}

/// Whether `data` is UTF-8 or UTF-16 text, all of it, whose first character past byte order marks
/// and whitespace is `<`: what a lenient reader would read as XML, so it is read here too, and
/// refused if it isn't well-formed. Without a byte order mark, UTF-16 is recognised only as dom
/// recognises it, by `<?`.
fn looks_like_xml(data: &[u8]) -> bool {
    fn opens(mut chars: impl Iterator<Item = char>) -> bool {
        chars.find(|c| !(c.is_ascii_whitespace() || *c == '\u{FEFF}')) == Some('<')
    }
    let utf16 = |data: &[u8], little: bool| {
        let (pairs, odd) = data.as_chunks::<2>();
        let units = pairs.iter().map(|&pair| {
            if little {
                u16::from_le_bytes(pair)
            } else {
                u16::from_be_bytes(pair)
            }
        });
        odd.is_empty()
            && char::decode_utf16(units)
                .collect::<Result<String, _>>()
                .is_ok_and(|text| opens(text.chars()))
    };
    match data {
        [0xFF, 0xFE, rest @ ..] => utf16(rest, true),
        [0xFE, 0xFF, rest @ ..] => utf16(rest, false),
        [b'<', 0x00, b'?', 0x00, ..] => utf16(data, true),
        [0x00, b'<', 0x00, b'?', ..] => utf16(data, false),
        _ => std::str::from_utf8(data).is_ok_and(|text| opens(text.chars())),
    }
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
    /// A hyperlink's action (`ppaction://…`), if it has one.
    action: Option<String>,
}

impl Node {
    fn is(&self, namespace: &str, name: &str) -> bool {
        self.namespace == namespace && self.name == name
    }

    /// A PresentationML element, Transitional or Strict.
    fn is_p(&self, name: &str) -> bool {
        self.name == name && (self.namespace == MAIN_P || self.namespace == STRICT_P)
    }

    /// A DrawingML element, Transitional or Strict.
    fn is_a(&self, name: &str) -> bool {
        self.name == name && (self.namespace == MAIN_A || self.namespace == STRICT_A)
    }

    fn is_hyperlink(&self) -> bool {
        self.is_a("hlinkClick") || self.is_a("hlinkHover") || self.is_a("hlinkMouseOver")
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
                // Exactly one root element, as dom reads parts.
                if open.is_empty() && !nodes.is_empty() {
                    return Err(Error::Malformed("has more than one root element".into()));
                }
                if open.len() >= dom::MAX_DEPTH {
                    return Err(Error::Malformed(format!(
                        "nests elements deeper than {} levels",
                        dom::MAX_DEPTH
                    )));
                }
                let (namespace, name) = dom::names(&reader, start);
                let hyperlink = (namespace == MAIN_A || namespace == STRICT_A)
                    && matches!(
                        name.as_str(),
                        "hlinkClick" | "hlinkHover" | "hlinkMouseOver"
                    );
                let mut refs = Vec::new();
                let mut key = None;
                let mut action = None;
                for attribute in start.attributes() {
                    let attribute = attribute.map_err(|e| Error::Malformed(e.to_string()))?;
                    let qualified = attribute.key.as_ref();
                    // Namespace declarations only: `xmlnsr:link` is an attribute like any other.
                    if qualified == "xmlns" || qualified.starts_with("xmlns:") {
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
                        || attribute_namespace == STRICT_REL_NS
                        || (attribute_namespace == VML_OFFICE && local == "relid")
                    {
                        // An empty id names no relationship (`r:id=""` on an action, say).
                        let id = value()?.trim().to_string();
                        if !id.is_empty() {
                            refs.push((qualified.to_string(), id));
                        }
                    } else if attribute_namespace.is_empty() {
                        if hyperlink && local == "action" {
                            action = Some(value()?);
                        } else if matches!(
                            (namespace.as_str(), name.as_str(), local),
                            (_, "cNvPr", "id")
                                | (MAIN_P | STRICT_P, "spTgt", "spid")
                                | (PACKAGE_RELS, "Relationship", "Id")
                        ) {
                            key = Some(value()?);
                        }
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
                    action,
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
            // As dom reads parts: nothing but whitespace, comments and processing instructions
            // outside the root element.
            Event::Text(text)
                if open.is_empty()
                    && !text
                        .xml10_content()
                        .chars()
                        .all(|c| matches!(c, ' ' | '\t' | '\r' | '\n')) =>
            {
                return Err(Error::Malformed("has text outside its root element".into()));
            }
            Event::CData(_) | Event::GeneralRef(_) if open.is_empty() => {
                return Err(Error::Malformed("has text outside its root element".into()));
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
    if nodes.is_empty() {
        return Err(Error::Malformed("has no root element".into()));
    }
    Ok(nodes)
}

/// What removing each element takes with it, found in one pass (parents come before children):
/// the animations or transition it is part of; then the extension it is in, at any depth (an
/// extension is optional, its content isn't); the graphic frame whose payload it is (a graphic
/// can't be empty); and each `mc:AlternateContent` it is a branch of, whose other branches are the
/// same content.
struct Extents {
    /// The nearest `p:timing` or `p:transition`, the element included.
    animation: Vec<Option<usize>>,
    /// The nearest `p:graphicFrame`, the element included.
    frame: Vec<Option<usize>>,
    /// What an element takes with it through extensions, graphics and alternates.
    climb: Vec<usize>,
}

impl Extents {
    fn new(nodes: &[Node]) -> Extents {
        let mut extents = Extents {
            animation: vec![None; nodes.len()],
            frame: vec![None; nodes.len()],
            climb: vec![0; nodes.len()],
        };
        // The nearest extension (`p:ext` or `a:ext` in an extension list), the element included.
        let mut extension: Vec<Option<usize>> = vec![None; nodes.len()];
        for (index, node) in nodes.iter().enumerate() {
            let parent = node.parent;
            extents.animation[index] = if node.is_p("timing") || node.is_p("transition") {
                Some(index)
            } else {
                parent.and_then(|p| extents.animation[p])
            };
            extents.frame[index] = if node.is_p("graphicFrame") {
                Some(index)
            } else {
                parent.and_then(|p| extents.frame[p])
            };
            let in_list =
                parent.is_some_and(|p| nodes[p].is_p("extLst") || nodes[p].is_a("extLst"));
            extension[index] = if in_list && (node.is_p("ext") || node.is_a("ext")) {
                Some(index)
            } else {
                parent.and_then(|p| extension[p])
            };
            extents.climb[index] = match parent {
                Some(p) if extension[p].is_some() => {
                    extension[p].map_or(index, |e| extents.climb[e])
                }
                Some(p) if nodes[p].is(MC, "Choice") || nodes[p].is(MC, "Fallback") => nodes[p]
                    .parent
                    .map_or(index, |alternate| extents.climb[alternate]),
                Some(p) if nodes[p].is_a("graphicData") => {
                    extents.frame[p].map_or(index, |frame| extents.climb[frame])
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
fn without_attributes(tag: &[u8], keys: &HashSet<String>) -> Result<Vec<u8>, Error> {
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
        if keys.contains(key) {
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

    /// `name` without active or external content, the output dropping nothing of its own.
    fn part(template: &Template, name: &str) -> Result<Part, Invalid> {
        super::part(template, name, &|_| false)
    }

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

    /// A package whose layout is `xml` with `relationships` as written.
    fn template_raw(xml: &[u8], relationships: &str) -> Template {
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

    fn shape(id: u32, link: &str) -> String {
        format!(
            r#"<p:sp><p:nvSpPr><p:cNvPr id="{id}" name="s{id}">{link}</p:cNvPr><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr/></p:sp>"#
        )
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
    fn what_the_output_drops_goes_like_external_content_but_unreported() {
        // A jump to a slide with a sound, a jump to another slide nothing refers to, and an
        // external relationship the output would drop too, which is still reported.
        let jump = r#"<a:hlinkClick r:id="rId2" action="ppaction://hlinksldjump"><a:snd r:embed="rId3" name="click.wav"/></a:hlinkClick>"#;
        let next = r#"<a:hlinkClick r:id="" action="ppaction://hlinkshowjump?jump=nextslide"/>"#;
        let xml = layout(&[shape(2, jump), shape(3, next)].concat(), "");
        let template = template(
            xml.as_bytes(),
            &[
                MASTER,
                ("rId2", "slide", "../slides/slide1.xml", false),
                ("rId3", "audio", "../media/click.wav", false),
                ("rId4", "slide", "../slides/slide2.xml", false),
                ("rId5", "slide", "https://example.com/slide", true),
            ],
        );
        let slides = |r: &Relationship| r.kind.ends_with("/slide");
        let part = super::part(&template, LAYOUT, &slides).unwrap();
        assert_eq!(ids(&part), ["rId1"]);
        assert_eq!(
            part.removed,
            [Removed::External {
                kind: "slide".into(),
                target: "https://example.com/slide".into()
            }]
        );
        assert_eq!(
            text(&part.xml),
            layout(&[shape(2, ""), shape(3, next)].concat(), "")
        );
        let rels = text(&part.rels);
        assert!(
            rels.contains("slideMaster1.xml") && !rels.contains("slides/"),
            "{rels}"
        );
        assert!(!rels.contains("click.wav"), "{rels}");

        // Dropping nothing, only the external relationship goes.
        let kept = super::part(&template, LAYOUT, &|_| false).unwrap();
        assert_eq!(ids(&kept), ["rId1", "rId2", "rId3", "rId4"]);
    }

    #[test]
    fn a_data_part_loses_what_the_output_drops_too() {
        let base = template(layout("", "").as_bytes(), &[MASTER]);
        let data = "ppt/media/data.bin";
        let mut entries: Vec<(String, Vec<u8>)> = base
            .parts()
            .map(|(name, bytes)| (name.to_string(), bytes.to_vec()))
            .collect();
        entries.push((data.into(), b"\x00data".to_vec()));
        entries.push((
            "ppt/media/_rels/data.bin.rels".into(),
            format!(r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{R}/slide" Target="../slides/slide1.xml"/><Relationship Id="rId2" Type="{R}/image" Target="a.png"/></Relationships>"#).into_bytes(),
        ));
        let template = Template::read(&crate::zip::write(&entries)).unwrap();
        let part = super::part(&template, data, &|r| r.kind.ends_with("/slide")).unwrap();
        assert_eq!(ids(&part), ["rId2"]);
        assert!(part.xml.is_none() && part.removed.is_empty());
        assert!(!text(&part.rels).contains("slide1"));
    }

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

    #[test]
    fn a_hyperlink_relationship_anything_else_refers_to_is_external_content() {
        // A picture linked through a relationship of the hyperlink type, and through a type that
        // only ends like one.
        let xml = layout(
            r#"<p:pic><p:nvPicPr><p:cNvPr id="2" name="a"/><p:cNvPicPr/><p:nvPr/></p:nvPicPr><p:blipFill><a:blip r:link="rId2"/></p:blipFill><p:spPr/></p:pic><p:pic><p:nvPicPr><p:cNvPr id="3" name="b"/><p:cNvPicPr/><p:nvPr/></p:nvPicPr><p:blipFill><a:blip r:link="rId3"/></p:blipFill><p:spPr/></p:pic>"#,
            "",
        );
        let template = template_raw(
            xml.as_bytes(),
            &format!(
                r#"<Relationship Id="rId1" Type="{R}/slideMaster" Target="../slideMasters/slideMaster1.xml"/><Relationship Id="rId2" Type="{R}/hyperlink" Target="https://tracker.example/a.png" TargetMode="External"/><Relationship Id="rId3" Type="http://evil.example/x/hyperlink" Target="https://tracker.example/b.png" TargetMode="External"/>"#
            ),
        );
        let part = part(&template, LAYOUT).unwrap();
        assert_eq!(ids(&part), ["rId1"]);
        assert_eq!(part.removed.len(), 2);
        let out = text(&part.xml);
        assert!(
            !out.contains("r:link") && out.matches("<a:blip/>").count() == 2,
            "{out}"
        );
    }

    #[test]
    fn hyperlinks_go_where_section_4_5_refuses_their_destination_or_their_action_runs_something() {
        let tree = [
            shape(2, r#"<a:hlinkClick r:id="rId2"/>"#),
            shape(3, r#"<a:hlinkHover r:id="rId3"/>"#),
            shape(
                4,
                r#"<a:hlinkClick r:id="rId4" action="ppaction://program"/>"#,
            ),
            shape(
                5,
                r#"<a:hlinkClick r:id="" action="ppaction://macro?name=Run"/>"#,
            ),
            shape(6, r#"<a:hlinkClick r:id="rId5"/>"#),
            shape(
                7,
                r#"<a:hlinkClick r:id="" action="ppaction://hlinkshowjump?jump=nextslide"/>"#,
            ),
        ]
        .concat();
        let template = template(
            layout(&tree, "").as_bytes(),
            &[
                MASTER,
                ("rId2", "hyperlink", "ms-msdt:/id PCWDiagnostic", true),
                ("rId3", "hyperlink", "file://host/share", true),
                (
                    "rId4",
                    "hyperlink",
                    "file:///C:/Windows/System32/calc.exe",
                    true,
                ),
                ("rId5", "hyperlink", "https://example.com/", true),
            ],
        );
        let part = part(&template, LAYOUT).unwrap();
        // The two actions first (document order), then the refused destinations; the program's
        // relationship is its action's.
        assert_eq!(
            part.removed,
            [
                Removed::Action {
                    action: "ppaction://program".into()
                },
                Removed::Action {
                    action: "ppaction://macro?name=Run".into()
                },
                Removed::Hyperlink {
                    target: "ms-msdt:/id PCWDiagnostic".into()
                },
                Removed::Hyperlink {
                    target: "file://host/share".into()
                },
            ]
        );
        assert_eq!(ids(&part), ["rId1", "rId5"]);
        let out = text(&part.xml);
        assert_eq!(out.matches("<a:hlink").count(), 2, "{out}");
        assert!(out.contains(r#"r:id="rId5""#) && out.contains("hlinkshowjump"));
        assert!(
            out.contains(r#"name="s4""#),
            "the shape stays, only its link goes"
        );
    }

    #[test]
    fn a_target_mode_other_than_internal_is_external() {
        let xml = layout(
            r#"<p:pic><p:nvPicPr><p:cNvPr id="2" name="a"/><p:cNvPicPr/><p:nvPr/></p:nvPicPr><p:blipFill><a:blip r:link="rId2"/></p:blipFill><p:spPr/></p:pic>"#,
            "",
        );
        for mode in [
            r#" TargetMode="external""#,
            r#" TargetMode="External ""#,
            "",
        ] {
            let target = if mode.is_empty() {
                "https://tracker.example/a.png"
            } else {
                "../media/a.png"
            };
            let template = template_raw(
                xml.as_bytes(),
                &format!(
                    r#"<Relationship Id="rId1" Type="{R}/slideMaster" Target="../slideMasters/slideMaster1.xml"/><Relationship Id="rId2" Type="{R}/image" Target="{target}"{mode}/>"#
                ),
            );
            let part = part(&template, LAYOUT).unwrap();
            assert_eq!(ids(&part), ["rId1"], "{mode:?}");
            assert_eq!(part.removed.len(), 1, "{mode:?}");
        }
    }

    #[test]
    fn a_relationship_without_an_id_goes_from_the_relationships_part() {
        let template = template_raw(
            layout("", "").as_bytes(),
            &format!(
                r#"<Relationship Id="rId1" Type="{R}/slideMaster" Target="../slideMasters/slideMaster1.xml"/><Relationship Type="{R}/image" Target="https://tracker.example/a.png" TargetMode="External"/>"#
            ),
        );
        let part = part(&template, LAYOUT).unwrap();
        assert_eq!(part.removed.len(), 1);
        assert!(!text(&part.rels).contains("tracker"));
    }

    #[test]
    fn legacy_drawings_and_orphaned_embeddings_go_unreported() {
        let template = template(
            layout("", "").as_bytes(),
            &[
                MASTER,
                ("rId2", "vmlDrawing", "../drawings/vmlDrawing1.vml", false),
                ("rId3", "package", "../embeddings/orphan.docx", false),
                (
                    "rId4",
                    "activeXControlBinary",
                    "../activeX/orphan.bin",
                    false,
                ),
            ],
        );
        let part = part(&template, LAYOUT).unwrap();
        assert!(part.removed.is_empty());
        assert_eq!(ids(&part), ["rId1"]);
        assert!(part.xml.is_none());
    }

    #[test]
    fn a_prefix_that_starts_like_xmlns_is_still_read() {
        let xml = r#"<p:sldLayout xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:xmlnsr="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><p:cSld><p:spTree><p:pic><a:blip xmlnsr:link="rId2"/></p:pic></p:spTree></p:cSld></p:sldLayout>"#;
        let template = template(
            xml.as_bytes(),
            &[
                MASTER,
                ("rId2", "image", "https://tracker.example/a.png", true),
            ],
        );
        assert!(text(&part(&template, LAYOUT).unwrap().xml).contains("<a:blip/>"));
    }

    #[test]
    fn an_ole_object_something_else_also_refers_to_is_one_removal() {
        let frame = r#"<p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id="4" name="o"/><p:cNvGraphicFramePr/><p:nvPr/></p:nvGraphicFramePr><p:xfrm/><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/presentationml/2006/ole"><p:oleObj r:id="rId2"><p:embed/></p:oleObj></a:graphicData></a:graphic></p:graphicFrame>"#;
        let tree = format!(r#"{frame}{}"#, shape(5, r#"<a:hlinkClick r:id="rId2"/>"#));
        let template = template(
            layout(&tree, "").as_bytes(),
            &[MASTER, ("rId2", "oleObject", "../embeddings/o.bin", false)],
        );
        let part = part(&template, LAYOUT).unwrap();
        assert_eq!(part.removed, [Removed::OleObject]);
        assert!(!text(&part.xml).contains("hlinkClick"));
    }

    #[test]
    fn a_graphics_payload_takes_its_frame_and_an_extensions_content_takes_the_extension() {
        let diagram = r#"<p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id="4" name="d"/><p:cNvGraphicFramePr/><p:nvPr/></p:nvGraphicFramePr><p:xfrm/><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/diagram"><dgm:relIds xmlns:dgm="http://schemas.openxmlformats.org/drawingml/2006/diagram" r:dm="rId2" r:lo="rId1" r:qs="rId1" r:cs="rId1"/></a:graphicData></a:graphic></p:graphicFrame>"#;
        let deep = r#"<p:pic><p:nvPicPr><p:cNvPr id="5" name="p"/><p:cNvPicPr/><p:nvPr/></p:nvPicPr><p:blipFill><a:blip r:embed="rId4"><a:extLst><a:ext uri="{BEBA8EAE-BF5A-486C-A8C5-ECC9F3942E4B}"><a14:imgProps xmlns:a14="http://schemas.microsoft.com/office/drawing/2010/main"><a14:imgLayer r:embed="rId3"><a14:imgEffect/></a14:imgLayer></a14:imgProps></a:ext></a:extLst></a:blip></p:blipFill><p:spPr/></p:pic>"#;
        let template = template(
            layout(&format!("{diagram}{deep}"), "").as_bytes(),
            &[
                MASTER,
                (
                    "rId2",
                    "diagramData",
                    "https://tracker.example/data.xml",
                    true,
                ),
                ("rId3", "image", "https://tracker.example/layer.wdp", true),
                ("rId4", "image", "../media/p.png", false),
            ],
        );
        let out = text(&part(&template, LAYOUT).unwrap().xml);
        assert!(
            !out.contains("graphicFrame") && !out.contains("<a:ext "),
            "{out}"
        );
        assert!(
            out.contains(r#"<a:blip r:embed="rId4"><a:extLst></a:extLst></a:blip>"#),
            "{out}"
        );
    }

    #[test]
    fn strict_parts_are_read_like_transitional_ones() {
        let xml = r#"<p:sldLayout xmlns:p="http://purl.oclc.org/ooxml/presentationml/main" xmlns:a="http://purl.oclc.org/ooxml/drawingml/main" xmlns:r="http://purl.oclc.org/ooxml/officeDocument/relationships"><p:cSld><p:spTree><p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id="4" name="o"/><p:cNvGraphicFramePr/><p:nvPr/></p:nvGraphicFramePr><p:xfrm/><a:graphic><a:graphicData uri="x"><p:oleObj r:id="rId2"><p:embed/></p:oleObj></a:graphicData></a:graphic></p:graphicFrame></p:spTree></p:cSld></p:sldLayout>"#;
        let template = template(
            xml.as_bytes(),
            &[MASTER, ("rId2", "package", "../embeddings/o.docx", false)],
        );
        let part = part(&template, LAYOUT).unwrap();
        assert_eq!(part.removed, [Removed::OleObject]);
        assert_eq!(ids(&part), ["rId1"]);
    }

    #[test]
    fn a_part_reports_at_most_one_more_removal_than_the_cap() {
        let rels: Vec<(String, &str, String, bool)> = (0..REPORTED + 10)
            .map(|i| {
                (
                    format!("rIdX{i}"),
                    "image",
                    format!("https://tracker.example/{i}.png"),
                    true,
                )
            })
            .collect();
        let mut all: Vec<(&str, &str, &str, bool)> = vec![MASTER];
        all.extend(
            rels.iter().map(|(id, kind, target, external)| {
                (id.as_str(), *kind, target.as_str(), *external)
            }),
        );
        let part = part(&template(layout("", "").as_bytes(), &all), LAYOUT).unwrap();
        assert_eq!(part.removed.len(), REPORTED);
        assert_eq!(ids(&part), ["rId1"], "every one is still removed");
    }

    #[test]
    fn objects_controls_and_actions_go_whatever_they_name() {
        // A control behind a relationship of the wrong type, an OLE object naming nothing, and
        // actions in a part that has no hyperlink at all.
        let tree = [
            r#"<p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id="4" name="o"/><p:cNvGraphicFramePr/><p:nvPr/></p:nvGraphicFramePr><p:xfrm/><a:graphic><a:graphicData uri="x"><p:oleObj r:id="rId404"><p:embed/></p:oleObj></a:graphicData></a:graphic></p:graphicFrame>"#.to_string(),
            shape(5, r#"<a:hlinkClick r:id="" action="ppaction://macro?name=Run"/>"#),
            shape(6, r#"<a:hlinkClick r:id="" action="PPACTION://hlinkfile/../program"/>"#),
        ]
        .concat();
        let template = template(
            layout(
                &tree,
                r#"<p:controls><p:control r:id="rId2"/></p:controls>"#,
            )
            .as_bytes(),
            &[MASTER, ("rId2", "image", "../activeX/activeX1.xml", false)],
        );
        let part = part(&template, LAYOUT).unwrap();
        assert_eq!(
            part.removed,
            [
                Removed::OleObject,
                Removed::Action {
                    action: "ppaction://macro?name=Run".into()
                },
                Removed::Action {
                    action: "PPACTION://hlinkfile/../program".into()
                },
                Removed::ActiveX,
            ]
        );
        assert_eq!(ids(&part), ["rId1"]);
        let out = text(&part.xml);
        assert!(!out.contains("oleObj") && !out.contains("control") && !out.contains("action"));
    }

    #[test]
    fn an_empty_id_names_nothing_and_an_id_less_relationship_goes_by_position() {
        // A next-slide button (`r:id=""`) keeps its link while an Id-less external relationship
        // goes; another Id-less one, internal, stays.
        let xml = layout(
            &shape(
                2,
                r#"<a:hlinkClick r:id="" action="ppaction://hlinkshowjump?jump=nextslide"/>"#,
            ),
            "",
        );
        let template = template_raw(
            xml.as_bytes(),
            &format!(
                r#"<Relationship Id="rId1" Type="{R}/slideMaster" Target="../slideMasters/slideMaster1.xml"/><Relationship Type="{R}/image" Target="https://tracker.example/a.png" TargetMode="External"/><Relationship Type="{R}/image" Target="../media/b.png"/>"#
            ),
        );
        let part = part(&template, LAYOUT).unwrap();
        assert_eq!(part.removed.len(), 1);
        assert!(part.xml.is_none());
        let rels = text(&part.rels);
        assert!(
            !rels.contains("tracker") && rels.contains("../media/b.png"),
            "{rels}"
        );
        assert_eq!(part.relationships.len(), 2);
    }

    #[test]
    fn add_ins_and_external_drawings_are_reported_and_lookalike_types_shown_whole() {
        let addin = r#"<p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id="4" name="w"/><p:cNvGraphicFramePr/><p:nvPr/></p:nvGraphicFramePr><p:xfrm/><a:graphic><a:graphicData uri="http://schemas.microsoft.com/office/webextensions/webextension/2010/11"><we:webextensionref xmlns:we="http://schemas.microsoft.com/office/webextensions/webextension/2010/11" r:id="rId2"/></a:graphicData></a:graphic></p:graphicFrame>"#;
        let template = template_raw(
            layout(addin, "").as_bytes(),
            &format!(
                r#"<Relationship Id="rId1" Type="{R}/slideMaster" Target="../slideMasters/slideMaster1.xml"/><Relationship Id="rId2" Type="http://schemas.microsoft.com/office/2011/relationships/webextension" Target="../webextensions/webextension1.xml"/><Relationship Id="rId3" Type="{R}/vmlDrawing" Target="\\evil.example\share\d.vml" TargetMode="External"/><Relationship Id="rId4" Type="{R}/Hyperlink" Target="https://tracker.example/" TargetMode="External"/>"#
            ),
        );
        let part = part(&template, LAYOUT).unwrap();
        assert_eq!(
            part.removed,
            [
                Removed::AddIn,
                Removed::External {
                    kind: "vmlDrawing".into(),
                    target: r"\\evil.example\share\d.vml".into()
                },
                Removed::External {
                    kind: format!("{R}/Hyperlink"),
                    target: "https://tracker.example/".into()
                },
            ]
        );
        assert!(!text(&part.xml).contains("graphicFrame"));
        assert_eq!(ids(&part), ["rId1"]);
    }

    #[test]
    fn a_hyperlink_used_by_a_picture_is_reported_as_misused() {
        let xml = layout(
            r#"<p:pic><p:nvPicPr><p:cNvPr id="2" name="a"/><p:cNvPicPr/><p:nvPr/></p:nvPicPr><p:blipFill><a:blip r:link="rId2"/></p:blipFill><p:spPr/></p:pic>"#,
            "",
        );
        let template = template(
            xml.as_bytes(),
            &[
                MASTER,
                ("rId2", "hyperlink", "https://tracker.example/a.png", true),
            ],
        );
        assert_eq!(
            part(&template, LAYOUT).unwrap().removed,
            [Removed::Misused {
                target: "https://tracker.example/a.png".into()
            }]
        );
    }

    #[test]
    fn actions_are_found_however_they_are_written() {
        // Character references, an action without `ppaction`, and a part related only to its
        // master: nothing names a hyperlink, and every action still goes.
        let tree = [
            shape(
                2,
                r#"<a:hlinkClick r:id="" action="&#112;paction://macro?name=Evil"/>"#,
            ),
            shape(
                3,
                r#"<a:hlinkHover r:id="" action="pp&#x61;ction://ole?verb=0"/>"#,
            ),
            shape(4, r#"<a:hlinkClick r:id="" action="macro:Module1.Evil"/>"#),
        ]
        .concat();
        let template = template(layout(&tree, "").as_bytes(), &[MASTER]);
        let part = part(&template, LAYOUT).unwrap();
        assert_eq!(part.removed.len(), 3);
        assert!(!text(&part.xml).contains("action"));
    }

    #[test]
    fn ids_compare_trimmed() {
        // A picture names `rId8` through a relationship whose Id is written ` rId8 `.
        let xml = layout(
            r#"<p:pic><p:nvPicPr><p:cNvPr id="2" name="a"/><p:cNvPicPr/><p:nvPr/></p:nvPicPr><p:blipFill><a:blip r:link="rId8"/></p:blipFill><p:spPr/></p:pic>"#,
            "",
        );
        let template = template(
            xml.as_bytes(),
            &[
                MASTER,
                (" rId8 ", "hyperlink", "https://tracker.example/a.png", true),
            ],
        );
        let part = part(&template, LAYOUT).unwrap();
        assert_eq!(part.removed.len(), 1);
        assert!(text(&part.xml).contains("<a:blip/>"));
    }

    #[test]
    fn a_kept_part_that_isnt_well_formed_is_invalid() {
        let template = template(b"<p:sldLayout><a>quality control</b>", &[MASTER]);
        assert!(part(&template, LAYOUT).is_err());
    }

    #[test]
    fn a_part_is_read_as_xml_by_what_it_holds() {
        // A layout named and typed as binary data still holds an action.
        let xml = layout(
            &shape(
                2,
                r#"<a:hlinkClick r:id="" action="ppaction://macro?name=Evil"/>"#,
            ),
            "",
        );
        let mut template = template(xml.as_bytes(), &[MASTER]);
        let bin = "ppt/slideLayouts/slideLayout1.bin";
        template = {
            let entries: Vec<(String, Vec<u8>)> = crate::read::entries(&crate::zip::write(
                &[
                    ("[Content_Types].xml".to_string(), br#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="bin" ContentType="application/vnd.openxmlformats-officedocument.presentationml.printerSettings"/><Override PartName="/ppt/presentation.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"/></Types>"#.to_vec()),
                    ("_rels/.rels".to_string(), template.part("_rels/.rels").unwrap().to_vec()),
                    ("ppt/presentation.xml".to_string(), template.part("ppt/presentation.xml").unwrap().to_vec()),
                    (bin.to_string(), xml.clone().into_bytes()),
                    ("ppt/slideLayouts/_rels/slideLayout1.bin.rels".to_string(), template.part("ppt/slideLayouts/_rels/slideLayout1.xml.rels").unwrap().to_vec()),
                ],
            ))
            .unwrap();
            Template::read(&crate::zip::write(&entries)).unwrap()
        };
        let part = part(&template, bin).unwrap();
        assert_eq!(part.removed.len(), 1);
        assert!(!text(&part.xml).contains("macro"));
    }

    #[test]
    fn a_kept_part_has_exactly_one_root_and_one_byte_order_mark() {
        let two = template(b"<a/><b/>", &[MASTER]);
        assert!(part(&two, LAYOUT).is_err());
        let none = template(b"<!-- nothing -->", &[MASTER]);
        assert!(part(&none, LAYOUT).is_err());
        assert!(looks_like_xml(b"\xEF\xBB\xBF\xEF\xBB\xBF<a/>"));
        assert!(looks_like_xml(b"\xFF\xFE\xFF\xFE<\x00a\x00/\x00>\x00"));
        assert!(!looks_like_xml(b"\x89PNG\r\n\x1a\n<"));
        // An embedded font whose size happens to start like UTF-16 `<` is still data.
        assert!(!looks_like_xml(b"\x3C\x00\x01\x00\x00\xD8\x41\x00"));
        assert!(!looks_like_xml(b"\x00\x3C\x00\x02LP"));
        assert!(!looks_like_xml(b"\xFF\xFE\x20\x00<\x00\x00\xDC"));
    }
}
