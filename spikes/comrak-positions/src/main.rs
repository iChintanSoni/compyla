//! Toolchain check 1 (docs/roadmap.md): comrak plus the directive pre-pass (markdown-spec.md §1.3)
//! reproduce the parse suite's positions exactly.
//!
//! For every fixture in spec/conformance/parse/ it compares, against expected.json:
//! - the position of every IR block node, in document order (sections, paragraphs, media, tables,
//!   lists, list items, code, quotes, breaks, directives)
//! - every table cell's `raw` source text (markdown-spec.md §4.6), re-split from the source
//! - every link destination, in order, including references resolved across directive bodies
//! - every parse diagnostic whose position this check is about (construct, char, fence,
//!   underline and node positions)
//!
//! Inline run text and frontmatter values are not positional and are out of scope here; the parse
//! suite pins them for Phase B.
//!
//! Design (the approach Phase B adopts): the pre-pass splits every container (the document, or a
//! directive body) into **segments** at directive boundaries. Each segment is parsed on its own and
//! its positions are offset back to absolute lines, so no markdown block can cross a directive (§1.3
//! rule 7) and total parsing work stays linear. Link destinations come from one parse of the whole
//! document, so the first definition in document order wins everywhere (§4.5).

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::{Arc, Mutex};

use comrak::nodes::{AstNode, NodeValue};
use comrak::options::BrokenLinkReference;
use comrak::{Arena, Options, ResolvedReference, parse_document};
use serde_json::Value;

/// Parse diagnostics this check computes; the rest (frontmatter values) aren't positional.
const CHECKED_CODES: &[&str] = &[
    "unsupported_content",
    "link_scheme_blocked",
    "directive_malformed",
    "directive_unclosed",
    "invalid_character",
    "ambiguous_break",
    "table_malformed",
    "document_in_code_fence",
];

const MARKERS: &[&str] = &["toc", "pagebreak", "col"];

const HTML_ELEMENTS: &[&str] = &[
    "a", "abbr", "address", "area", "article", "aside", "audio", "b", "base", "bdi", "bdo", "big", "blockquote", "body", "br",
    "button", "canvas", "caption", "center", "cite", "code", "col", "colgroup", "data", "datalist", "dd", "del", "details",
    "dfn", "dialog", "div", "dl", "dt", "em", "embed", "fieldset", "figcaption", "figure", "font", "footer", "form", "h1",
    "h2", "h3", "h4", "h5", "h6", "head", "header", "hgroup", "hr", "html", "i", "iframe", "img", "input", "ins", "kbd",
    "label", "legend", "li", "link", "main", "map", "mark", "menu", "meta", "meter", "nav", "noscript", "object", "ol",
    "optgroup", "option", "output", "p", "picture", "pre", "progress", "q", "rp", "rt", "ruby", "s", "samp", "script",
    "search", "section", "select", "slot", "small", "source", "span", "strike", "strong", "style", "sub", "summary", "sup",
    "table", "tbody", "td", "template", "textarea", "tfoot", "th", "thead", "time", "title", "tr", "track", "tt", "u", "ul",
    "var", "video", "wbr",
];

// ---------------------------------------------------------------------------------------------
// Output of the check: a flat, document-order skeleton.

