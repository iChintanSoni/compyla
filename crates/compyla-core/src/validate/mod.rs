//! Validation (markdown-spec.md §5–§9): profile rules over the IR. Shared machinery lives here; each
//! profile is a submodule.

mod flow;
pub mod pptx;
pub mod xlsx;

use std::collections::{HashMap, HashSet};

use crate::diagnostic::{Diagnostic, Sink};
use crate::ir::{Block, Directive, Document, Position, plain_text};
use crate::parse::Sidecar;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Format {
    Docx,
    Pdf,
    Pptx,
    Xlsx,
}

impl Format {
    pub fn parse(name: &str) -> Option<Format> {
        match name {
            "docx" => Some(Format::Docx),
            "pdf" => Some(Format::Pdf),
            "pptx" => Some(Format::Pptx),
            "xlsx" => Some(Format::Xlsx),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Format::Docx => "docx",
            Format::Pdf => "pdf",
            Format::Pptx => "pptx",
            Format::Xlsx => "xlsx",
        }
    }
}

/// What the host's asset resolver returns (markdown-spec.md §10.3).
#[derive(Debug, Clone)]
pub enum Asset {
    Bytes(Vec<u8>),
    /// Exists, without bytes (permitted for `lint` only).
    Exists,
    NotFound,
}

/// Everything a profile needs to validate a document.
pub struct Input<'a> {
    pub document: &'a Document,
    pub sidecar: &'a Sidecar,
    pub assets: &'a dyn Fn(&str) -> Asset,
    /// A supplied pptx template's layouts, for layout binding (§8.5); `None` with the house
    /// template.
    pub template: Option<&'a pptx::layout::TemplateLayouts>,
}

/// What a profile composed (architecture.md D15): the model its renderer writes.
#[derive(Debug)]
pub enum Composed {
    Flow,
    Pptx(pptx::model::Deck),
    Xlsx(xlsx::Workbook),
}

/// The profile's diagnostics, at the positions of the nodes involved, and its composed model.
/// Callers that report the diagnostics renumber the document first and collect into a located sink
/// (`crate::locate`, §10.7, §11.3).
pub fn validate(input: &Input<'_>, format: Format, out: &mut Sink) -> Composed {
    directives(&input.document.body, format, out);
    ragged_tables(&input.document.body, out);
    match format {
        Format::Docx | Format::Pdf => {
            images(input, out);
            flow::validate(input, format, out);
            Composed::Flow
        }
        Format::Pptx => {
            images(input, out);
            Composed::Pptx(pptx::validate(input, out))
        }
        Format::Xlsx => Composed::Xlsx(xlsx::validate(input, out)),
    }
}

// ---------------------------------------------------------------------------------------------
// Directive registry (§1.5) and arguments (§1.6).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectiveKind {
    Marker,
    Annotation,
    Container,
}

/// The directive registry: each name's kind and the formats that use it.
pub fn directive(name: &str) -> Option<(DirectiveKind, &'static [Format])> {
    use DirectiveKind::*;
    use Format::*;
    Some(match name {
        "toc" | "pagebreak" => (Marker, &[Docx, Pdf]),
        "col" => (Marker, &[Pptx]),
        "image" => (Annotation, &[Docx, Pdf, Pptx]),
        "layout" => (Annotation, &[Pptx]),
        "columns" | "anchor" | "table" => (Annotation, &[Xlsx]),
        "notes" => (Container, &[Pptx]),
        _ => return None,
    })
}

/// Whether `format` uses the directive (otherwise it is ignored quietly, or unknown).
pub fn uses(name: &str, format: Format) -> bool {
    directive(name).is_some_and(|(_, formats)| formats.contains(&format))
}

/// `unknown_directive` everywhere, and `unknown_directive_option` for arguments to a directive that
/// takes none (§1.5, §1.6).
fn directives(blocks: &[Block], format: Format, out: &mut Sink) {
    for_each_block(blocks, &mut |block| {
        let Block::Directive(d) = block else { return };
        let line = d.position.map(|p| p.line);
        match directive(&d.name) {
            None => out.push(at(
                Diagnostic::new("unknown_directive", format!("Unknown directive `{}`; its content renders as if it were absent.", d.name))
                    .hint("Known directives: toc, pagebreak, image, layout, notes, col, columns, anchor, table."),
                line,
            )),
            Some(_) if matches!(d.name.as_str(), "toc" | "pagebreak" | "notes" | "col") && !d.args.is_empty() && uses(&d.name, format) => out.push(at(
                Diagnostic::new("unknown_directive_option", format!("`{}` takes no arguments; `{}` is ignored.", d.name, d.args)),
                line,
            )),
            Some(_) => {}
        }
    });
}

