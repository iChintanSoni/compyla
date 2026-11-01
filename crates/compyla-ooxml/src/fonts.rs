//! The font families a template's output uses, and the fonts a template embeds (markdown-spec.md
//! §3, C62).

use std::collections::HashSet;

use crate::dom::{Element, MAIN_A, MAIN_P, MAIN_W, REL_NS};
use crate::inspect::{layouts_of, masters, theme};
use crate::template::{Invalid, Kind, Template};

/// The content type of a font Word obfuscates (ECMA-376 Part 1 §17.8.1).
const OBFUSCATED: &str = "application/vnd.openxmlformats-officedocument.obfuscatedFont";

/// The families §3 checks for output written with `template`, each once (compared by ASCII-lowercase
/// form) in the order first named: the theme's major and minor fonts (latin, then East Asian and
/// complex where not empty); then, for pptx, the typefaces its slide masters and their layouts name;
/// for docx, the fonts its document defaults name. Theme references (`+mj-lt`, Word's `*Theme`
/// attributes), empty names and bullet fonts (`a:buFont`) aren't families. Docx styles join when
/// the docx renderer settles which it writes with (#16).
pub fn families(template: &Template) -> Result<Vec<String>, Invalid> {
    let mut families = Families::default();
    if let Some(theme) = theme(template)?
        && let Some(scheme) = theme.path(&[(MAIN_A, "themeElements"), (MAIN_A, "fontScheme")])
    {
        for group in ["majorFont", "minorFont"] {
            for slot in ["latin", "ea", "cs"] {
                if let Some(font) = scheme.path(&[(MAIN_A, group), (MAIN_A, slot)]) {
                    families.add(font.attr("typeface").unwrap_or_default());
                }
            }
        }
    }
    match template.kind {
        Kind::Pptx => {
            for master in masters(template)? {
                let Some(xml) = template.xml(&master)? else {
                    continue;
                };
                families.typefaces(&xml);
                for layout in layouts_of(template, &master, &xml)? {
                    if let Some(layout) = template.xml(&layout)? {
                        families.typefaces(&layout);
                    }
                }
            }
        }
        Kind::Docx => {
            if let Some(part) = template.related(&template.main, "styles")?
                && let Some(styles) = template.xml(&part)?
                && let Some(fonts) = styles.path(&[
                    (MAIN_W, "docDefaults"),
                    (MAIN_W, "rPrDefault"),
                    (MAIN_W, "rPr"),
                    (MAIN_W, "rFonts"),
                ])
            {
                for slot in ["ascii", "hAnsi", "eastAsia", "cs"] {
                    families.add(fonts.attr_ns(MAIN_W, slot).unwrap_or_default());
                }
            }
        }
        Kind::Xlsx => {}
    }
    Ok(families.list)
}

#[derive(Default)]
struct Families {
    list: Vec<String>,
    seen: HashSet<String>,
}

impl Families {
    fn add(&mut self, family: &str) {
        let family = family.trim();
        if family.is_empty() || family.starts_with('+') {
            return;
        }
        if self.seen.insert(family.to_ascii_lowercase()) {
            self.list.push(family.to_string());
        }
    }

    /// The `a:latin`, `a:ea` and `a:cs` typefaces of a part.
    fn typefaces(&mut self, part: &Element) {
        for element in part.descendants() {
            if element.namespace == MAIN_A && matches!(element.name.as_str(), "latin" | "ea" | "cs")
            {
                self.add(element.attr("typeface").unwrap_or_default());
            }
        }
    }
}

/// The families a pptx template embeds (`p:embeddedFont`), which its pptx output keeps (§3).
pub fn pptx_embedded(template: &Template) -> Result<Vec<String>, Invalid> {
    if template.kind != Kind::Pptx {
        return Ok(Vec::new());
    }
    let presentation = template.xml(&template.main)?.unwrap_or_default();
    Ok(presentation
        .path(&[(MAIN_P, "embeddedFontLst")])
        .into_iter()
        .flat_map(|list| list.children_named(MAIN_P, "embeddedFont"))
        .filter_map(|embedded| embedded.child(MAIN_P, "font"))
        .filter_map(|font| font.attr("typeface"))
        .map(str::to_string)
        .collect())
}