#[derive(Debug, Clone, PartialEq, Eq)]
struct Pos {
    kind: &'static str,
    line: usize,
    column: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Diag {
    line: Option<usize>,
    column: Option<usize>,
    code: String,
}

#[derive(Default)]
struct Skeleton {
    positions: Vec<Pos>,
    raws: Vec<Vec<Vec<String>>>,
    links: Vec<String>,
    diagnostics: Vec<Diag>,
}

impl Skeleton {
    fn diag(&mut self, code: &str, line: Option<usize>, column: Option<usize>) {
        self.diagnostics.push(Diag { line, column, code: code.to_string() });
    }
}

// ---------------------------------------------------------------------------------------------
// Directive pre-pass (markdown-spec.md §1.3).

#[derive(Debug)]
struct Region {
    parent: Option<usize>,
    open_line: usize,
    close_line: Option<usize>,
    unclosed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    Normal,
    Opener(usize),
    Closer,
    Blank,
}

struct PrePass {
    lines: Vec<String>,
    kinds: Vec<Kind>,
    owner: Vec<Option<usize>>,
    regions: Vec<Region>,
}

fn is_opening_fence(line: &str) -> Option<(char, usize, usize)> {
    let indent = line.len() - line.trim_start_matches(' ').len();
    if indent > 3 {
        return None;
    }
    let rest = &line[indent..];
    let ch = rest.chars().next()?;
    if ch != '`' && ch != '~' {
        return None;
    }
    let len = rest.chars().take_while(|&c| c == ch).count();
    if len < 3 {
        return None;
    }
    if ch == '`' && rest[len..].contains('`') {
        return None;
    }
    Some((ch, len, indent))
}

fn is_closing_fence(line: &str, ch: char, len: usize) -> bool {
    let indent = line.len() - line.trim_start_matches(' ').len();
    if indent > 3 {
        return false;
    }
    let rest = &line[indent..];
    let run = rest.chars().take_while(|&c| c == ch).count();
    run >= len && rest[run..].trim().is_empty()
}

/// Parses an opener line: `colons [ws] name [ws args] [[ws] colons]`. Returns (name, one_line).
fn parse_opener(line: &str) -> Option<(String, bool)> {
    let colons = line.chars().take_while(|&c| c == ':').count();
    if colons < 3 {
        return None;
    }
    let rest = line[colons..].trim_start_matches([' ', '\t']);
    let name_len = rest
        .char_indices()
        .take_while(|&(i, c)| if i == 0 { c.is_ascii_alphabetic() } else { c.is_ascii_alphanumeric() || c == '_' || c == '-' })
        .count();
    if name_len == 0 {
        return None;
    }
    let name = rest[..name_len].to_ascii_lowercase();
    let after = &rest[name_len..];
    let trimmed = after.trim_end_matches([' ', '\t']);
    let trailing = trimmed.len() - trimmed.trim_end_matches(':').len();
    let one_line = trailing >= 3;
    let body = if one_line { &trimmed[..trimmed.len() - trailing] } else { trimmed };
    if !body.is_empty() && !body.starts_with([' ', '\t']) {
        return None; // e.g. `:::note{.x}`: args must be separated by whitespace
    }
    Some((name, one_line))
}

/// A blockquote line's content: leading `>` markers (each with one optional following space) removed.
fn quote_content(line: &str) -> &str {
    let mut rest = line.trim_start_matches(' ');
    while let Some(after) = rest.strip_prefix('>') {
        rest = after.strip_prefix(' ').unwrap_or(after).trim_start_matches(' ');
    }
    rest
}

fn is_closer(line: &str) -> bool {
    let colons = line.chars().take_while(|&c| c == ':').count();
    colons >= 3 && line[colons..].trim_matches([' ', '\t']).is_empty()
}

fn pre_pass(source: &str, skeleton: &mut Skeleton) -> PrePass {
    // §1.1: forbidden characters → U+FFFD, reported at their position.
    let normalized = source.replace("\r\n", "\n").replace('\r', "\n");
    let mut lines: Vec<String> = Vec::new();
    for (index, raw) in normalized.split('\n').enumerate() {
        let mut line = String::new();
        for (col, ch) in raw.chars().enumerate() {
            let forbidden = matches!(ch as u32, 0x0..=0x8 | 0xB | 0xC | 0xE..=0x1F | 0xFFFE | 0xFFFF);
            if forbidden {
                skeleton.diag("invalid_character", Some(index + 1), Some(col + 1));
                line.push('\u{FFFD}');
            } else {
                line.push(ch);
            }
        }
        lines.push(line);
    }

    let n = lines.len();
    let mut kinds = vec![Kind::Normal; n];
    let mut owner: Vec<Option<usize>> = vec![None; n];
    let mut regions: Vec<Region> = Vec::new();

    // Frontmatter (§2): excluded from directive recognition and from markdown parsing.
    let mut start = 0;
    if lines.first().is_some_and(|l| l.trim_end() == "---") {
        if let Some(end) = (1..n).find(|&j| lines[j].trim_end() == "---") {
            for kind in kinds.iter_mut().take(end + 1) {
                *kind = Kind::Blank;
            }
            start = end + 1;
        }
    }

    let mut stack: Vec<usize> = Vec::new();
    let mut fence: Option<(char, usize)> = None;
    let mut quoted_fence: Option<(char, usize)> = None; // a fenced code block inside a blockquote
    let mut marker_open: Option<usize> = None; // a marker's opener line, whose next-line closer it may take

    for i in start..n {
        let line = lines[i].clone();
        let top = stack.last().copied();
        owner[i] = top;
        let marker_line = marker_open.take();

        if let Some((ch, len)) = fence {
            if is_closing_fence(&line, ch, len) {
                fence = None;
            }
            continue;
        }
        if let Some((ch, len, _)) = is_opening_fence(&line) {
            fence = Some((ch, len));
            continue;
        }

        if line.starts_with(":::") {
            if is_closer(&line) {
                if marker_line.is_some_and(|m| m + 1 == i) {
                    kinds[i] = Kind::Closer; // belongs to the marker on the previous line
                } else if let Some(r) = stack.pop() {
                    kinds[i] = Kind::Closer;
                    owner[i] = Some(r);
                    regions[r].close_line = Some(i);
                } else {
                    skeleton.diag("directive_malformed", Some(i + 1), Some(1));
                    kinds[i] = Kind::Blank; // dropped; still ends the previous block
                }
            } else if let Some((name, one_line)) = parse_opener(&line) {
                let r = regions.len();
                regions.push(Region { parent: top, open_line: i, close_line: None, unclosed: false });
                kinds[i] = Kind::Opener(r);
                if MARKERS.contains(&name.as_str()) && !one_line {
                    marker_open = Some(i);
                } else if !one_line {
                    stack.push(r);
                }
            } else {
                skeleton.diag("directive_malformed", Some(i + 1), Some(1));
            }
            continue;
        }

        // Fence-like lines that aren't at column 1: indented 1–3 spaces, or after quote markers.
        // Lines inside fenced code, including fences inside a blockquote, never count (rule 9).
        let stripped = line.trim_start_matches(' ');
        let indent = line.len() - stripped.len();
        let quoted = indent <= 3 && stripped.starts_with('>');
        let unquoted = quote_content(&line);
        if quoted {
            if let Some((ch, len)) = quoted_fence {
                if is_closing_fence(unquoted, ch, len) {
                    quoted_fence = None;
                }
                continue;
            }
            if let Some((ch, len, _)) = is_opening_fence(unquoted) {
                quoted_fence = Some((ch, len));
                continue;
            }
        } else {
            quoted_fence = None;
        }
        let candidate = if (1..=3).contains(&indent) && !quoted { Some(stripped) } else if quoted { Some(unquoted.trim_start_matches(' ')) } else { None };
        if let Some(rest) = candidate {
            if rest.starts_with(":::") && (is_closer(rest) || parse_opener(rest).is_some()) {
                let column = line[..line.find(":::").unwrap()].chars().count() + 1;
                skeleton.diag("directive_malformed", Some(i + 1), Some(column));
            }
        }
    }

    // Unclosed openers become leaf directives; their would-be bodies return to the enclosing context.
    for &r in stack.iter().rev() {
        regions[r].unclosed = true;
        skeleton.diag("directive_unclosed", Some(regions[r].open_line + 1), None);
    }
    let resolve = |mut container: Option<usize>, regions: &[Region]| {
        while let Some(r) = container {
            if !regions[r].unclosed {
                break;
            }
            container = regions[r].parent;
        }
        container
    };
    for slot in owner.iter_mut() {
        *slot = resolve(*slot, &regions);
    }
    for r in 0..regions.len() {
        regions[r].parent = resolve(regions[r].parent, &regions);
    }

    PrePass { lines, kinds, owner, regions }
}

/// One item of a container, in order: a run of the container's own markdown lines, or a child
/// directive. Every directive boundary ends a run (§1.3 rule 7).
enum Item {
    Run { start: usize, end: usize },
    Directive(usize),
}

fn segments(pre: &PrePass, container: Option<usize>) -> Vec<Item> {
    let (from, to) = match container {
        None => (0, pre.lines.len()),
        Some(r) => (pre.regions[r].open_line + 1, pre.regions[r].close_line.unwrap_or(pre.lines.len())),
    };
    let mut items = Vec::new();
    let mut run: Option<usize> = None;
    for i in from..to {
        let own = pre.kinds[i] == Kind::Normal && pre.owner[i] == container;
        let child = matches!(pre.kinds[i], Kind::Opener(r) if pre.regions[r].parent == container);
        if own {
            run.get_or_insert(i);
            continue;
        }
        if let Some(start) = run.take() {
            items.push(Item::Run { start, end: i });
        }
        if let Kind::Opener(r) = pre.kinds[i] {
            if child {
                items.push(Item::Directive(r));
            }
        }
    }
    if let Some(start) = run {
        items.push(Item::Run { start, end: to });
    }
    items
}

// ---------------------------------------------------------------------------------------------
// comrak → skeleton.

fn options<'c>(callback: Option<Arc<dyn comrak::options::BrokenLinkCallback + 'c>>) -> Options<'c> {
    let mut options = Options::default();
    options.extension.table = true;
    options.extension.tasklist = true;
    options.extension.strikethrough = true;
    options.extension.autolink = true;
    options.extension.footnotes = true;
    options.parse.sourcepos_chars = true;
    options.parse.broken_link_callback = callback;
    options
}

