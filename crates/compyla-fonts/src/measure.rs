//! Measuring a run of text (markdown-spec.md §3, C62): each character in the face of its class, with
//! the fallback to the run's other faces, then Carlito, then its East Asian Width; pieces shaped
//! with the features viewers apply; widths in EMU, in integer arithmetic.

use rustybuzz::{Direction, Feature, UnicodeBuffer};
use ttf_parser::Tag;
use unicode_bidi::{BidiInfo, Level};
use unicode_script::Script;

use crate::chars::{self, Class};
use crate::face::{Face, Style, carlito};

/// Consolas's advance, in its units per em (§3).
const CONSOLAS_ADVANCE: i64 = 1126;
const CONSOLAS_UNITS: i64 = 2048;

/// A font a run measures characters with.
#[derive(Clone, Copy, Debug)]
pub enum Font<'a> {
    Face(&'a Face),
    /// Consolas without a font file (§3): each character advances 1,126/2,048 em, a Wide or
    /// Fullwidth one 1 em, and one in the zero-advance set nothing. It maps every character.
    Consolas,
}

impl Font<'_> {
    fn same(&self, other: &Font<'_>) -> bool {
        match (self, other) {
            (Font::Face(a), Font::Face(b)) => std::ptr::eq(*a, *b),
            (Font::Consolas, Font::Consolas) => true,
            _ => false,
        }
    }
}

/// A run's fonts by class (§3). An East Asian or complex font that is `None` means the latin one.
#[derive(Clone, Copy, Debug)]
pub struct Fonts<'a> {
    pub latin: Font<'a>,
    pub east_asian: Option<Font<'a>>,
    pub complex: Option<Font<'a>>,
}

/// The ligatures a docx run's `w14:ligatures` names (§3); none in pptx.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ligatures {
    pub standard: bool,
    pub contextual: bool,
    pub historical: bool,
    pub discretional: bool,
}

/// The optional OpenType features a run's text gets (§3); the format decides them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Features {
    pub kerning: bool,
    pub ligatures: Ligatures,
    /// `calt`: in pptx always; in docx only where `w14:cntxtAlts` is on.
    pub contextual_alternates: bool,
}

/// A run: text with one size and one set of run properties (§3).
#[derive(Clone, Copy, Debug)]
pub struct Run<'a> {
    pub text: &'a str,
    pub fonts: Fonts<'a>,
    /// The size in EMU (1 pt = 12,700 EMU).
    pub size: i64,
    /// The size of complex characters in EMU: docx `w:szCs`; in pptx the same as `size`.
    pub complex_size: i64,
    /// The run's style, for Carlito where no run font maps a character.
    pub style: Style,
    pub features: Features,
    /// Character spacing in EMU, once after each character outside the zero-advance set.
    pub spacing: i64,
    /// The document's language (§2), as a BCP 47 tag.
    pub language: Option<&'a str>,
    /// The paragraph's direction: its `rtl` (pptx) or `w:bidi` (docx) where set, else its first
    /// strong character's, which the caller resolves over the whole paragraph. `None` resolves it
    /// from the run's own text, which is right only for a run that is its whole paragraph.
    pub rtl: Option<bool>,
}

/// The font a character is measured with, or `None` where no face maps it.
#[derive(Clone, Copy, Debug)]
enum Pick<'a> {
    Font(Font<'a>),
    Unmapped,
}

impl Pick<'_> {
    fn same(&self, other: &Pick<'_>) -> bool {
        match (self, other) {
            (Pick::Font(a), Pick::Font(b)) => a.same(b),
            (Pick::Unmapped, Pick::Unmapped) => true,
            _ => false,
        }
    }
}

/// The faces one measurement shapes with, each parsed once.
struct Shapers<'a> {
    faces: Vec<(&'a Face, rustybuzz::Face<'a>)>,
}

