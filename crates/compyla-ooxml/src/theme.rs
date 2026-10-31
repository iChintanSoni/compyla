//! A template's theme tokens (markdown-spec.md §9.2, §10.4): its twelve colors and its heading and
//! body fonts. A workbook takes them as its own theme, rebuilt from them, so nothing else of the
//! template comes along.

use crate::dom::{Element, MAIN_A};
use crate::template::{Invalid, Template};
use crate::xml::Writer;

/// The theme's color slots, in `a:clrScheme` order.
pub const SLOTS: [&str; 12] = [
    "dk1", "lt1", "dk2", "lt2", "accent1", "accent2", "accent3", "accent4", "accent5", "accent6",
    "hlink", "folHlink",
];

/// Slots by index in [`SLOTS`].
pub const LT1: usize = 1;
pub const ACCENT1: usize = 4;
pub const HLINK: usize = 10;

/// The longest font name a workbook can hold (SpreadsheetML `name/@val`, MS-OI29500 §2.1.711).
const MAX_FONT_NAME: usize = 31;

/// WCAG 2's minimum contrast for normal text (1.4.3, AA).
pub const READABLE: f64 = 4.5;

/// Office's default theme colors, for a slot a theme leaves out or can't be read.
const DEFAULT_COLORS: [&str; 12] = [
    "000000", "FFFFFF", "44546A", "E7E6E6", "4472C4", "ED7D31", "A5A5A5", "FFC000", "5B9BD5",
    "70AD47", "0563C1", "954F72",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    /// RRGGBB, uppercase, one per slot of [`SLOTS`].
    pub colors: [String; 12],
    /// The heading font.
    pub major: Fonts,
    /// The body font.
    pub minor: Fonts,
}

/// A font scheme entry's typefaces; the Latin one is never empty.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fonts {
    pub latin: String,
    pub east_asian: String,
    pub complex: String,
}

/// A color slot's color: an sRGB value, or a system color's last value, as six hex digits.
pub fn color(slot: &Element) -> Option<String> {
    slot.child(MAIN_A, "srgbClr")
        .and_then(|c| c.attr("val"))
        .or_else(|| slot.child(MAIN_A, "sysClr").and_then(|c| c.attr("lastClr")))
        .filter(|c| c.len() == 6 && c.bytes().all(|b| b.is_ascii_hexdigit()))
        .map(str::to_ascii_uppercase)
}

impl Theme {
    /// The template's theme (its main part's, or a pptx's first master's), if it has one.
    pub fn of(template: &Template) -> Result<Option<Theme>, Invalid> {
        Ok(crate::inspect::theme(template)?.map(|theme| Theme::from(&theme)))
    }

    /// The tokens of an `a:theme` element. A slot it leaves out, or whose color can't be read,
    /// takes Office's default, and a font scheme entry without a Latin typeface a workbook can name
    /// (a blank one, or one longer than 31 characters) Office's (Calibri Light, Calibri).
    pub fn from(theme: &Element) -> Theme {
        let elements = theme.child(MAIN_A, "themeElements");
        let scheme = elements.and_then(|e| e.child(MAIN_A, "clrScheme"));
        let colors = std::array::from_fn(|i| {
            scheme
                .and_then(|s| s.child(MAIN_A, SLOTS[i]))
                .and_then(color)
                .unwrap_or_else(|| DEFAULT_COLORS[i].to_string())
        });
        let fonts = elements.and_then(|e| e.child(MAIN_A, "fontScheme"));
        let font = |entry: &str, default: &str| {
            let entry = fonts.and_then(|f| f.child(MAIN_A, entry));
            let typeface = |script: &str| {
                entry
                    .and_then(|e| e.child(MAIN_A, script))
                    .and_then(|s| s.attr("typeface"))
                    .unwrap_or_default()
                    .to_string()
            };
            let latin = typeface("latin");
            Fonts {
                latin: if latin.trim().is_empty() || latin.chars().count() > MAX_FONT_NAME {
                    default.to_string()
                } else {
                    latin
                },
                east_asian: typeface("ea"),
                complex: typeface("cs"),
            }
        };
        Theme {
            colors,
            major: font("majorFont", "Calibri Light"),
            minor: font("minorFont", "Calibri"),
        }
    }

    /// The color in slot `index`, or the slot's default when it isn't six hex digits (the fields
    /// are public, so a caller may have set anything).
    pub fn color(&self, index: usize) -> &str {
        let color = &self.colors[index];
        if color.len() == 6 && color.bytes().all(|b| b.is_ascii_hexdigit()) {
            color
        } else {
            DEFAULT_COLORS[index]
        }
    }