/// Link destinations resolved document-wide (§4.5). One comrak parse of the whole document, with
/// directive fence lines blanked, resolves every reference with CommonMark's own rules, so the first
/// definition in document order wins. Destinations are keyed by the link's absolute position, plus
/// the label map that segment parses need to recognize references defined in other segments.
struct Links {
    by_position: HashMap<(usize, usize), String>,
    by_label: HashMap<String, String>,
}

fn global_links(pre: &PrePass) -> Links {
    let global: Vec<String> =
        (0..pre.lines.len()).map(|i| if pre.kinds[i] == Kind::Normal { pre.lines[i].clone() } else { String::new() }).collect();
    let labels: Arc<Mutex<HashSet<String>>> = Arc::default();
    let sink = Arc::clone(&labels);
    let record = Arc::new(move |r: BrokenLinkReference| {
        sink.lock().unwrap().insert(r.normalized.to_string());
        None
    });

    // Labels a segment parse can't resolve on its own; the probe below resolves them globally.
    for container in std::iter::once(None).chain((0..pre.regions.len()).map(Some)) {
        for item in segments(pre, container) {
            if let Item::Run { start, end } = item {
                let arena = Arena::new();
                let _ = parse_document(&arena, &pre.lines[start..end].join("\n"), &options(Some(record.clone())));
            }
        }
    }
    let labels: Vec<String> = labels.lock().unwrap().iter().cloned().collect();

    let mut text = global.clone();
    let probe_start = text.len() + 2;
    text.push(String::new());
    for label in &labels {
        text.push(format!("[probe][{label}]"));
        text.push(String::new());
    }
    let arena = Arena::new();
    let root = parse_document(&arena, &text.join("\n"), &options(None));
    let mut links = Links { by_position: HashMap::new(), by_label: HashMap::new() };
    for node in root.descendants() {
        let data = node.data.borrow();
        if let NodeValue::Link(link) | NodeValue::Image(link) = &data.value {
            let pos = data.sourcepos.start;
            if pos.line >= probe_start {
                links.by_label.insert(labels[(pos.line - probe_start) / 2].clone(), link.url.clone());
            } else {
                links.by_position.insert((pos.line, pos.column), link.url.clone());
            }
        }
    }
    links
}

