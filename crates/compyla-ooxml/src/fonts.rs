//! The font families a template's output uses, and the fonts a template embeds (markdown-spec.md
//! §3, C62).

use std::collections::{HashMap, HashSet};

use crate::dom::{Element, MAIN_A, MAIN_P, MAIN_W, REL_NS};
use crate::inspect::{layouts_of, masters, theme};
use crate::template::{Invalid, Kind, Template};

/// The content type of a font Word obfuscates (ECMA-376 Part 1 §17.8.1).
const OBFUSCATED: &str = "application/vnd.openxmlformats-officedocument.obfuscatedFont";

/// The families §3 checks for output written with `template`, each once (compared by ASCII-lowercase
/// form) in the order first named. First the theme's major and minor fonts (latin, then East Asian
/// and complex where not empty). Then, for pptx, the typefaces named by the parts its output keeps:
/// the presentation's default text style, its table styles, and each slide master with its layouts,
/// and the notes and handout masters, each master with its own theme's fonts. For docx, the fonts
/// its document defaults name. Theme references (`+mj-lt`, Word's `*Theme` attributes), empty names
/// and bullet fonts (`a:buFont`) aren't families. Docx styles join when the docx renderer settles
/// which it writes with (#16). Each part is read once, however many parts list it.
pub fn families(template: &Template) -> Result<Vec<String>, Invalid> {
    let mut families = Families::default();
    if let Some(theme) = theme(template)? {
        families.theme(&theme);
    }
    match template.kind {
        Kind::Pptx => {
            let presentation = template.xml(&template.main)?.unwrap_or_default();
            if let Some(style) = presentation.child(MAIN_P, "defaultTextStyle") {
                families.typefaces(style);
            }
            if let Some(part) = template.related(&template.main, "tableStyles")?
                && let Some(styles) = template.xml(&part)?
            {
                families.typefaces(&styles);
            }
            let mut masters = masters(template)?;
            for kind in ["notesMaster", "handoutMaster"] {
                masters.extend(template.related(&template.main, kind)?);
            }
            let mut read = HashSet::new();
            let mut first = |part: &str| read.insert(part.to_ascii_lowercase());
            for master in masters {
                if !first(&master) {
                    continue;
                }
                let Some(xml) = template.xml(&master)? else {
                    continue;
                };
                if let Some(part) = template.related(&master, "theme")?
                    && first(&part)
                    && let Some(theme) = template.xml(&part)?
                    && theme.is(MAIN_A, "theme")
                {
                    families.theme(&theme);
                }
                families.typefaces(&xml);
                for layout in layouts_of(template, &master, &xml)? {
                    if first(&layout)
                        && let Some(layout) = template.xml(&layout)?
                    {
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
    /// A family as the template names it (§3).
    fn add(&mut self, family: &str) {
        if family.is_empty() || family.starts_with('+') {
            return;
        }
        if self.seen.insert(family.to_ascii_lowercase()) {
            self.list.push(family.to_string());
        }
    }

    /// A theme's major and minor fonts.
    fn theme(&mut self, theme: &Element) {
        let Some(scheme) = theme.path(&[(MAIN_A, "themeElements"), (MAIN_A, "fontScheme")]) else {
            return;
        };
        for group in ["majorFont", "minorFont"] {
            for slot in ["latin", "ea", "cs"] {
                if let Some(font) = scheme.path(&[(MAIN_A, group), (MAIN_A, slot)]) {
                    self.add(font.attr("typeface").unwrap_or_default());
                }
            }
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

/// The families a pptx template embeds (`p:embeddedFont`), ASCII-lowercase: those whose regular
/// face (`p:regular`) is a part of the package, which its pptx output keeps (§3).
pub fn pptx_embedded(template: &Template) -> Result<HashSet<String>, Invalid> {
    if template.kind != Kind::Pptx {
        return Ok(HashSet::new());
    }
    let presentation = template.xml(&template.main)?.unwrap_or_default();
    let Some(list) = presentation.child(MAIN_P, "embeddedFontLst") else {
        return Ok(HashSet::new());
    };
    let parts: HashMap<String, String> = template
        .relationships(&template.main)?
        .into_iter()
        .filter(|r| !r.external && template.part(&r.target).is_some())
        .map(|r| (r.id, r.target))
        .collect();
    Ok(list
        .children_named(MAIN_P, "embeddedFont")
        .filter(|embedded| {
            embedded
                .child(MAIN_P, "regular")
                .and_then(|regular| regular.attr_ns(REL_NS, "id"))
                .is_some_and(|id| parts.contains_key(id))
        })
        .filter_map(|embedded| embedded.child(MAIN_P, "font")?.attr("typeface"))
        .map(str::to_ascii_lowercase)
        .collect())
}

/// The regular fonts a docx template embeds (`w:embedRegular` in its font table), by family. They're
/// read whatever the font resolver finds, so an unreadable font table makes the template invalid
/// either way (§3).
#[derive(Default)]
pub struct Embedded<'t> {
    fonts: HashMap<String, EmbeddedFont<'t>>,
}

/// A font a docx template embeds: its bytes as stored, and the key that de-obfuscates them where
/// its content type says they're obfuscated.
#[derive(Clone, Copy)]
pub struct EmbeddedFont<'t> {
    stored: &'t [u8],
    key: Option<[u8; 16]>,
}

impl<'t> Embedded<'t> {
    /// The fonts a docx template embeds, and none for other templates. A font whose relationship
    /// or part is missing, or whose key is unreadable, isn't embedded.
    pub fn read(template: &'t Template) -> Result<Embedded<'t>, Invalid> {
        let mut embedded = Embedded::default();
        if template.kind != Kind::Docx {
            return Ok(embedded);
        }
        let Some(part) = template.related(&template.main, "fontTable")? else {
            return Ok(embedded);
        };
        let Some(table) = template.xml(&part)? else {
            return Ok(embedded);
        };
        let targets: HashMap<String, String> = template
            .relationships(&part)?
            .into_iter()
            .filter(|r| !r.external)
            .map(|r| (r.id, r.target))
            .collect();
        for font in table.children_named(MAIN_W, "font") {
            let (Some(name), Some(embed)) = (
                font.attr_ns(MAIN_W, "name"),
                font.child(MAIN_W, "embedRegular"),
            ) else {
                continue;
            };
            let Some(target) = embed.attr_ns(REL_NS, "id").and_then(|id| targets.get(id)) else {
                continue;
            };
            let Some(stored) = template.part(target) else {
                continue;
            };
            let obfuscated = template
                .content_type(target)
                .is_some_and(|t| crate::template::media_type(t).eq_ignore_ascii_case(OBFUSCATED));
            let key = match obfuscated {
                true => match embed.attr_ns(MAIN_W, "fontKey").and_then(font_key) {
                    Some(key) => Some(key),
                    None => continue,
                },
                false => None,
            };
            // The first a family's name embeds wins.
            embedded
                .fonts
                .entry(name.to_ascii_lowercase())
                .or_insert(EmbeddedFont { stored, key });
        }
        Ok(embedded)
    }

    /// The regular font embedded for `family`, compared by its ASCII-lowercase form.
    pub fn regular(&self, family: &str) -> Option<EmbeddedFont<'t>> {
        self.fonts.get(&family.to_ascii_lowercase()).copied()
    }
}

impl EmbeddedFont<'_> {
    /// Its size in bytes, known before they're copied.
    pub fn size(&self) -> usize {
        self.stored.len()
    }

    /// Its bytes, de-obfuscated as ECMA-376 Part 1 §17.8.1 describes where they're stored
    /// obfuscated.
    pub fn bytes(&self) -> Vec<u8> {
        let mut bytes = self.stored.to_vec();
        if let Some(key) = &self.key {
            deobfuscate(&mut bytes, key);
        }
        bytes
    }
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

    #[test]
    fn the_families_are_named_as_the_template_names_them() {
        // lint/144: the theme's fonts with its East Asian and complex ones, the default text style,
        // the table styles, the master's body style, its layouts' typefaces (a duplicate in another
        // case, a stand-in in lower case), the notes master's theme and text; each once.
        let pptx = Template::read(include_bytes!(
            "../../../spec/conformance/lint/144-pptx-fonts-unresolved/brand.potx"
        ))
        .unwrap();
        assert_eq!(
            families(&pptx).unwrap(),
            [
                "Brand Display",
                "Brand Mincho",
                "Brand Sans",
                "Brand Naskh",
                "Brand Default",
                "Brand Table",
                "Source Serif 4",
                "calibri",
                "Broken Font",
                "Brand Gothic",
                "Brand Ghost",
                "Brand Remote",
                "Brand Lost",
                "Brand Notes",
                "Brand Notes Body",
            ]
        );
        // Of its embedded fonts, only the one whose regular face is in the package counts.
        assert_eq!(
            pptx_embedded(&pptx).unwrap(),
            HashSet::from(["brand sans".to_string()])
        );

        // lint/145: the theme's fonts, then the document defaults' four slots.
        let docx = Template::read(include_bytes!(
            "../../../spec/conformance/lint/145-docx-embedded-fonts/brand.dotx"
        ))
        .unwrap();
        assert_eq!(
            families(&docx).unwrap(),
            [
                "Brand Serif",
                "Brand Sans",
                "Brand Body",
                "Brand Ming",
                "Brand Arabic"
            ]
        );
        let embedded = Embedded::read(&docx).unwrap();
        assert!(embedded.regular("BRAND SERIF").is_some());
        assert!(embedded.regular("Brand Body").is_none());
    }
}
