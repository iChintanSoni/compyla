//! A small XML tree for reading template parts, with namespaces resolved: OOXML producers may bind
//! any prefix, so elements and attributes are matched by namespace and local name. Building is
//! iterative, so nesting depth can't overflow the stack, and a DTD refuses the part (§10.4).

use quick_xml::NsReader;
use quick_xml::events::Event;
use quick_xml::name::ResolveResult;

pub const MAIN_W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
pub const MAIN_P: &str = "http://schemas.openxmlformats.org/presentationml/2006/main";
pub const MAIN_A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
pub const MAIN_X: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
pub const REL_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
pub const PACKAGE_RELS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
pub const CONTENT_TYPES: &str = "http://schemas.openxmlformats.org/package/2006/content-types";

#[derive(Debug, Clone, Default)]
pub struct Element {
    pub namespace: String,
    pub name: String,
    pub attributes: Vec<(String, String, String)>,
    pub children: Vec<Element>,
    pub text: String,
}

impl Element {
    /// The attribute `name` in no namespace.
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attr_ns("", name)
    }

    pub fn attr_ns(&self, namespace: &str, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|(ns, local, _)| ns == namespace && local == name)
            .map(|(_, _, value)| value.as_str())
    }

    pub fn is(&self, namespace: &str, name: &str) -> bool {
        self.namespace == namespace && self.name == name
    }

    pub fn child(&self, namespace: &str, name: &str) -> Option<&Element> {
        self.children.iter().find(|c| c.is(namespace, name))
    }

    pub fn children_named<'a>(
        &'a self,
        namespace: &'a str,
        name: &'a str,
    ) -> impl Iterator<Item = &'a Element> + 'a {
        self.children.iter().filter(move |c| c.is(namespace, name))
    }

    /// Every element below this one, depth first, in document order.
    pub fn descendants(&self) -> Vec<&Element> {
        let mut out = Vec::new();
        let mut stack: Vec<&Element> = self.children.iter().rev().collect();
        while let Some(element) = stack.pop() {
            out.push(element);
            stack.extend(element.children.iter().rev());
        }
        out
    }

    /// The path `steps` of children, each by namespace and name.
    pub fn path(&self, steps: &[(&str, &str)]) -> Option<&Element> {
        steps.iter().try_fold(self, |element, (namespace, name)| {
            element.child(namespace, name)
        })
    }
}

/// The deepest nesting a part may have. Real OOXML nests a few dozen levels; the limit keeps
/// building and dropping the tree off the call stack's limits.
const MAX_DEPTH: usize = 1_000;

/// Why a part didn't parse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The part declares a DTD, which templates may not (§10.4).
    Dtd,
    Malformed(String),
}

