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
}

fn parts(bytes: &[u8]) -> Result<Parts, String> {
    let mut reader = Reader::from_reader(bytes);
    reader.config_mut().trim_text(false);
    let mut buffer = Vec::new();
    let (mut depth, mut root, mut open_end, mut close) = (0usize, None, 0, bytes.len());
    let mut relationships = None;
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
    })
}

fn local(name: &str) -> &str {
    name.rsplit_once(':').map_or(name, |(_, local)| local)
}

/// `presentation.xml` with its slide list replaced by `slides` (each a slide id and the
/// relationship id of its part), custom shows removed, and section lists removed from its
/// extensions.
pub fn edit(bytes: &[u8], slides: &[(u32, String)]) -> Result<Vec<u8>, String> {
    let root = parts(bytes)?;
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
    fn declares_a_prefix_where_the_template_binds_none() {
        let xml = br#"<presentation xmlns="urn:p"><sldMasterIdLst/><sldSz cx="1" cy="2"/></presentation>"#;
        let out = String::from_utf8(edit(xml, &[(256, "s1".into())]).unwrap()).unwrap();
        assert!(
            out.contains(r#"<sldMasterIdLst/><sldIdLst xmlns:cmp="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sldId id="256" cmp:id="s1"/></sldIdLst><sldSz"#),
            "{out}"
        );
    }
}
