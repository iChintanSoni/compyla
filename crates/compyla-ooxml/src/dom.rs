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
/// The most elements a template part may have (markdown-spec.md §12.1): a tree costs far more
/// memory than its bytes, so the limit bounds what one part can cost. A real styles part, master
/// or layout has a few thousand.
pub const MAX_ELEMENTS: usize = 500_000;
/// The most namespace bindings in scope at once; quick-xml resolves prefixes by scanning them.
const MAX_BINDINGS: usize = 1_000;

/// Why a part didn't parse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The part declares a DTD, which templates may not (§10.4).
    Dtd,
    /// The part has more than [`MAX_ELEMENTS`] elements (§12.1).
    TooLarge,
    Malformed(String),
}

/// A part's bytes as UTF-8. XML parts may be UTF-8 or UTF-16 (with a byte order mark, or starting
/// `<?` in either byte order); UTF-16 is transcoded, so every check sees characters, not bytes.
pub fn utf8(bytes: &[u8]) -> Result<std::borrow::Cow<'_, [u8]>, Error> {
    let utf16 = |data: &[u8], little: bool| -> Result<std::borrow::Cow<'_, [u8]>, Error> {
        let units = data.chunks_exact(2).map(|pair| {
            let pair = [pair[0], pair[1]];
            if little {
                u16::from_le_bytes(pair)
            } else {
                u16::from_be_bytes(pair)
            }
        });
        let text: String = char::decode_utf16(units)
            .collect::<Result<String, _>>()
            .map_err(|_| Error::Malformed("isn't valid UTF-16".into()))?;
        Ok(std::borrow::Cow::Owned(text.into_bytes()))
    };
    match bytes {
        [0x00, 0x00, 0xFE, 0xFF, ..] | [0xFF, 0xFE, 0x00, 0x00, ..] => Err(Error::Malformed(
            "is UTF-32, which OOXML parts never are".into(),
        )),
        [0xEF, 0xBB, 0xBF, rest @ ..] => Ok(std::borrow::Cow::Borrowed(rest)),
        [0xFF, 0xFE, rest @ ..] => utf16(rest, true),
        [0xFE, 0xFF, rest @ ..] => utf16(rest, false),
        [b'<', 0x00, b'?', 0x00, ..] => utf16(bytes, true),
        [0x00, b'<', 0x00, b'?', ..] => utf16(bytes, false),
        _ => Ok(std::borrow::Cow::Borrowed(bytes)),
    }
}

/// Parses an XML part into its root element.
pub fn parse(bytes: &[u8]) -> Result<Element, Error> {
    let mut roots = walk(bytes, &|open| open.len() == 1)?;
    match (roots.pop(), roots.is_empty()) {
        (Some(root), true) => Ok(root),
        _ => Err(Error::Malformed("not exactly one root element".into())),
    }
}

/// The last element at `path` (namespace and name of each element from the root), built as a tree,
/// without building anything else: a part of any size costs only what that element holds.
pub fn find_last(bytes: &[u8], path: &[(&str, &str)]) -> Result<Option<Element>, Error> {
    let at = |open: &OpenPath| {
        open.len() == path.len()
            && open
                .iter()
                .zip(path)
                .all(|((ns, name), (want_ns, want_name))| ns == want_ns && name == want_name)
    };
    Ok(walk(bytes, &at)?.pop())
}

/// The open elements' names, root first: (namespace, local name) of each.
type OpenPath = [(String, String)];