/// A parsed option list (§1.6): `key=value` pairs and bare flags, keys lowercased.
pub struct Options {
    pub entries: Vec<(String, Option<String>)>,
    pub repeated: Option<String>,
}

pub fn options(args: &str) -> Options {
    let mut entries: Vec<(String, Option<String>)> = Vec::new();
    let mut keys = HashSet::new();
    let mut repeated = None;
    for token in args.split([',', ' ', '\t']).filter(|t| !t.is_empty()) {
        let (key, value) = match token.split_once('=') {
            Some((k, v)) => (k.to_ascii_lowercase(), Some(v.to_string())),
            None => (token.to_ascii_lowercase(), None),
        };
        if !keys.insert(key.clone()) {
            repeated.get_or_insert(key.clone());
        }
        entries.push((key, value));
    }
    Options { entries, repeated }
}

pub fn at(diagnostic: Diagnostic, line: Option<usize>) -> Diagnostic {
    match line {
        Some(line) => diagnostic.at_line(line),
        None => diagnostic,
    }
}

/// Unicode simple case folding, for comparing names case-insensitively. Per character: the
/// single-character lowercase mapping, plus the folds that differ from it (CaseFolding.txt, status
/// C and S).
pub fn fold(text: &str) -> String {
    text.chars().map(fold_char).collect()
}