/// Parses an XML part into its root element.
pub fn parse(bytes: &[u8]) -> Result<Element, Error> {
    let mut reader = NsReader::from_reader(bytes);
    reader.config_mut().trim_text(false);
    let mut stack: Vec<Element> = vec![Element::default()];
    let mut buffer = Vec::new();
    loop {
        let event = reader
            .read_event_into(&mut buffer)
            .map_err(|e| Error::Malformed(e.to_string()))?;
        match event {
            Event::Start(start) => {
                if stack.len() > MAX_DEPTH {
                    return Err(Error::Malformed(format!(
                        "nests elements deeper than {MAX_DEPTH} levels"
                    )));
                }
                let element = element(&reader, &start)?;
                stack.push(element);
            }
            Event::Empty(start) => {
                let element = element(&reader, &start)?;
                stack
                    .last_mut()
                    .expect("the document root")
                    .children
                    .push(element);
            }
            Event::End(_) => {
                let done = stack.pop().expect("balanced by the reader");
                stack
                    .last_mut()
                    .ok_or_else(|| Error::Malformed("unbalanced end tag".into()))?
                    .children
                    .push(done);
            }
            Event::Text(text) => {
                // Entity references arrive as their own events, so the text has none.
                if let Some(top) = stack.last_mut() {
                    top.text.push_str(&text.xml10_content());
                }
            }
            Event::CData(data) => {
                if let Some(top) = stack.last_mut() {
                    top.text.push_str(&data);
                }
            }
            Event::GeneralRef(reference) => {
                // Only the predefined entities and character references: there is no DTD.
                let name = reference.to_string();
                let character = match name.as_str() {
                    "amp" => Some('&'),
                    "lt" => Some('<'),
                    "gt" => Some('>'),
                    "quot" => Some('"'),
                    "apos" => Some('\''),
                    _ => reference.resolve_char_ref().ok().flatten(),
                };
                let character = character
                    .ok_or_else(|| Error::Malformed(format!("unknown entity `&{name};`")))?;
                if let Some(top) = stack.last_mut() {
                    top.text.push(character);
                }
            }
            Event::DocType(_) => return Err(Error::Dtd),
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }
    if stack.len() != 1 {
        return Err(Error::Malformed("unclosed element".into()));
    }
    let mut document = stack.pop().unwrap_or_default();
    document
        .children
        .pop()
        .filter(|_| document.children.is_empty())
        .ok_or_else(|| Error::Malformed("not exactly one root element".into()))
}

fn element(
    reader: &NsReader<&[u8]>,
    start: &quick_xml::events::BytesStart<'_>,
) -> Result<Element, Error> {
    let (resolved, local) = reader.resolver().resolve_element(start.name());
    let namespace = match resolved {
        ResolveResult::Bound(ns) => ns.as_ref().to_string(),
        _ => String::new(),
    };
    let name = local.as_ref().to_string();
    let mut attributes = Vec::new();
    for attribute in start.attributes() {
        let attribute = attribute.map_err(|e| Error::Malformed(e.to_string()))?;
        let key = attribute.key;
        if key.as_ref().starts_with("xmlns") {
            continue;
        }
        let (resolved, local) = reader.resolver().resolve_attribute(key);
        let namespace = match resolved {
            ResolveResult::Bound(ns) => ns.as_ref().to_string(),
            _ => String::new(),
        };
        let value = attribute
            .normalized_value(quick_xml::XmlVersion::Implicit1_0)
            .map_err(|e| Error::Malformed(e.to_string()))?
            .into_owned();
        attributes.push((namespace, local.as_ref().to_string(), value));
    }
    Ok(Element {
        namespace,
        name,
        attributes,
        children: Vec::new(),
        text: String::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn namespaces_resolve_whatever_the_prefix() {
        let xml = br#"<?xml version="1.0"?>
<q:presentation xmlns:q="http://schemas.openxmlformats.org/presentationml/2006/main"
    xmlns:z="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
  <q:sldMasterIdLst><q:sldMasterId id="1" z:id="rId1"/></q:sldMasterIdLst>
  <q:note>a &amp; b &#65;</q:note>
</q:presentation>"#;
        let root = parse(xml).unwrap();
        assert!(root.is(MAIN_P, "presentation"));
        let id = root
            .path(&[(MAIN_P, "sldMasterIdLst"), (MAIN_P, "sldMasterId")])
            .unwrap();
        assert_eq!(id.attr("id"), Some("1"));
        assert_eq!(id.attr_ns(REL_NS, "id"), Some("rId1"));
        assert_eq!(root.child(MAIN_P, "note").unwrap().text, "a & b A");
    }

    #[test]
    fn dtds_and_malformed_parts_are_refused() {
        let dtd = br#"<?xml version="1.0"?><!DOCTYPE a [<!ENTITY x "y">]><a>&x;</a>"#;
        assert_eq!(parse(dtd).unwrap_err(), Error::Dtd);
        assert!(matches!(parse(b"<a><b></a>"), Err(Error::Malformed(_))));
        assert!(matches!(parse(b"<a/><b/>"), Err(Error::Malformed(_))));
    }

    #[test]
    fn deep_nesting_is_refused_without_overflow() {
        let deep = |n: usize| format!("{}{}", "<a>".repeat(n), "</a>".repeat(n));
        assert!(parse(deep(500).as_bytes()).is_ok());
        assert!(matches!(
            parse(deep(200_000).as_bytes()),
            Err(Error::Malformed(_))
        ));
    }
}