/// The regular font a docx template embeds for `family` (`w:embedRegular` in its font table, the
/// family compared without ASCII case), de-obfuscated as ECMA-376 Part 1 §17.8.1 describes where its
/// content type says it is obfuscated (§3). `None` where it embeds none, or the part is missing or
/// its key unreadable.
pub fn docx_embedded(template: &Template, family: &str) -> Result<Option<Vec<u8>>, Invalid> {
    if template.kind != Kind::Docx {
        return Ok(None);
    }
    let Some(part) = template.related(&template.main, "fontTable")? else {
        return Ok(None);
    };
    let Some(table) = template.xml(&part)? else {
        return Ok(None);
    };
    let Some(embed) = table
        .children_named(MAIN_W, "font")
        .filter(|font| {
            font.attr_ns(MAIN_W, "name")
                .is_some_and(|name| name.trim().eq_ignore_ascii_case(family))
        })
        .find_map(|font| font.child(MAIN_W, "embedRegular"))
    else {
        return Ok(None);
    };
    let Some(id) = embed.attr_ns(REL_NS, "id") else {
        return Ok(None);
    };
    let Some(target) = template
        .relationships(&part)?
        .into_iter()
        .find(|r| !r.external && r.id.trim() == id.trim())
        .map(|r| r.target)
    else {
        return Ok(None);
    };
    let Some(bytes) = template.part(&target) else {
        return Ok(None);
    };
    let mut bytes = bytes.to_vec();
    let obfuscated = template
        .content_type(&target)
        .is_some_and(|t| crate::template::media_type(t).eq_ignore_ascii_case(OBFUSCATED));
    if obfuscated {
        let Some(key) = embed.attr_ns(MAIN_W, "fontKey").and_then(font_key) else {
            return Ok(None);
        };
        deobfuscate(&mut bytes, &key);
    }
    Ok(Some(bytes))
}

/// The key a `w:fontKey` GUID gives (§17.8.1): its sixteen bytes as written, reversed.
fn font_key(guid: &str) -> Option<[u8; 16]> {
    let hex: Vec<u8> = guid
        .bytes()
        .filter(|b| !matches!(b, b'{' | b'}' | b'-'))
        .collect();
    if hex.len() != 32 {
        return None;
    }
    let mut key = [0u8; 16];
    for (i, pair) in hex.chunks(2).enumerate() {
        let text = std::str::from_utf8(pair).ok()?;
        key[15 - i] = u8::from_str_radix(text, 16).ok()?;
    }
    Some(key)
}

/// Undoes Word's obfuscation: the font's first 32 bytes XORed with the key, twice over.
fn deobfuscate(bytes: &mut [u8], key: &[u8; 16]) {
    for (i, byte) in bytes.iter_mut().take(32).enumerate() {
        *byte ^= key[i % 16];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_font_key_is_the_guid_reversed() {
        let key = font_key("{00112233-4455-6677-8899-AABBCCDDEEFF}").unwrap();
        assert_eq!(
            key,
            [
                0xFF, 0xEE, 0xDD, 0xCC, 0xBB, 0xAA, 0x99, 0x88, 0x77, 0x66, 0x55, 0x44, 0x33, 0x22,
                0x11, 0x00
            ]
        );
        assert!(font_key("{not a guid}").is_none());
        let mut bytes = vec![0u8; 40];
        deobfuscate(&mut bytes, &key);
        assert_eq!(&bytes[..16], &key);
        assert_eq!(&bytes[16..32], &key);
        assert!(bytes[32..].iter().all(|&b| b == 0));
    }
}