/// Reads a part, building the elements whose open path (from the root) `capture` selects, with
/// everything inside them, in document order.
fn walk(bytes: &[u8], capture: &dyn Fn(&OpenPath) -> bool) -> Result<Vec<Element>, Error> {
    let bytes = utf8(bytes)?;
    let mut reader = NsReader::from_reader(bytes.as_ref());
    reader.config_mut().trim_text(false);
    reader
        .resolver_mut()
        .set_max_namespace_bindings(MAX_BINDINGS);
    let mut built = 0usize;
    // The open elements' names, and the stack of elements being built inside a captured one.
    let mut open: Vec<(String, String)> = Vec::new();
    let mut building: Vec<Element> = Vec::new();
    let mut captured = Vec::new();
    let mut buffer = Vec::new();
    loop {
        let event = reader
            .read_event_into(&mut buffer)
            .map_err(|e| Error::Malformed(e.to_string()))?;
        let empty = matches!(event, Event::Empty(_));
        match &event {
            Event::Start(start) | Event::Empty(start)
                if !building.is_empty() || {
                    let (ns, name) = names(&reader, start);
                    open.push((ns, name));
                    let selected = capture(&open);
                    open.pop();
                    selected
                } =>
            {
                built += 1;
                if built > MAX_ELEMENTS {
                    return Err(Error::TooLarge);
                }
                if open.len() + building.len() > MAX_DEPTH {
                    return Err(Error::Malformed(format!(
                        "nests elements deeper than {MAX_DEPTH} levels"
                    )));
                }
                let element = element(&reader, start)?;
                if building.is_empty() && !empty {
                    open.push((element.namespace.clone(), element.name.clone()));
                }
                match (empty, building.last_mut()) {
                    (true, Some(parent)) => parent.children.push(element),
                    (true, None) => captured.push(element),
                    (false, _) => building.push(element),
                }
            }
            Event::Start(start) => {
                if open.len() > MAX_DEPTH {
                    return Err(Error::Malformed(format!(
                        "nests elements deeper than {MAX_DEPTH} levels"
                    )));
                }
                open.push(names(&reader, start));
            }
            Event::Empty(_) => {}
            Event::End(_) => match building.pop() {
                Some(done) => match building.last_mut() {
                    Some(parent) => parent.children.push(done),
                    None => {
                        open.pop();
                        captured.push(done);
                    }
                },
                None => {
                    open.pop()
                        .ok_or_else(|| Error::Malformed("unbalanced end tag".into()))?;
                }
            },
            Event::Text(text) => {
                // Entity references arrive as their own events, so the text has none.
                if let Some(top) = building.last_mut() {
                    top.text.push_str(&text.xml10_content());
                }
            }
            Event::CData(data) => {
                if let Some(top) = building.last_mut() {
                    top.text.push_str(data);
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
                if let Some(top) = building.last_mut() {
                    top.text.push(character);
                }
            }
            Event::DocType(_) => return Err(Error::Dtd),
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }
    if !open.is_empty() || !building.is_empty() {
        return Err(Error::Malformed("unclosed element".into()));
    }
    Ok(captured)
}

/// An element's namespace and local name.
fn names(reader: &NsReader<&[u8]>, start: &quick_xml::events::BytesStart<'_>) -> (String, String) {
    let (resolved, local) = reader.resolver().resolve_element(start.name());
    let namespace = match resolved {
        ResolveResult::Bound(ns) => ns.as_ref().to_string(),
        _ => String::new(),
    };
    (namespace, local.as_ref().to_string())
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
    fn utf16_parts_are_read_as_characters() {
        let text = "<?xml version=\"1.0\" encoding=\"UTF-16\"?><a>é</a>";
        let mut little = vec![0xFF, 0xFE];
        little.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
        assert_eq!(parse(&little).unwrap().text, "é");
        let big: Vec<u8> = text.encode_utf16().flat_map(u16::to_be_bytes).collect();
        assert_eq!(parse(&big).unwrap().text, "é");
        let dtd = "<?xml version=\"1.0\"?><!DOCTYPE a><a/>";
        let hidden: Vec<u8> = dtd.encode_utf16().flat_map(u16::to_le_bytes).collect();
        assert_eq!(parse(&hidden).unwrap_err(), Error::Dtd);
        assert!(matches!(
            parse(&[0xFF, 0xFE, 0, 0, b'<', 0, 0, 0]),
            Err(Error::Malformed(_))
        ));
    }

    #[test]
    fn find_last_builds_only_its_element() {
        let w = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
        // More elements than a tree may have, then the section: only the section is built.
        let xml = format!(
            "<w:document xmlns:w=\"{w}\"><w:body>{}<w:sectPr><w:pgSz w:w=\"1\"/></w:sectPr></w:body></w:document>",
            "<w:p/>".repeat(MAX_ELEMENTS + 10)
        );
        let path = [(w, "document"), (w, "body"), (w, "sectPr")];
        let section = find_last(xml.as_bytes(), &path).unwrap().unwrap();
        assert_eq!(section.children[0].attr_ns(w, "w"), Some("1"));
        assert_eq!(parse(xml.as_bytes()).unwrap_err(), Error::TooLarge);
        let none = format!("<w:document xmlns:w=\"{w}\"><w:body/></w:document>");
        assert!(find_last(none.as_bytes(), &path).unwrap().is_none());
    }

    #[test]
    fn element_counts_are_limited() {
        let many = format!("<a>{}</a>", "<b/>".repeat(MAX_ELEMENTS));
        assert_eq!(parse(many.as_bytes()).unwrap_err(), Error::TooLarge);
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
