//! Font faces (markdown-spec.md §3, C62): font bytes within §12.1's limit, the face a family and
//! style choose in them, and the variation coordinates a style sets.

use std::borrow::Cow;
use std::sync::OnceLock;

use rustybuzz::Variation;
use ttf_parser::Tag;
use ttf_parser::os2::Style as Os2Style;

/// The largest font file read (§12.1).
pub const MAX_FONT_BYTES: usize = 64 * 1024 * 1024;
/// The most faces a collection, tables a face and character-map encoding records a face may have
/// (§12.1): far beyond real fonts, and what keeps reading and measuring a hostile one linear.
pub const MAX_FACES: u32 = 256;
pub const MAX_TABLES: u16 = 512;
pub const MAX_CMAP_RECORDS: u16 = 64;

/// A style (§3): what a family resolves per.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Style {
    Regular,
    Bold,
    Italic,
    BoldItalic,
}

impl Style {
    pub fn is_bold(self) -> bool {
        matches!(self, Style::Bold | Style::BoldItalic)
    }

    pub fn is_italic(self) -> bool {
        matches!(self, Style::Italic | Style::BoldItalic)
    }
}

/// Why font bytes aren't a face the engine reads: they count as not found (§3, §12.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Unreadable {
    /// Over `MAX_FONT_BYTES`.
    TooLarge,
    /// Not a TrueType or OpenType font or collection, or a face that doesn't parse.
    NotAFont,
}

impl std::fmt::Display for Unreadable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Unreadable::TooLarge => write!(f, "is larger than {} MiB", MAX_FONT_BYTES >> 20),
            Unreadable::NotAFont => write!(f, "isn't a TrueType or OpenType font"),
        }
    }
}

/// A face to measure with: font bytes, the face's index in them, and its variation coordinates.
#[derive(Clone, Debug)]
pub struct Face {
    data: Cow<'static, [u8]>,
    index: u32,
    variations: Vec<Variation>,
    units_per_em: i64,
    ascent_descent: (u32, u32),
}

impl Face {
    /// The face `bytes` hold for `family` and `style` (§3): in a collection, the first whose family
    /// name (name ID 1, any record) matches the family and whose OS/2 bold and italic bits match
    /// the style, else the first whose family matches, else the first; a single font whatever its
    /// names say. A variable font gets `wght` 400 or 700, and for italic `ital` 1, or the minimum
    /// `slnt` where it has no `ital`, each clamped to the axis's range.
    pub fn load(
        bytes: impl Into<Cow<'static, [u8]>>,
        family: &str,
        style: Style,
    ) -> Result<Face, Unreadable> {
        let data = bytes.into();
        if data.len() > MAX_FONT_BYTES {
            return Err(Unreadable::TooLarge);
        }
        let index = match ttf_parser::fonts_in_collection(&data) {
            // A collection must hold the 4-byte offsets its 12-byte header declares, so a forged
            // count can't make the scan longer than the input.
            Some(count) if count > MAX_FACES || u64::from(count) * 4 + 12 > data.len() as u64 => {
                return Err(Unreadable::NotAFont);
            }
            Some(count) => choose(&data, count, family, style),
            None => 0,
        };
        if !within_limits(&data, index) {
            return Err(Unreadable::NotAFont);
        }
        let face = ttf_parser::Face::parse(&data, index).map_err(|_| Unreadable::NotAFont)?;
        let cmap_records = face.tables().cmap.map_or(0, |cmap| cmap.subtables.len());
        if cmap_records > MAX_CMAP_RECORDS {
            return Err(Unreadable::NotAFont);
        }
        let units_per_em = i64::from(face.units_per_em());
        if units_per_em <= 0 {
            return Err(Unreadable::NotAFont);
        }
        let variations = variations(&face, style);
        let ascent_descent = ascent_descent(&face);
        Ok(Face {
            data,
            index,
            variations,
            units_per_em,
            ascent_descent,
        })
    }