impl<'a> Shapers<'a> {
    fn get(&mut self, face: &'a Face) -> &rustybuzz::Face<'a> {
        let index = match self.faces.iter().position(|(f, _)| std::ptr::eq(*f, face)) {
            Some(index) => index,
            None => {
                self.faces.push((face, face.shaper()));
                self.faces.len() - 1
            }
        };
        &self.faces[index].1
    }

    /// Whether `font` maps `c` to a glyph other than `.notdef` (§3).
    fn maps(&mut self, font: Font<'a>, c: char) -> bool {
        match font {
            Font::Face(face) => self.get(face).glyph_index(c).is_some_and(|g| g.0 != 0),
            Font::Consolas => true,
        }
    }

    /// The advance of `face`'s U+0020 in its units, with its variations, where it maps one.
    fn space(&mut self, face: &'a Face) -> Option<i64> {
        let shaper = self.get(face);
        let glyph = shaper.glyph_index(' ').filter(|g| g.0 != 0)?;
        shaper.glyph_hor_advance(glyph).map(i64::from)
    }
}

/// The font for a character of `class` (§3): the run's font for the class, else the first of its
/// latin, East Asian and complex fonts, then Carlito, that maps it.
fn pick<'a>(shapers: &mut Shapers<'a>, run: &Run<'a>, c: char, class: Class) -> Pick<'a> {
    let primary = class_font(run, class);
    if shapers.maps(primary, c) {
        return Pick::Font(primary);
    }
    let fallbacks = [
        Some(run.fonts.latin),
        run.fonts.east_asian,
        run.fonts.complex,
        Some(Font::Face(carlito(run.style))),
    ];
    fallbacks
        .into_iter()
        .flatten()
        .find(|&font| shapers.maps(font, c))
        .map_or(Pick::Unmapped, Pick::Font)
}

fn class_font<'a>(run: &Run<'a>, class: Class) -> Font<'a> {
    match class {
        Class::Latin => run.fonts.latin,
        Class::EastAsian => run.fonts.east_asian.unwrap_or(run.fonts.latin),
        Class::Complex => run.fonts.complex.unwrap_or(run.fonts.latin),
    }
}

/// The width of `run` in EMU (§3).
pub fn measure<'a>(run: &Run<'a>) -> i64 {
    let chars: Vec<(usize, char)> = run.text.char_indices().collect();
    if chars.is_empty() {
        return 0;
    }
    let plain: Vec<char> = chars.iter().map(|&(_, c)| c).collect();
    let classes = chars::classes(&plain);
    let bases = chars::bases(&plain);
    let scripts = chars::scripts(&plain);
    let levels = levels(run);

    let mut shapers = Shapers { faces: Vec::new() };
    // A mark is measured with the face its base is measured with (§3).
    let mut picks: Vec<Pick<'a>> = plain
        .iter()
        .zip(&classes)
        .map(|(&c, &class)| pick(&mut shapers, run, c, class))
        .collect();
    for (i, &base) in bases.iter().enumerate() {
        if base != i {
            picks[i] = picks[base];
        }
    }
    // A character no face maps that advances nothing stays in the piece around it, so it doesn't
    // break shaping across it (kerning across a soft hyphen, say); the shaper hides it, or shapes
    // it as `.notdef`, which advances nothing. Not a mark: one whose base no face maps stays
    // unmapped, and advances nothing, rather than be shaped alone by a face that maps it.
    let joins = |i: usize| chars::advances_nothing(plain[i]) && !chars::is_mark(plain[i]);
    let mut last = None;
    for (i, pick) in picks.iter_mut().enumerate() {
        match *pick {
            Pick::Unmapped if joins(i) => {
                if let Some(font) = last {
                    *pick = font;
                }
            }
            Pick::Font(_) => last = Some(*pick),
            Pick::Unmapped => {}
        }
    }
    if let Some(first) = picks.iter().position(|p| matches!(p, Pick::Font(_))) {
        for i in 0..first {
            if joins(i) {
                picks[i] = picks[first];
            }
        }
    }
    let size = |class: Class| match class {
        Class::Complex => run.complex_size,
        _ => run.size,
    };

    // A mark takes its base's script and level too, so it stays in its base's piece (§3).
    let script = |i: usize| scripts[bases[i]];
    let level = |i: usize| levels[chars[bases[i]].0];
    let mut width = 0;
    let mut start = 0;
    for end in 1..=chars.len() {
        let split = end == chars.len()
            || matches!(picks[end], Pick::Unmapped)
            || !picks[end].same(&picks[start])
            || size(classes[end]) != size(classes[start])
            || script(end) != script(start)
            || level(end) != level(start);
        if !split {
            continue;
        }
        let bytes = chars[start].0..chars.get(end).map_or(run.text.len(), |&(b, _)| b);
        let text = &run.text[bytes.clone()];
        let piece_size = size(classes[start]);
        let piece = match picks[start] {
            Pick::Font(Font::Face(face)) => shaped(
                shapers.get(face),
                face.units_per_em(),
                text,
                piece_size,
                script(start),
                level(start),
                run,
            ),
            Pick::Font(Font::Consolas) => consolas(text, piece_size),
            Pick::Unmapped => {
                let space = match class_font(run, classes[start]) {
                    Font::Face(face) => shapers.space(face).map(|a| (a, face.units_per_em())),
                    Font::Consolas => None,
                };
                unmapped(plain[start], space, piece_size)
            }
        };
        let spaced = text
            .chars()
            .filter(|&c| !chars::advances_nothing(c))
            .count() as i64;
        width += piece + run.spacing * spaced;
        start = end;
    }
    width
}