struct Walker<'p> {
    pre: &'p PrePass,
    links: Links,
    /// Line offset of the segment being walked: comrak line + offset = absolute line.
    offset: usize,
    skeleton: Skeleton,
}

impl Walker<'_> {
    fn container(&mut self, container: Option<usize>) {
        let items = segments(self.pre, container);
        if container.is_none() {
            self.code_fence_document(&items);
        }
        for item in items {
            match item {
                Item::Run { start, end } => {
                    let by_label = self.links.by_label.clone();
                    let callback = Arc::new(move |r: BrokenLinkReference| {
                        by_label.get(r.normalized).map(|url| ResolvedReference { url: url.clone(), title: String::new() })
                    });
                    let arena = Arena::new();
                    let root = parse_document(&arena, &self.pre.lines[start..end].join("\n"), &options(Some(callback)));
                    self.offset = start;
                    for child in root.children() {
                        self.block(child);
                    }
                }
                Item::Directive(r) => {
                    self.skeleton.positions.push(Pos { kind: "directive", line: self.pre.regions[r].open_line + 1, column: 1 });
                    if !self.pre.regions[r].unclosed {
                        self.container(Some(r));
                    }
                }
            }
        }
    }

    /// §4.9 `document_in_code_fence`: the whole body is one fenced markdown code block.
    fn code_fence_document(&mut self, items: &[Item]) {
        let [Item::Run { start, end }] = items else { return };
        let arena = Arena::new();
        let root = parse_document(&arena, &self.pre.lines[*start..*end].join("\n"), &options(None));
        let blocks: Vec<_> = root.children().collect();
        if let [only] = blocks.as_slice() {
            if let NodeValue::CodeBlock(code) = &only.data.borrow().value {
                let lang = code.info.split_ascii_whitespace().next().unwrap_or("");
                if code.fenced && (lang == "markdown" || lang == "md") {
                    let line = only.data.borrow().sourcepos.start.line + start;
                    self.skeleton.diag("document_in_code_fence", Some(line), None);
                }
            }
        }
    }

    fn at(&self, node: &AstNode<'_>) -> (usize, usize) {
        let pos = node.data.borrow().sourcepos.start;
        (pos.line + self.offset, pos.column)
    }

    fn push(&mut self, kind: &'static str, node: &AstNode<'_>) {
        let (line, column) = self.at(node);
        self.skeleton.positions.push(Pos { kind, line, column });
    }

    /// The destination of a link node: the document-wide one at its position, else comrak's.
    fn destination(&self, node: &AstNode<'_>, local: &str) -> String {
        self.links.by_position.get(&self.at(node)).cloned().unwrap_or_else(|| local.to_string())
    }

    fn block<'a>(&mut self, node: &'a AstNode<'a>) {
        let value = node.data.borrow().value.clone();
        match value {
            NodeValue::Heading(heading) => {
                self.push("section", node);
                let end = node.data.borrow().sourcepos.end.line + self.offset;
                if heading.setext && self.pre.lines[end - 1].trim_start().starts_with('-') {
                    self.skeleton.diag("ambiguous_break", Some(end), None);
                }
                self.inlines(node);
            }
            NodeValue::Paragraph => self.paragraph(node),
            NodeValue::Table(_) => {
                self.push("table", node);
                self.table_raws(node);
                for row in node.children() {
                    for cell in row.children() {
                        self.inlines(cell);
                    }
                }
            }
            NodeValue::List(_) => {
                self.push("list", node);
                for item in node.children() {
                    self.push("item", item);
                    for child in item.children() {
                        self.block(child);
                    }
                }
            }
            NodeValue::CodeBlock(_) => self.push("code", node),
            NodeValue::BlockQuote => {
                self.push("quote", node);
                for child in node.children() {
                    self.block(child);
                }
            }
            NodeValue::ThematicBreak => self.push("break", node),
            NodeValue::HtmlBlock(html) => {
                let literal = html.literal.trim();
                if is_marker(literal) {
                    self.push("directive", node);
                } else if !(literal.starts_with("<!--") && literal.ends_with("-->") && literal.matches("-->").count() == 1) {
                    let (line, column) = self.at(node);
                    self.skeleton.diag("unsupported_content", Some(line), Some(column));
                }
            }
            NodeValue::FootnoteDefinition(_) => {
                let (line, column) = self.at(node);
                self.skeleton.diag("unsupported_content", Some(line), Some(column));
            }
            _ => {}
        }
    }

    fn paragraph<'a>(&mut self, node: &'a AstNode<'a>) {
        // §4.3/§4.7: a paragraph of only images (each optionally linked) yields one media per image.
        let mut images: Vec<(&'a AstNode<'a>, Option<String>)> = Vec::new();
        let mut only_images = true;
        for child in node.children() {
            match &child.data.borrow().value {
                NodeValue::Image(_) => images.push((child, None)),
                NodeValue::Link(link) => {
                    let kids: Vec<_> = child.children().collect();
                    if kids.len() == 1 && matches!(kids[0].data.borrow().value, NodeValue::Image(_)) {
                        images.push((child, Some(link.url.clone())));
                    } else {
                        only_images = false;
                    }
                }
                NodeValue::SoftBreak | NodeValue::LineBreak => {}
                NodeValue::Text(text) if text.trim().is_empty() => {}
                _ => only_images = false,
            }
        }
        if only_images && !images.is_empty() {
            for (image, link) in images {
                self.push("media", image);
                if let Some(local) = link {
                    // A linked image keeps its link only when the destination is allowed (§4.7).
                    let url = self.destination(image, &local);
                    if destination_allowed(&url) {
                        self.skeleton.links.push(url);
                    } else {
                        let (line, column) = self.at(image);
                        self.skeleton.diag("link_scheme_blocked", Some(line), Some(column));
                    }
                }
            }
            return;
        }

        self.push("paragraph", node);
        let pos = node.data.borrow().sourcepos;
        let (first_line, last_line) = (pos.start.line + self.offset, pos.end.line + self.offset);
        if last_line > first_line {
            let first = &self.pre.lines[first_line - 1];
            let second = &self.pre.lines[first_line];
            let is_delimiter = second.contains('-') && second.trim().chars().all(|c| matches!(c, '|' | ':' | '-' | ' '));
            if first.contains('|') && is_delimiter && split_cells(first).len() != split_cells(second).len() {
                self.skeleton.diag("table_malformed", Some(first_line), None);
            }
        }
        self.inlines(node);
    }

    fn inlines<'a>(&mut self, node: &'a AstNode<'a>) {
        for child in node.children() {
            let value = child.data.borrow().value.clone();
            let (line, column) = self.at(child);
            match value {
                NodeValue::Image(_) | NodeValue::FootnoteReference(_) => {
                    self.skeleton.diag("unsupported_content", Some(line), Some(column));
                    continue;
                }
                NodeValue::HtmlInline(html) => {
                    if let Some(name) = tag_name(&html) {
                        let known = HTML_ELEMENTS.contains(&name.to_ascii_lowercase().as_str());
                        if known && !name.eq_ignore_ascii_case("br") {
                            self.skeleton.diag("unsupported_content", Some(line), Some(column));
                        }
                    }
                }
                NodeValue::Link(link) => {
                    let url = self.destination(child, &link.url);
                    if destination_allowed(&url) {
                        self.skeleton.links.push(url);
                    } else {
                        self.skeleton.diag("link_scheme_blocked", Some(line), Some(column));
                    }
                }
                _ => {}
            }
            self.inlines(child);
        }
    }

    fn table_raws<'a>(&mut self, table: &'a AstNode<'a>) {
        let start = table.data.borrow().sourcepos.start;
        let first = start.line + self.offset;
        let rows = table.children().count(); // header + data rows
        let mut raws = Vec::new();
        for line_index in 0..=rows {
            if line_index == 1 {
                continue; // the delimiter row
            }
            let line = &self.pre.lines[first - 1 + line_index];
            let row: String = line.chars().skip(start.column - 1).collect();
            raws.push(split_cells(&row));
        }
        self.skeleton.raws.push(raws);
    }
}