    /// The WCAG 2 contrast ratio between two of its colors, by slot.
    pub fn contrast(&self, a: usize, b: usize) -> f64 {
        let (a, b) = (luminance(self.color(a)), luminance(self.color(b)));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    /// The theme as a part (`a:theme`): these colors and fonts, with a plain format scheme.
    pub fn to_xml(&self) -> Vec<u8> {
        let mut xml = Writer::new();
        xml.open("a:theme", &[("xmlns:a", MAIN_A), ("name", "Compyla")]);
        xml.open("a:themeElements", &[]);
        xml.open("a:clrScheme", &[("name", "Compyla")]);
        for (index, slot) in SLOTS.iter().enumerate() {
            xml.open(&format!("a:{slot}"), &[]);
            xml.empty("a:srgbClr", &[("val", self.color(index))]);
            xml.close(&format!("a:{slot}"));
        }
        xml.close("a:clrScheme");
        xml.open("a:fontScheme", &[("name", "Compyla")]);
        for (entry, fonts) in [("a:majorFont", &self.major), ("a:minorFont", &self.minor)] {
            xml.open(entry, &[]);
            xml.empty("a:latin", &[("typeface", &fonts.latin)]);
            xml.empty("a:ea", &[("typeface", &fonts.east_asian)]);
            xml.empty("a:cs", &[("typeface", &fonts.complex)]);
            xml.close(entry);
        }
        xml.close("a:fontScheme");
        xml.raw(FORMAT_SCHEME);
        xml.close("a:themeElements");
        xml.empty("a:objectDefaults", &[]);
        xml.empty("a:extraClrSchemeLst", &[]);
        xml.close("a:theme");
        xml.finish()
    }
}

/// WCAG 2's relative luminance of an RRGGBB color (checked by [`Theme::color`]).
fn luminance(color: &str) -> f64 {
    let channel = |i: usize| {
        let byte = color
            .get(i..i + 2)
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        let value = f64::from(byte.unwrap_or(0)) / 255.0;
        if value <= 0.039_28 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(0) + 0.7152 * channel(2) + 0.0722 * channel(4)
}

/// Solid fills and plain lines in the theme's colors: what a theme part must have, and nothing
/// that refers to other parts.
const FORMAT_SCHEME: &str = r#"<a:fmtScheme name="Compyla"><a:fillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:fillStyleLst><a:lnStyleLst><a:ln w="6350" cap="flat" cmpd="sng" algn="ctr"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:prstDash val="solid"/><a:miter lim="800000"/></a:ln><a:ln w="12700" cap="flat" cmpd="sng" algn="ctr"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:prstDash val="solid"/><a:miter lim="800000"/></a:ln><a:ln w="19050" cap="flat" cmpd="sng" algn="ctr"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:prstDash val="solid"/><a:miter lim="800000"/></a:ln></a:lnStyleLst><a:effectStyleLst><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle></a:effectStyleLst><a:bgFillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:bgFillStyleLst></a:fmtScheme>"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_read_with_defaults_for_what_is_missing() {
        let theme = crate::dom::parse(
            br#"<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><a:themeElements><a:clrScheme name="x"><a:dk1><a:sysClr val="windowText" lastClr="000000"/></a:dk1><a:dk2><a:srgbClr val="1f2a44"/></a:dk2><a:accent1><a:srgbClr val="nothex"/></a:accent1></a:clrScheme><a:fontScheme name="x"><a:majorFont><a:latin typeface="Brand &amp; Co"/><a:ea typeface=""/><a:cs typeface=""/></a:majorFont></a:fontScheme></a:themeElements></a:theme>"#,
        )
        .unwrap();
        let theme = Theme::from(&theme);
        assert_eq!(theme.colors[0], "000000");
        assert_eq!(theme.colors[2], "1F2A44");
        assert_eq!(
            theme.colors[4], "4472C4",
            "an unreadable accent takes the default"
        );
        assert_eq!(
            theme.colors[1], "FFFFFF",
            "a missing slot takes the default"
        );
        assert_eq!(theme.major.latin, "Brand & Co");
        assert_eq!(theme.minor.latin, "Calibri");
        // Written and read back, the tokens are the same.
        let written = crate::dom::parse(&theme.to_xml()).unwrap();
        assert_eq!(Theme::from(&written), theme);
    }

    #[test]
    fn typefaces_a_workbook_cant_name_take_the_default() {
        let theme = crate::dom::parse(
            br#"<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><a:themeElements><a:fontScheme name="x"><a:majorFont><a:latin typeface="  "/></a:majorFont><a:minorFont><a:latin typeface="Proxima Nova Alt Condensed Semibold Extra"/></a:minorFont></a:fontScheme></a:themeElements></a:theme>"#,
        )
        .unwrap();
        let theme = Theme::from(&theme);
        assert_eq!(
            (theme.major.latin.as_str(), theme.minor.latin.as_str()),
            ("Calibri Light", "Calibri")
        );
    }

    #[test]
    fn a_malformed_color_is_never_used() {
        let mut theme = Theme::from(
            &crate::dom::parse(
                br#"<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"/>"#,
            )
            .unwrap(),
        );
        theme.colors[ACCENT1] = "ab\u{e9}".into();
        assert_eq!(theme.color(ACCENT1), "4472C4");
        assert!(theme.contrast(LT1, ACCENT1) > 1.0);
        let xml = String::from_utf8(theme.to_xml()).unwrap();
        assert!(xml.contains(r#"<a:accent1><a:srgbClr val="4472C4"/>"#));
    }

    #[test]
    fn contrast_is_wcags() {
        let mut theme = Theme::from(
            &crate::dom::parse(
                br#"<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"/>"#,
            )
            .unwrap(),
        );
        assert!(
            (theme.contrast(0, LT1) - 21.0).abs() < 1e-9,
            "black on white"
        );
        theme.colors[ACCENT1] = "C55A11".into();
        assert!(
            theme.contrast(LT1, ACCENT1) < READABLE,
            "white on this orange"
        );
        theme.colors[ACCENT1] = "4472C4".into();
        assert!(theme.contrast(LT1, ACCENT1) > READABLE);
    }
}