    pub fn units_per_em(&self) -> i64 {
        self.units_per_em
    }

    /// Its ascent and descent in its units, for a last line's height (§3, §8.6): the OS/2 table's
    /// `usWinAscent` and `usWinDescent`, else, where it has no OS/2 table long enough to hold them,
    /// the magnitudes of `hhea`'s ascender and descender; where both are 0, Carlito's.
    pub fn ascent_descent(&self) -> (u32, u32) {
        self.ascent_descent
    }

    /// The face for shaping, with its variations set.
    pub(crate) fn shaper(&self) -> rustybuzz::Face<'_> {
        let mut face = rustybuzz::Face::from_slice(&self.data, self.index)
            .expect("a loaded face parses again");
        if !self.variations.is_empty() {
            face.set_variations(&self.variations);
        }
        face
    }

    /// Whether the face maps `c` to a glyph other than `.notdef` (§3).
    pub fn maps(&self, c: char) -> bool {
        ttf_parser::Face::parse(&self.data, self.index)
            .ok()
            .and_then(|face| face.glyph_index(c))
            .is_some_and(|glyph| glyph.0 != 0)
    }
}

/// Carlito's `usWinAscent` and `usWinDescent`, Calibri's too.
pub(crate) const CARLITO_ASCENT_DESCENT: (u32, u32) = (1950, 550);

fn ascent_descent(face: &ttf_parser::Face) -> (u32, u32) {
    // The OS/2 win values are unsigned, read from the table's bytes: ttf-parser's accessors read
    // them as signed and negate the descent, which overflows for 0x8000. The table is found as
    // ttf-parser's own tables are, by scanning the directory in order and taking the last record,
    // so an unsorted directory doesn't hide it.
    let raw = face.raw_face();
    let os2 = raw
        .table_records
        .into_iter()
        .filter(|record| record.tag == ttf_parser::Tag::from_bytes(b"OS/2"))
        .last()
        .and_then(|record| {
            let start = usize::try_from(record.offset).ok()?;
            let end = start.checked_add(usize::try_from(record.length).ok()?)?;
            raw.data.get(start..end)
        })
        .filter(|table| table.len() >= 78)
        .map(|table| {
            let word = |at: usize| u32::from(u16::from_be_bytes([table[at], table[at + 1]]));
            (word(74), word(76))
        });
    let metrics = os2.unwrap_or_else(|| {
        let hhea = face.tables().hhea;
        (
            u32::from(hhea.ascender.unsigned_abs()),
            u32::from(hhea.descender.unsigned_abs()),
        )
    });
    match metrics {
        (0, 0) => CARLITO_ASCENT_DESCENT,
        metrics => metrics,
    }
}

/// Where face `index` starts: 0 for a single font, else its offset in a collection.
fn face_offset(data: &[u8], index: u32) -> Option<usize> {
    if ttf_parser::fonts_in_collection(data).is_none() {
        return Some(0);
    }
    let at = 12 + 4 * index as usize;
    let bytes = data.get(at..at + 4)?;
    Some(u32::from_be_bytes(bytes.try_into().ok()?) as usize)
}

/// Whether face `index` declares no more tables than `MAX_TABLES`, read from its header before
/// parsing, since parsing walks every table record.
fn within_limits(data: &[u8], index: u32) -> bool {
    face_offset(data, index)
        .and_then(|offset| data.get(offset + 4..offset + 6))
        .is_some_and(|count| u16::from_be_bytes([count[0], count[1]]) <= MAX_TABLES)
}

/// The index of the face a collection of `count` faces holds for `family` and `style`.
fn choose(data: &[u8], count: u32, family: &str, style: Style) -> u32 {
    let mut family_match = None;
    let mut seen = std::collections::HashSet::new();
    for index in 0..count {
        // A face listed twice gives the same answer twice: read it once.
        if !within_limits(data, index) || !seen.insert(face_offset(data, index)) {
            continue;
        }
        let Ok(face) = ttf_parser::Face::parse(data, index) else {
            continue;
        };
        if !names_family(&face, family) {
            continue;
        }
        let os2 = face.tables().os2;
        let bold = os2.is_some_and(|t| t.is_bold());
        let italic = os2.is_some_and(|t| t.style() == Os2Style::Italic);
        if bold == style.is_bold() && italic == style.is_italic() {
            return index;
        }
        family_match.get_or_insert(index);
    }
    family_match.unwrap_or(0)
}