fn is_marker(literal: &str) -> bool {
    literal
        .strip_prefix("<!--")
        .and_then(|s| s.strip_suffix("-->"))
        .map(str::trim)
        .is_some_and(|name| {
            let mut chars = name.chars();
            chars.next().is_some_and(|c| c.is_ascii_alphabetic()) && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        })
}

fn tag_name(html: &str) -> Option<&str> {
    let rest = html.strip_prefix("</").or_else(|| html.strip_prefix('<'))?;
    let len = rest.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '-').count();
    (len > 0 && rest.chars().next()?.is_ascii_alphabetic()).then(|| &rest[..len])
}

/// markdown-spec.md §4.5 link allowlist, on the destination as a browser reads it, refusing other
/// controls and a leading non-ASCII space or formatting character; percent-decoded, the part
/// before any query or fragment must pass too (C59).
fn destination_allowed(url: &str) -> bool {
    let read = |url: &str| -> String {
        url.trim_matches(|c: char| c <= ' ').chars().filter(|c| !matches!(c, '\t' | '\n' | '\r')).collect()
    };
    let formatting = |c: char| {
        matches!(c, '\u{061C}' | '\u{200B}'..='\u{200F}' | '\u{2028}'..='\u{202E}' | '\u{2060}'..='\u{2064}' | '\u{2066}'..='\u{2069}' | '\u{FEFF}')
    };
    let url = read(url);
    if url.chars().any(char::is_control) || url.starts_with(|c: char| c.is_whitespace() || formatting(c)) {
        return false;
    }
    let path = url.split(['?', '#']).next().unwrap_or_default();
    let decoded: String = read(&percent_decoded(path)).chars().filter(|c| !c.is_control()).collect();
    allowed(&url) && allowed(&decoded)
}