/// The bidi level of each byte of the run's text (UAX #9).
fn levels(run: &Run<'_>) -> Vec<Level> {
    let paragraph = run
        .rtl
        .map(|rtl| if rtl { Level::rtl() } else { Level::ltr() });
    BidiInfo::new(run.text, paragraph).levels
}

/// `units × size ÷ units per em`, rounded down, in exact integer arithmetic.
fn scale(units: i64, size: i64, units_per_em: i64) -> i64 {
    (i128::from(units) * i128::from(size)).div_euclid(i128::from(units_per_em)) as i64
}

/// The features applied besides those a shaper applies by default (§3): the format's optional
/// ones, and off the ones shapers turn on by themselves that §3 excludes.
fn features(f: &Features) -> Vec<Feature> {
    let set = |tag: &[u8; 4], on: bool| Feature::new(Tag::from_bytes(tag), u32::from(on), ..);
    vec![
        set(b"kern", f.kerning),
        set(b"liga", f.ligatures.standard),
        set(b"clig", f.ligatures.contextual),
        set(b"hlig", f.ligatures.historical),
        set(b"dlig", f.ligatures.discretional),
        set(b"calt", f.contextual_alternates),
        set(b"frac", false),
        set(b"numr", false),
        set(b"dnom", false),
        set(b"rand", false),
        set(b"trak", false),
    ]
}

/// A piece shaped with `face`: its advances in font units, scaled to `size`. A mark the face maps
/// neither alone nor composed with its base advances nothing (§3). Only characters the face maps
/// are shaped with it, marks aside, so its `.notdef` glyphs are exactly those marks; shaping merges
/// a mark into its base's cluster, so they're known by glyph, not cluster.
fn shaped(
    shaper: &rustybuzz::Face<'_>,
    units_per_em: i64,
    text: &str,
    size: i64,
    script: Script,
    level: Level,
    run: &Run<'_>,
) -> i64 {
    let mut buffer = UnicodeBuffer::new();
    buffer.push_str(text);
    buffer.set_direction(if level.is_rtl() {
        Direction::RightToLeft
    } else {
        Direction::LeftToRight
    });
    if !matches!(script, Script::Common | Script::Inherited | Script::Unknown)
        && let Some(script) = rustybuzz::Script::from_iso15924_tag(Tag::from_bytes_lossy(
            script.short_name().as_bytes(),
        ))
    {
        buffer.set_script(script);
    }
    if let Some(language) = run.language.and_then(|l| l.parse().ok()) {
        buffer.set_language(language);
    }
    buffer.guess_segment_properties();
    let glyphs = rustybuzz::shape(shaper, &features(&run.features), buffer);
    let units: i64 = glyphs
        .glyph_infos()
        .iter()
        .zip(glyphs.glyph_positions())
        .filter(|(info, _)| info.glyph_id != 0)
        .map(|(_, position)| i64::from(position.x_advance))
        .sum();
    scale(units, size, units_per_em)
}

/// A piece in Consolas without a font file (§3).
fn consolas(text: &str, size: i64) -> i64 {
    let units = text
        .chars()
        .map(|c| {
            if chars::advances_nothing(c) {
                0
            } else if chars::is_wide(c) {
                CONSOLAS_UNITS
            } else {
                CONSOLAS_ADVANCE
            }
        })
        .sum();
    scale(units, size, CONSOLAS_UNITS)
}