fn fold_char(c: char) -> char {
    match c {
        'ς' => 'σ',
        'ϐ' => 'β',
        'ϑ' => 'θ',
        'ϕ' => 'φ',
        'ϖ' => 'π',
        'ϰ' => 'κ',
        'ϱ' => 'ρ',
        'ϵ' => 'ε',
        'ſ' => 's',
        'µ' => 'μ',
        '\u{345}' | '\u{1FBE}' => 'ι',
        'ẛ' => 'ṡ',
        // Cyrillic variant letters, which are their own lowercase.
        '\u{1C80}' => 'в',
        '\u{1C81}' => 'д',
        '\u{1C82}' => 'о',
        '\u{1C83}' => 'с',
        '\u{1C84}' | '\u{1C85}' => 'т',
        '\u{1C86}' => 'ъ',
        '\u{1C87}' => 'ѣ',
        '\u{1C88}' => 'ꙋ',
        // Status S since Unicode 15.1.
        '\u{1FD3}' => '\u{390}',
        '\u{1FE3}' => '\u{3B0}',
        '\u{FB05}' => '\u{FB06}',
        // No simple folding: İ (only a full one), and uppercase Cherokee, which lowercase Cherokee
        // folds to.
        '\u{130}' | '\u{13A0}'..='\u{13F5}' => c,
        '\u{13F8}'..='\u{13FD}' => char::from_u32(c as u32 - 8).unwrap_or(c),
        '\u{AB70}'..='\u{ABBF}' => char::from_u32(c as u32 - 0xAB70 + 0x13A0).unwrap_or(c),
        _ => {
            let mut lower = c.to_lowercase();
            match (lower.next(), lower.next()) {
                (Some(l), None) => l,
                _ => c,
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Shared rules.

/// §5: ragged rows are repaired, with a warning at each row's line.
fn ragged_tables(blocks: &[Block], out: &mut Sink) {
    for_each_block(blocks, &mut |block| {
        let Block::Table(table) = block else { return };
        for (index, row) in table.rows.iter().enumerate() {
            if row.len() != table.header.len() {
                let line = table.position.map(|p| row_line(p, index));
                out.push(at(
                    Diagnostic::new(
                        "table_not_rectangular",
                        format!("This row has {} cells; the header has {}. It is padded or the table widened.", row.len(), table.header.len()),
                    )
                    .hint("Give every row as many cells as the header."),
                    line,
                ));
            }
        }
    });
}

/// The source line of a table's data row `index` (0-based): header, delimiter, then one line per row.
pub fn row_line(table: Position, index: usize) -> usize {
    // IR input may carry any line, so this can't overflow (§10.7).
    table.line.saturating_add(2).saturating_add(index)
}

/// §4.7, §5: every media source must resolve, and its bytes must be PNG, JPEG or GIF.
fn images(input: &Input<'_>, out: &mut Sink) {
    for_each_block(&input.document.body, &mut |block| {
        let Block::Media(media) = block else { return };
        let line = media.position.map(|p| p.line);
        let bytes = match data_uri(&media.src) {
            Some(bytes) => Some(bytes),
            None => match (input.assets)(&media.src) {
                Asset::Bytes(bytes) => Some(bytes),
                Asset::Exists => None,
                Asset::NotFound => {
                    out.push(at(
                        Diagnostic::new(
                            "image_unresolved",
                            format!(
                                "Image `{}` can't be found; its alt text is shown instead.",
                                media.src
                            ),
                        )
                        .hint("Check the path, or make the image available to the host."),
                        line,
                    ));
                    return;
                }
            },
        };
        let Some(bytes) = bytes else { return };
        let unsupported = |why: &str| {
            at(
                Diagnostic::new(
                    "image_unsupported",
                    format!(
                        "Image `{}` {why}; its alt text is shown instead.",
                        media.src
                    ),
                )
                .hint("Convert the image to PNG or JPEG."),
                line,
            )
        };
        // §12.1: an image too large to decode safely is a limit, not a fallback.
        let limit = |why: String| {
            let diagnostic = Diagnostic::new("limit_exceeded", why);
            match media.position {
                Some(p) => diagnostic.at(p.line, Some(p.column)),
                None => diagnostic,
            }
        };
        if bytes.len() > crate::image::MAX_ENCODED {
            out.push(limit(format!(
                "Image `{}` is larger than 50 MiB.",
                media.src
            )));
        } else if image_format(&bytes).is_none() {
            out.push(unsupported("isn't PNG, JPEG or GIF"));
        } else {
            match crate::image::dimensions(&bytes) {
                None => out.push(unsupported("has a header that can't be read")),
                Some((w, h)) if u64::from(w) * u64::from(h) > crate::image::MAX_PIXELS => {
                    out.push(limit(format!(
                        "Image `{}` is {w} × {h} pixels, more than 100 megapixels.",
                        media.src
                    )));
                }
                Some(_) => {}
            }
        }
    });
}

/// The supported image formats, by content (§4.7).
pub fn image_format(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        Some("png")
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("jpeg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("gif")
    } else {
        None
    }
}

/// `data:` URIs with base64 content are decoded by the core itself (§4.7).
pub fn data_uri(src: &str) -> Option<Vec<u8>> {
    let rest = src.strip_prefix("data:")?;
    let (header, payload) = rest.split_once(',')?;
    if !header.ends_with(";base64") {
        return None;
    }
    base64(payload)
}

fn base64(text: &str) -> Option<Vec<u8>> {
    let value = |c: u8| match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' | b'-' => Some(62),
        b'/' | b'_' => Some(63),
        _ => None,
    };
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let (mut buffer, mut bits) = (0u32, 0);
    for c in text
        .bytes()
        .filter(|c| !c.is_ascii_whitespace() && *c != b'=')
    {
        buffer = (buffer << 6) | u32::from(value(c)?);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
        }
    }
    Some(out)
}

/// Slugs (§4.5): lowercase; keep letters, marks, numbers, `_`, `-` and spaces; spaces to `-`;
/// duplicates numbered in document order.
pub struct Slugger {
    used: HashSet<String>,
    /// The last suffix tried for each base, so repeated headings take linear time: every lower
    /// suffix is already used.
    suffixes: HashMap<String, usize>,
}

impl Slugger {
    pub fn new() -> Self {
        Self {
            used: HashSet::new(),
            suffixes: HashMap::new(),
        }
    }

    pub fn slug(&mut self, text: &str) -> String {
        let base: String = text
            .to_lowercase()
            .chars()
            .filter(|c| c.is_alphanumeric() || is_mark(*c) || matches!(c, '_' | '-' | ' '))
            .map(|c| if c == ' ' { '-' } else { c })
            .collect();
        let mut slug = base.clone();
        if self.used.contains(&slug) {
            let n = self.suffixes.entry(base.clone()).or_insert(0);
            loop {
                *n += 1;
                slug = format!("{base}-{n}");
                if !self.used.contains(&slug) {
                    break;
                }
            }
        }
        self.used.insert(slug.clone());
        slug
    }
}

impl Default for Slugger {
    fn default() -> Self {
        Self::new()
    }
}

/// Combining marks (Unicode category M), by the main combining blocks.
fn is_mark(c: char) -> bool {
    matches!(c as u32, 0x0300..=0x036F | 0x0483..=0x0489 | 0x0591..=0x05BD | 0x0610..=0x061A | 0x064B..=0x065F | 0x0900..=0x0903 | 0x093A..=0x094F | 0x1AB0..=0x1AFF | 0x1DC0..=0x1DFF | 0x20D0..=0x20FF | 0xFE20..=0xFE2F)
}

/// Fragment links that match no target produce `link_anchor_unresolved` (§4.5).
pub fn fragment_links(input: &Input<'_>, targets: &[String], what: &str, out: &mut Sink) {
    let targets: HashSet<&str> = targets.iter().map(String::as_str).collect();
    for (url, position) in &input.sidecar.links {
        let Some(fragment) = url.strip_prefix('#') else {
            continue;
        };
        let fragment = percent_decode(fragment);
        if !targets.contains(fragment.as_str()) {
            out.push(
                Diagnostic::new(
                    "link_anchor_unresolved",
                    format!("Link `{url}` matches no {what}; its text is kept without a link."),
                )
                .at(position.line, Some(position.column)),
            );
        }
    }
}

/// Decodes `%XX` escapes of two hex digits; anything else is kept as written.
pub(crate) fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let hex = |b: u8| (b as char).to_digit(16);
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(high) = bytes.get(i + 1).copied().and_then(hex)
            && let Some(low) = bytes.get(i + 2).copied().and_then(hex)
        {
            out.push((high * 16 + low) as u8);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Visits every block, depth-first in document order, including inside sections, lists, quotes and
/// directive bodies.
pub fn for_each_block(blocks: &[Block], visit: &mut dyn FnMut(&Block)) {
    for block in blocks {
        visit(block);
        match block {
            Block::Section(s) => for_each_block(&s.children, visit),
            Block::Quote(q) => for_each_block(&q.children, visit),
            Block::Directive(d) => for_each_block(&d.children, visit),
            Block::List(l) => {
                for item in &l.items {
                    for_each_block(&item.children, visit);
                }
            }
            _ => {}
        }
    }
}

/// `::: image` placement (§1.5, §6.1, §8.4): it applies to the images it wraps or, when empty, to the
/// immediately following media block; with no image to apply to it is `directive_misplaced`.
/// `check_options` validates the profile's own options (§1.6).
pub fn image_directives(
    blocks: &[Block],
    check_options: &dyn Fn(&Directive, &mut Sink),
    out: &mut Sink,
) {
    for (index, block) in blocks.iter().enumerate() {
        if let Block::Directive(d) = block
            && d.name == "image"
        {
            let applies = if d.children.is_empty() {
                matches!(blocks.get(index + 1), Some(Block::Media(_)))
            } else {
                let mut found = false;
                for_each_block(&d.children, &mut |b| found |= matches!(b, Block::Media(_)));
                found
            };
            // A misplaced directive has no effect, so its options aren't checked (§1.5).
            if applies {
                check_options(d, out);
            } else {
                out.push(at(
                    Diagnostic::new(
                        "directive_misplaced",
                        "`::: image` has no image to apply to; it is ignored.",
                    )
                    .hint("Wrap the image in the directive, or put the image directly after it."),
                    d.position.map(|p| p.line),
                ));
            }
        }
        match block {
            Block::Section(s) => image_directives(&s.children, check_options, out),
            Block::Quote(q) => image_directives(&q.children, check_options, out),
            Block::Directive(d) => image_directives(&d.children, check_options, out),
            Block::List(l) => {
                for item in &l.items {
                    image_directives(&item.children, check_options, out);
                }
            }
            _ => {}
        }
    }
}

/// A section title's plain text.
pub fn title(runs: &[crate::ir::Run]) -> String {
    plain_text(runs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_decoding() {
        assert_eq!(percent_decode("caf%C3%A9"), "café");
        assert_eq!(percent_decode("a%20b%2"), "a b%2");
        assert_eq!(percent_decode("%+1%-1"), "%+1%-1");
        // A `%` before a multibyte character is kept, never sliced through.
        assert_eq!(percent_decode("%aé"), "%aé");
        assert_eq!(percent_decode("%é"), "%é");
    }
}