/// Whether one of the face's family-name records (name ID 1) is `family`, compared by ASCII-lowercase
/// form (§3). A record that isn't Unicode is read as ASCII, its other bytes matching nothing.
fn names_family(face: &ttf_parser::Face<'_>, family: &str) -> bool {
    // A record of the wrong length can't match, and isn't decoded: a face may have thousands.
    let utf16 = family.encode_utf16().count() * 2;
    let bytes = family.chars().count();
    face.names()
        .into_iter()
        .filter(|name| name.name_id == ttf_parser::name_id::FAMILY)
        .filter(|name| {
            let len = name.name.len();
            if name.is_unicode() {
                len == utf16
            } else {
                len == bytes
            }
        })
        .any(|name| {
            let text = name.to_string().unwrap_or_else(|| {
                name.name
                    .iter()
                    .map(|&b| if b.is_ascii() { b as char } else { '\u{FFFD}' })
                    .collect()
            });
            text.eq_ignore_ascii_case(family)
        })
}

/// The variation coordinates `style` sets (§3).
fn variations(face: &ttf_parser::Face<'_>, style: Style) -> Vec<Variation> {
    let axes: Vec<_> = face.variation_axes().into_iter().collect();
    let axis = |tag: &[u8; 4]| axes.iter().find(|a| a.tag == Tag::from_bytes(tag));
    let mut out = Vec::new();
    let mut set = |tag: &[u8; 4], value: f32| {
        if let Some(a) = axis(tag) {
            out.push(Variation {
                tag: a.tag,
                value: value.clamp(a.min_value, a.max_value),
            });
        }
    };
    set(b"wght", if style.is_bold() { 700.0 } else { 400.0 });
    if style.is_italic() {
        if axis(b"ital").is_some() {
            set(b"ital", 1.0);
        } else if let Some(slnt) = axis(b"slnt") {
            let min = slnt.min_value;
            set(b"slnt", min);
        }
    }
    out
}

const CARLITO: [&[u8]; 4] = [
    include_bytes!("../fonts/Carlito-Regular.ttf"),
    include_bytes!("../fonts/Carlito-Bold.ttf"),
    include_bytes!("../fonts/Carlito-Italic.ttf"),
    include_bytes!("../fonts/Carlito-BoldItalic.ttf"),
];

/// Carlito in `style`, the bundled stand-in for Calibri and Calibri Light (§3), unmodified.
pub fn carlito(style: Style) -> &'static Face {
    static FACES: [OnceLock<Face>; 4] = [
        OnceLock::new(),
        OnceLock::new(),
        OnceLock::new(),
        OnceLock::new(),
    ];
    let index = style as usize;
    FACES[index].get_or_init(|| {
        Face::load(CARLITO[index], "Carlito", style).expect("the bundled Carlito parses")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn carlito_loads_in_every_style() {
        for style in [
            Style::Regular,
            Style::Bold,
            Style::Italic,
            Style::BoldItalic,
        ] {
            let face = carlito(style);
            assert_eq!(face.units_per_em(), 2048);
            assert!(face.maps('A') && !face.maps('\u{4E00}'));
        }
    }

    #[test]
    fn bytes_that_arent_a_font_are_unreadable() {
        assert_eq!(
            Face::load(b"not a font".to_vec(), "X", Style::Regular).unwrap_err(),
            Unreadable::NotAFont
        );
        assert_eq!(
            Face::load(vec![0u8; MAX_FONT_BYTES + 1], "X", Style::Regular).unwrap_err(),
            Unreadable::TooLarge
        );
    }
}
