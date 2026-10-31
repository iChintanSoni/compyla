//! Editing the template's `presentation.xml` in place (architecture.md D12): its slide list is
//! replaced, and what refers to the template's own slides (custom shows, sections) is removed.
//! Everything else stays byte for byte, so nothing the template set is lost.

use quick_xml::Reader;
use quick_xml::events::Event;

/// An element's byte range and qualified name.
struct Child {
    name: String,
    start: usize,
    end: usize,
}

/// The root element's name, where its start tag ends, its children, and where its end tag starts.
fn children(bytes: &[u8]) -> Result<(String, usize, Vec<Child>, usize), String> {
    let mut reader = Reader::from_reader(bytes);
    reader.config_mut().trim_text(false);
    let mut buffer = Vec::new();
    let (mut depth, mut root, mut open_end, mut close) = (0usize, None, 0, bytes.len());
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
    Ok((root.ok_or("no root element")?, open_end, children, close))
}

fn local(name: &str) -> &str {
    name.rsplit_once(':').map_or(name, |(_, local)| local)
}

/// `presentation.xml` with its slide list replaced by `slides` (each a slide id and the
/// relationship id of its part), custom shows removed, and section lists removed from its
/// extensions.
pub fn edit(bytes: &[u8], slides: &[(u32, String)]) -> Result<Vec<u8>, String> {
    let (root, open_end, children, close) = children(bytes)?;
    let prefix = root
        .rsplit_once(':')
        .map(|(p, _)| format!("{p}:"))
        .unwrap_or_default();
    let mut list = format!(
        "<{prefix}sldIdLst xmlns:cmp=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\">"
    );
    for (id, relationship) in slides {
        list.push_str(&format!(
            "<{prefix}sldId id=\"{id}\" cmp:id=\"{relationship}\"/>"
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
    let mut out = bytes[..open_end].to_vec();
    let mut cursor = open_end;
    let mut placed = false;
    for child in &children {
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
    out.extend(&bytes[cursor..close]);
    if !placed {
        out.extend(list.as_bytes());
    }
    out.extend(&bytes[close..]);
    Ok(out)
}

/// An `extLst` without the extensions that list sections, which name the template's slides.
fn without_sections(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let (_, open_end, children, close) = children(bytes)?;
    let mut out = bytes[..open_end].to_vec();
    let mut cursor = open_end;
    for child in &children {
        out.extend(&bytes[cursor..child.start]);
        cursor = child.end;
        let ext = &bytes[child.start..child.end];
        if !ext.windows(10).any(|w| w == b"sectionLst") {
            out.extend(ext);
        }
    }
    out.extend(&bytes[cursor..]);
    let _ = close;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_the_slide_list_and_keeps_the_rest() {
        let xml = br#"<?xml version="1.0"?>
<p:presentation xmlns:p="urn:p" xmlns:r="urn:r"><p:sldMasterIdLst><p:sldMasterId id="1" r:id="rId1"/></p:sldMasterIdLst><p:sldIdLst><p:sldId id="256" r:id="rId9"/></p:sldIdLst><p:sldSz cx="1" cy="2"/><p:custShowLst><p:custShow/></p:custShowLst><p:extLst><p:ext uri="a"><s:sectionLst xmlns:s="urn:s"/></p:ext><p:ext uri="b"><x/></p:ext></p:extLst></p:presentation>"#;
        let out = String::from_utf8(edit(xml, &[(256, "cmpSlide1".into())]).unwrap()).unwrap();
        assert!(out.contains(r#"<p:sldMasterIdLst><p:sldMasterId id="1" r:id="rId1"/></p:sldMasterIdLst><p:sldIdLst xmlns:cmp="#));
        assert!(out.contains(
            r#"<p:sldId id="256" cmp:id="cmpSlide1"/></p:sldIdLst><p:sldSz cx="1" cy="2"/>"#
        ));
        assert!(!out.contains("rId9") && !out.contains("custShow") && !out.contains("sectionLst"));
        assert!(out.contains(r#"<p:ext uri="b"><x/></p:ext>"#));
    }

    #[test]
    fn inserts_a_slide_list_where_there_was_none() {
        let xml = br#"<presentation xmlns="urn:p"><sldMasterIdLst/><sldSz cx="1" cy="2"/></presentation>"#;
        let out = String::from_utf8(edit(xml, &[(256, "s1".into())]).unwrap()).unwrap();
        assert!(
            out.contains(r#"<sldMasterIdLst/><sldIdLst xmlns:cmp="#),
            "{out}"
        );
    }
}