/// A character no face maps (§3), measured on its own: nothing for the zero-advance set, 1 em if
/// Wide or Fullwidth, a space separator as its class face's U+0020 (`space`: its advance and units
/// per em) where that maps one, else ½ em.
fn unmapped(c: char, space: Option<(i64, i64)>, size: i64) -> i64 {
    if chars::advances_nothing(c) {
        return 0;
    }
    if chars::is_wide(c) {
        return size;
    }
    if chars::is_space(c)
        && let Some((advance, units_per_em)) = space
    {
        return scale(advance, size, units_per_em);
    }
    scale(1, size, 2)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PT: i64 = 12_700;

    fn run<'a>(text: &'a str, fonts: Fonts<'a>) -> Run<'a> {
        Run {
            text,
            fonts,
            size: 12 * PT,
            complex_size: 12 * PT,
            style: Style::Regular,
            features: Features::default(),
            spacing: 0,
            language: None,
            rtl: None,
        }
    }

    fn carlito_fonts() -> Fonts<'static> {
        Fonts {
            latin: Font::Face(carlito(Style::Regular)),
            east_asian: None,
            complex: None,
        }
    }

    #[test]
    fn widths_are_shaped_advances_scaled_and_floored() {
        // Carlito's "H" advances 1276 of 2048 units: at 12 pt, 1276 × 152,400 ÷ 2048 = 94,951.17.
        let face = carlito(Style::Regular);
        let h = i64::from(
            face.shaper()
                .glyph_hor_advance(face.shaper().glyph_index('H').unwrap())
                .unwrap(),
        );
        assert_eq!(measure(&run("H", carlito_fonts())), h * 12 * PT / 2048);
        assert_eq!(measure(&run("", carlito_fonts())), 0);
    }

    #[test]
    fn kerning_applies_only_when_asked() {
        let plain = run("AVATAR WAVE", carlito_fonts());
        let kerned = Run {
            features: Features {
                kerning: true,
                ..Features::default()
            },
            ..plain
        };
        assert!(measure(&kerned) < measure(&plain));
    }

    #[test]
    fn east_asian_text_carlito_lacks_measures_one_em() {
        // No run face maps 漢, so it advances 1 em.
        assert_eq!(measure(&run("漢字", carlito_fonts())), 2 * 12 * PT);
    }

    #[test]
    fn unmapped_spaces_marks_and_others() {
        let fonts = carlito_fonts();
        let face = carlito(Style::Regular);
        // U+3000 is Fullwidth: 1 em. U+2009 THIN SPACE, if Carlito lacks it, as its U+0020.
        assert_eq!(measure(&run("\u{3000}", fonts)), 12 * PT);
        let thin = measure(&run("\u{2009}", fonts));
        if !face.maps('\u{2009}') {
            let space = Shapers { faces: Vec::new() }.space(face).unwrap();
            assert_eq!(thin, space * 12 * PT / 2048);
        }
        // A mark Carlito lacks, after a base it maps, advances nothing: q + U+0301.
        assert!(!face.maps('\u{0301}'));
        assert_eq!(measure(&run("q\u{0301}", fonts)), measure(&run("q", fonts)));
        // A default-ignorable character no face maps advances nothing.
        assert_eq!(measure(&run("\u{E0001}", fonts)), 0);
    }

    #[test]
    fn a_zero_width_character_no_face_maps_doesnt_break_kerning() {
        // Carlito lacks U+00AD SOFT HYPHEN: "T­o" kerns like "To".
        assert!(!carlito(Style::Regular).maps('\u{00AD}'));
        let kerned = |text| Run {
            features: Features {
                kerning: true,
                ..Features::default()
            },
            ..run(text, carlito_fonts())
        };
        assert_eq!(measure(&kerned("T\u{00AD}o")), measure(&kerned("To")));
        assert!(measure(&kerned("To")) < measure(&run("To", carlito_fonts())));
    }

    #[test]
    fn consolas_advances_its_fixed_width() {
        let fonts = Fonts {
            latin: Font::Consolas,
            east_asian: None,
            complex: None,
        };
        // Four characters at 1,126/2,048 em, a wide one at 1 em, a mark at nothing.
        assert_eq!(
            measure(&run("abcd漢\u{0301}", fonts)),
            (4 * 1126 + 2048) * 12 * PT / 2048
        );
    }

    #[test]
    fn spacing_counts_characters_that_advance() {
        let plain = run("ab\u{200B}c", carlito_fonts());
        let spaced = Run {
            spacing: 100,
            ..plain
        };
        assert_eq!(measure(&spaced) - measure(&plain), 3 * 100);
    }

    #[test]
    fn complex_characters_take_the_complex_size() {
        let face = carlito(Style::Regular);
        let fonts = Fonts {
            latin: Font::Face(face),
            east_asian: None,
            complex: None,
        };
        let small = run("\u{05D0}", fonts);
        let large = Run {
            complex_size: 24 * PT,
            ..small
        };
        // Carlito lacks Hebrew: an unmapped character is ½ em, at the complex size.
        assert_eq!(measure(&small), 6 * PT);
        assert_eq!(measure(&large), 12 * PT);
    }
}