fn percent_decoded(text: &str) -> String {
    let bytes = text.as_bytes();
    let hex = |b: u8| (b as char).to_digit(16);
    let (mut out, mut i) = (Vec::with_capacity(bytes.len()), 0);
    while i < bytes.len() {
        match (bytes[i], bytes.get(i + 1).copied().and_then(hex), bytes.get(i + 2).copied().and_then(hex)) {
            (b'%', Some(high), Some(low)) => {
                out.push((high * 16 + low) as u8);
                i += 3;
            }
            (byte, _, _) => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn allowed(url: &str) -> bool {
    let scheme_len = url.chars().take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-')).count();
    let has_scheme = scheme_len > 0 && url.starts_with(|c: char| c.is_ascii_alphabetic()) && url[scheme_len..].starts_with(':');
    if has_scheme {
        let scheme = url[..scheme_len].to_ascii_lowercase();
        return matches!(scheme.as_str(), "http" | "https" | "mailto" | "tel");
    }
    url.starts_with('#') || !(url.starts_with('/') || url.starts_with('\\') || url.contains('\\'))
}

/// GFM 0.29 cell splitting for `raw` (markdown-spec.md §4.6): split on unescaped pipes, outer pipes
/// optional, cells trimmed of spaces and tabs, `\|` unescaped.
fn split_cells(row: &str) -> Vec<String> {
    let trimmed = row.trim_matches([' ', '\t']);
    let mut cells = Vec::new();
    let mut current = String::new();
    let mut chars = trimmed.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek() == Some(&'|') => {
                current.push('|');
                chars.next();
            }
            '|' => cells.push(std::mem::take(&mut current)),
            _ => current.push(c),
        }
    }
    cells.push(current);
    if trimmed.starts_with('|') {
        cells.remove(0);
    }
    if trimmed.ends_with('|') && !trimmed.ends_with("\\|") && trimmed.len() > 1 {
        cells.pop();
    }
    cells.into_iter().map(|c| c.trim_matches([' ', '\t']).to_string()).collect()
}

fn analyze(source: &str) -> Skeleton {
    let mut skeleton = Skeleton::default();
    let pre = pre_pass(source, &mut skeleton);
    let links = global_links(&pre);
    let mut walker = Walker { pre: &pre, links, offset: 0, skeleton };
    walker.container(None);
    walker.skeleton
}

// ---------------------------------------------------------------------------------------------
// Expected values from the fixture's IR.

fn expected_skeleton(expected: &Value) -> Skeleton {
    fn walk(node: &Value, out: &mut Skeleton) {
        let kind: &'static str = match node["type"].as_str() {
            Some("section") => "section",
            Some("paragraph") => "paragraph",
            Some("media") => "media",
            Some("table") => "table",
            Some("list") => "list",
            Some("code") => "code",
            Some("quote") => "quote",
            Some("break") => "break",
            Some("directive") => "directive",
            None => "item",
            Some(other) => panic!("unknown node type {other}"),
        };
        let pos = &node["position"];
        out.positions.push(Pos { kind, line: pos["line"].as_u64().unwrap() as usize, column: pos["column"].as_u64().unwrap() as usize });
        if kind == "table" {
            let row = |cells: &Value| cells.as_array().unwrap().iter().map(|c| c["raw"].as_str().unwrap().to_string()).collect::<Vec<_>>();
            let mut raws = vec![row(&node["header"])];
            raws.extend(node["rows"].as_array().unwrap().iter().map(row));
            out.raws.push(raws);
        }
        if kind == "media" {
            if let Some(link) = node["link"].as_str() {
                out.links.push(link.to_string());
            }
        }
        for runs in [&node["runs"], &node["title"]] {
            collect_links(runs, out);
        }
        if kind == "table" {
            for cell in node["header"].as_array().unwrap() {
                collect_links(&cell["runs"], out);
            }
            for row in node["rows"].as_array().unwrap() {
                for cell in row.as_array().unwrap() {
                    collect_links(&cell["runs"], out);
                }
            }
        }
        for key in ["children", "items"] {
            if let Some(children) = node[key].as_array() {
                for child in children {
                    walk(child, out);
                }
            }
        }
    }
    fn collect_links(runs: &Value, out: &mut Skeleton) {
        let mut previous: Option<&str> = None;
        for run in runs.as_array().into_iter().flatten() {
            let link = run["link"].as_str();
            if let Some(link) = link {
                if previous != Some(link) {
                    out.links.push(link.to_string());
                }
            }
            previous = link;
        }
    }

    let mut out = Skeleton::default();
    for node in expected["ir"]["body"].as_array().unwrap() {
        walk(node, &mut out);
    }
    for d in expected["diagnostics"].as_array().unwrap() {
        let code = d["code"].as_str().unwrap();
        if CHECKED_CODES.contains(&code) {
            out.diag(code, d["line"].as_u64().map(|v| v as usize), d["column"].as_u64().map(|v| v as usize));
        }
    }
    out
}

