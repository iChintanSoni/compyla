//! Editing the template's `presentation.xml` in place (architecture.md D12): its slide list is
//! replaced, and what refers to the template's own slides (custom shows, sections) is removed.
//! Everything else stays byte for byte, so nothing the template set is lost.

use quick_xml::Reader;
use quick_xml::events::Event;

const RELATIONSHIPS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

/// An element's byte range and qualified name.
struct Child {
    name: String,
    start: usize,
    end: usize,
}

/// An element's parts: its name, where its start tag ends, its children, where its end tag starts,
/// and the prefix it binds to the relationships namespace, if any.
struct Parts {
    name: String,
    open_end: usize,
    children: Vec<Child>,
    close: usize,
    relationships: Option<String>,
    /// The root is an empty element (`<a/>`): nothing can go inside it as it is.
    empty: bool,
}

fn parts(bytes: &[u8]) -> Result<Parts, String> {
    let mut reader = Reader::from_reader(bytes);
    reader.config_mut().trim_text(false);
    let mut buffer = Vec::new();
    let (mut depth, mut root, mut open_end, mut close) = (0usize, None, 0, bytes.len());
    let mut relationships = None;
    let mut empty = false;
    let mut children = Vec::new();
    let mut current: Option<(String, usize)> = None;
    loop {
        let before = reader.buffer_position() as usize;
        let event = reader
            .read_event_into(&mut buffer)
            .map_err(|e| e.to_string())?;
        let after = reader.buffer_position() as usize;
        match event {
            Event::Start(start) => {
                let name = start.name().as_ref().to_string();
                match depth {
                    0 => {
                        root = Some(name);
                        open_end = after;
                        for attribute in start.attributes().flatten() {
                            if let Some(prefix) = attribute.key.as_ref().strip_prefix("xmlns:")
                                && &*attribute.value == RELATIONSHIPS
                            {
                                relationships = Some(prefix.to_string());
                            }
                        }
                    }
                    1 => current = Some((name, before)),
                    _ => {}
                }
                depth += 1;
            }
            // A root with no content: nothing to edit inside it.
            Event::Empty(start) if depth == 0 => {
                root = Some(start.name().as_ref().to_string());
                (open_end, close, empty) = (after, after, true);
            }
            Event::Empty(start) if depth == 1 => children.push(Child {
                name: start.name().as_ref().to_string(),
                start: before,
                end: after,
            }),
            Event::End(_) => {
                depth = depth.checked_sub(1).ok_or("unbalanced XML")?;
                match depth {
                    0 => close = before,
                    1 => {
                        if let Some((name, start)) = current.take() {
                            children.push(Child {
                                name,
                                start,
                                end: after,
                            });
                        }
                    }
                    _ => {}
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }
    Ok(Parts {
        name: root.ok_or("no root element")?,
        open_end,
        children,
        close,
        relationships,
        empty,
    })
}

fn local(name: &str) -> &str {
    name.rsplit_once(':').map_or(name, |(_, local)| local)
}

/// `presentation.xml` with its slide list replaced by `slides` (each a slide id and the
/// relationship id of its part), custom shows removed, and section lists removed from its
/// extensions.
pub fn edit(bytes: &[u8], slides: &[(u32, String)]) -> Result<Vec<u8>, String> {
    let bytes = utf8(bytes)?;
    let bytes = bytes.as_slice();
    let root = parts(bytes)?;
    if root.empty {
        return Err("the presentation element is empty".into());
    }
    let prefix = root
        .name
        .rsplit_once(':')
        .map(|(p, _)| format!("{p}:"))
        .unwrap_or_default();
    // The template's own prefix for relationship ids where it binds one, as PowerPoint writes it.
    let (r, declaration) = match &root.relationships {
        Some(r) => (r.clone(), String::new()),
        None => ("cmp".to_string(), format!(" xmlns:cmp=\"{RELATIONSHIPS}\"")),
    };
    let mut list = format!("<{prefix}sldIdLst{declaration}>");
    for (id, relationship) in slides {
        list.push_str(&format!(
            "<{prefix}sldId id=\"{id}\" {r}:id=\"{relationship}\"/>"
        ));
    }
    list.push_str(&format!("</{prefix}sldIdLst>"));
    let list = if slides.is_empty() {
        String::new()
    } else {
        list
    };

    // CT_Presentation's sequence: the slide list comes before the slide size.
    const AFTER: [&str; 11] = [
        "sldSz",
        "notesSz",
        "smartTags",
        "embeddedFontLst",
        "custShowLst",
        "photoAlbum",
        "custDataLst",
        "kinsoku",
        "defaultTextStyle",
        "modifyVerifier",
        "extLst",
    ];
    let mut out = bytes[..root.open_end].to_vec();
    let mut cursor = root.open_end;
    let mut placed = false;
    for child in &root.children {
        out.extend(&bytes[cursor..child.start]);
        cursor = child.end;
        let name = local(&child.name);
        if !placed && AFTER.contains(&name) {
            out.extend(list.as_bytes());
            placed = true;
        }
        match name {
            "sldIdLst" | "custShowLst" => {}
            "extLst" => out.extend(without_sections(&bytes[child.start..child.end])?),
            _ => out.extend(&bytes[child.start..child.end]),
        }
    }
    out.extend(&bytes[cursor..root.close]);
    if !placed {
        out.extend(list.as_bytes());
    }
    out.extend(&bytes[root.close..]);
    Ok(out)
}

/// A part as UTF-8 (§10.4 allows UTF-16 parts): transcoded, with its declaration saying so.
fn utf8(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let text = compyla_ooxml::dom::utf8(bytes).map_err(|_| "an XML part isn't UTF-8 or UTF-16")?;
    let transcoded = matches!(text, std::borrow::Cow::Owned(_));
    let mut text = text.into_owned();
    if transcoded
        && text.starts_with(b"<?xml")
        && let Some(end) = text.windows(2).position(|w| w == b"?>")
        && let Some(value) = encoding_value(&text[..end])
    {
        text.splice(value, b"UTF-8".iter().copied());
    }
    Ok(text)
}

/// Where the value of an XML declaration's `encoding` pseudo-attribute lies, between its quotes
/// (XML 1.0 §2.8, §4.3.3: spaces may surround `=`).
fn encoding_value(declaration: &[u8]) -> Option<std::ops::Range<usize>> {
    let name = declaration.windows(8).position(|w| w == b"encoding")? + 8;
    let mut at = name;
    let space = |b: &u8| matches!(b, b' ' | b'\t' | b'\r' | b'\n');
    while declaration.get(at).is_some_and(space) {
        at += 1;
    }
    if declaration.get(at) != Some(&b'=') {
        return None;
    }
    at += 1;
    while declaration.get(at).is_some_and(space) {
        at += 1;
    }
    let quote = *declaration.get(at).filter(|q| matches!(q, b'"' | b'\''))?;
    let start = at + 1;
    let length = declaration.get(start..)?.iter().position(|&b| b == quote)?;
    Some(start..start + length)
}

/// `presProps.xml` without show settings that name slides or custom shows the output doesn't have:
/// the show plays every slide.
pub fn show_properties(bytes: &[u8]) -> Result<Vec<u8>, String> {
    without_in(bytes, "showPr", &["custShow", "sldRg"])
}

/// `viewProps.xml` without the outline view's list of the template's slides.
pub fn view_properties(bytes: &[u8]) -> Result<Vec<u8>, String> {
    without_in(bytes, "outlineViewPr", &["sldLst"])
}

/// A part whose root's `child` elements lose their children named `names`.
fn without_in(bytes: &[u8], child: &str, names: &[&str]) -> Result<Vec<u8>, String> {
    let bytes = utf8(bytes)?;
    let root = parts(&bytes)?;
    let mut out = bytes[..root.open_end].to_vec();
    let mut cursor = root.open_end;
    for element in &root.children {
        out.extend(&bytes[cursor..element.start]);
        cursor = element.end;
        let range = &bytes[element.start..element.end];
        if local(&element.name) == child {
            out.extend(without(range, names)?);
        } else {
            out.extend(range);
        }
    }
    out.extend(&bytes[cursor..]);
    Ok(out)
}

/// An element without its children named `names`.
fn without(bytes: &[u8], names: &[&str]) -> Result<Vec<u8>, String> {
    let element = parts(bytes)?;
    let mut out = bytes[..element.open_end].to_vec();
    let mut cursor = element.open_end;
    for child in &element.children {
        out.extend(&bytes[cursor..child.start]);
        cursor = child.end;
        if !names.contains(&local(&child.name)) {
            out.extend(&bytes[child.start..child.end]);
        }
    }
    out.extend(&bytes[cursor..]);
    Ok(out)
}

/// An `extLst` without the extensions that list sections, which name the template's slides.
fn without_sections(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let list = parts(bytes)?;
    let mut out = bytes[..list.open_end].to_vec();
    let mut cursor = list.open_end;
    for child in &list.children {
        out.extend(&bytes[cursor..child.start]);
        cursor = child.end;
        let ext = &bytes[child.start..child.end];
        if !ext.windows(10).any(|w| w == b"sectionLst") {
            out.extend(ext);
        }
    }
    out.extend(&bytes[cursor..]);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_the_slide_list_and_keeps_the_rest() {
        let xml = br#"<?xml version="1.0"?>
<p:presentation xmlns:p="urn:p" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><p:sldMasterIdLst><p:sldMasterId id="1" r:id="rId1"/></p:sldMasterIdLst><p:sldIdLst><p:sldId id="256" r:id="rId9"/></p:sldIdLst><p:sldSz cx="1" cy="2"/><p:custShowLst><p:custShow/></p:custShowLst><p:extLst><p:ext uri="a"><s:sectionLst xmlns:s="urn:s"/></p:ext><p:ext uri="b"><x/></p:ext></p:extLst></p:presentation>"#;
        let out = String::from_utf8(edit(xml, &[(256, "cmpSlide1".into())]).unwrap()).unwrap();
        assert!(
            out.contains(r#"</p:sldMasterIdLst><p:sldIdLst><p:sldId id="256" r:id="cmpSlide1"/></p:sldIdLst><p:sldSz cx="1" cy="2"/>"#),
            "{out}"
        );
        assert!(!out.contains("rId9") && !out.contains("custShow") && !out.contains("sectionLst"));
        assert!(out.contains(r#"<p:ext uri="b"><x/></p:ext>"#));
    }

    #[test]
    fn utf16_parts_are_edited_as_utf8() {
        let xml = r#"<?xml version="1.0" encoding="UTF-16"?><presentation xmlns="urn:p"><sldSz cx="1" cy="2"/></presentation>"#;
        let mut utf16 = vec![0xFF, 0xFE];
        utf16.extend(xml.encode_utf16().flat_map(u16::to_le_bytes));
        let out = String::from_utf8(edit(&utf16, &[(256, "s1".into())]).unwrap()).unwrap();
        assert!(
            out.starts_with(r#"<?xml version="1.0" encoding="UTF-8"?>"#),
            "{out}"
        );
        assert!(out.contains("<sldIdLst"));
    }

    #[test]
    fn every_spelling_of_a_utf16_declaration_becomes_utf8() {
        for declaration in [
            r#"<?xml version="1.0" encoding = 'utf-16LE'?>"#,
            r#"<?xml version="1.0" encoding="Utf-16" standalone="yes"?>"#,
            "<?xml version=\"1.0\" encoding\n=\"UTF-16BE\"?>",
        ] {
            let xml = format!("{declaration}<p:presentationPr xmlns:p=\"urn:p\"/>");
            let mut utf16 = vec![0xFF, 0xFE];
            utf16.extend(xml.encode_utf16().flat_map(u16::to_le_bytes));
            let out = String::from_utf8(show_properties(&utf16).unwrap()).unwrap();
            let head = &out[..out.find("?>").unwrap()];
            assert!(
                head.contains("UTF-8") && !head.to_lowercase().contains("16"),
                "{out}"
            );
        }
    }

    #[test]
    fn an_empty_presentation_is_refused() {
        assert!(
            edit(
                br#"<p:presentation xmlns:p="urn:p"/>"#,
                &[(256, "s1".into())]
            )
            .is_err()
        );
    }

    #[test]
    fn the_outline_view_loses_its_slides() {
        let xml = br#"<p:viewPr xmlns:p="urn:p"><p:outlineViewPr><p:cViewPr/><p:sldLst><p:sld r:id="rId1"/></p:sldLst></p:outlineViewPr><p:gridSpacing/></p:viewPr>"#;
        let out = String::from_utf8(view_properties(xml).unwrap()).unwrap();
        assert_eq!(
            out,
            r#"<p:viewPr xmlns:p="urn:p"><p:outlineViewPr><p:cViewPr/></p:outlineViewPr><p:gridSpacing/></p:viewPr>"#
        );
    }

    #[test]
    fn an_empty_root_is_kept() {
        let xml = br#"<?xml version="1.0"?><p:presentationPr xmlns:p="urn:p"/>"#;
        assert_eq!(show_properties(xml).unwrap(), xml.to_vec());
    }

    #[test]
    fn show_settings_naming_slides_are_removed() {
        let xml = br#"<p:presentationPr xmlns:p="urn:p"><p:showPr loop="1"><p:custShow id="0"/></p:showPr><p:extLst/></p:presentationPr>"#;
        let out = String::from_utf8(show_properties(xml).unwrap()).unwrap();
        assert_eq!(
            out,
            r#"<p:presentationPr xmlns:p="urn:p"><p:showPr loop="1"></p:showPr><p:extLst/></p:presentationPr>"#
        );
    }

    #[test]
    fn declares_a_prefix_where_the_template_binds_none() {
        let xml = br#"<presentation xmlns="urn:p"><sldMasterIdLst/><sldSz cx="1" cy="2"/></presentation>"#;
        let out = String::from_utf8(edit(xml, &[(256, "s1".into())]).unwrap()).unwrap();
        assert!(
            out.contains(r#"<sldMasterIdLst/><sldIdLst xmlns:cmp="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sldId id="256" cmp:id="s1"/></sldIdLst><sldSz"#),
            "{out}"
        );
    }
}