fn main() -> ExitCode {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../spec/conformance/parse");
    let mut fixtures: Vec<PathBuf> =
        fs::read_dir(&root).expect("parse suite").map(|e| e.unwrap().path()).filter(|p| p.join("case.json").is_file()).collect();
    fixtures.sort();

    let (mut failures, mut nodes, mut cells, mut links, mut diagnostics) = (0, 0, 0, 0, 0);
    for fixture in &fixtures {
        let name = fixture.file_name().unwrap().to_string_lossy().to_string();
        let source = fs::read_to_string(fixture.join("input.md")).unwrap();
        let expected: Value = serde_json::from_str(&fs::read_to_string(fixture.join("expected.json")).unwrap()).unwrap();

        let want = expected_skeleton(&expected);
        let mut got = analyze(&source);
        // Columns are compared only where the fixture states one (markdown-spec.md §11.3).
        let mut want_diags = want.diagnostics.clone();
        for (g, w) in got.diagnostics.iter_mut().zip(want_diags.iter_mut()) {
            if w.column.is_none() {
                g.column = None;
            }
        }
        got.diagnostics.sort();
        want_diags.sort();
        for g in got.diagnostics.iter_mut() {
            if want_diags.iter().any(|w| w.code == g.code && w.line == g.line && w.column.is_none()) {
                g.column = None;
            }
        }
        got.diagnostics.sort();

        let mut problems = Vec::new();
        if got.positions != want.positions {
            problems.push(format!("positions\n    got  {:?}\n    want {:?}", got.positions, want.positions));
        }
        if got.raws != want.raws {
            problems.push(format!("table raws\n    got  {:?}\n    want {:?}", got.raws, want.raws));
        }
        if got.links != want.links {
            problems.push(format!("links\n    got  {:?}\n    want {:?}", got.links, want.links));
        }
        if got.diagnostics != want_diags {
            problems.push(format!("diagnostics\n    got  {:?}\n    want {:?}", got.diagnostics, want_diags));
        }
        if problems.is_empty() {
            nodes += want.positions.len();
            cells += want.raws.iter().flatten().map(Vec::len).sum::<usize>();
            links += want.links.len();
            diagnostics += want_diags.len();
            println!("ok    {name}");
        } else {
            failures += 1;
            println!("FAIL  {name}");
            for problem in problems {
                println!("  {problem}");
            }
        }
    }

    println!(
        "\n{} fixtures, {failures} failing; matched {nodes} node positions, {cells} table cells, {links} link destinations, {diagnostics} diagnostics",
        fixtures.len()
    );
    if failures == 0 { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}
