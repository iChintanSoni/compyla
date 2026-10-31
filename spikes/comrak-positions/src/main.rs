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
                if marker_line == Some(i - 1) {
                    kinds[i] = Kind::Closer; // belongs to the marker on the previous line
                } else if let Some(r) = stack.pop() {
                    kinds[i] = Kind::Closer;
                    owner[i] = Some(r);
                } else {
                    skeleton.diag("directive_malformed", Some(i + 1), Some(1));
                    kinds[i] = Kind::Blank; // dropped; still ends the previous block
                }
            } else if let Some((name, one_line)) = parse_opener(&line) {
                let r = regions.len();
                regions.push(Region { parent: top, open_line: i, unclosed: false });
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
        let stripped = line.trim_start_matches(' ');
        let indent = line.len() - stripped.len();
        let unquoted = line.trim_start_matches(|c: char| c == '>' || c == ' ');
        let quoted = line.trim_start().starts_with('>');
        let candidate = if (1..=3).contains(&indent) && !quoted { Some(stripped) } else if quoted { Some(unquoted) } else { None };
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

const PLACEHOLDER: &str = "compyla-directive:";

/// The text of one container (the document, or a directive body): its own lines kept, each child
/// directive's opener replaced by a one-line HTML comment placeholder (an HTML block that interrupts
/// a paragraph and ends on its line, §1.3 rule 7), every other line blank. Line and column
/// positions are therefore identical to the source.
fn container_text(pre: &PrePass, container: Option<usize>) -> String {
    let mut out = Vec::with_capacity(pre.lines.len());
    for i in 0..pre.lines.len() {
        let line = match pre.kinds[i] {
            Kind::Opener(r) if pre.regions[r].parent == container => format!("<!--{PLACEHOLDER}{r}-->"),
            Kind::Normal if pre.owner[i] == container => pre.lines[i].clone(),
            _ => String::new(),
        };
        out.push(line);
    }
    out.join("\n")
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

struct Walker<'p> {
    pre: &'p PrePass,
    references: HashMap<String, String>,
    skeleton: Skeleton,
}

impl Walker<'_> {
    fn container(&mut self, container: Option<usize>) {
        let text = container_text(self.pre, container);
        let references = self.references.clone();
        let callback = Arc::new(move |r: BrokenLinkReference| {
            references.get(r.normalized).map(|url| ResolvedReference { url: url.clone(), title: String::new() })
        });
        let arena = Arena::new();
        let root = parse_document(&arena, &text, &options(Some(callback)));

        if container.is_none() {
            let blocks: Vec<_> = root.children().filter(|n| !matches!(n.data.borrow().value, NodeValue::FrontMatter(_))).collect();
            if let [only] = blocks.as_slice() {
                if let NodeValue::CodeBlock(code) = &only.data.borrow().value {
                    let lang = code.info.split_ascii_whitespace().next().unwrap_or("");
                    if code.fenced && (lang == "markdown" || lang == "md") {
                        let pos = only.data.borrow().sourcepos.start;
                        self.skeleton.diag("document_in_code_fence", Some(pos.line), None);
                    }
                }
            }
        }
        for child in root.children() {
            self.block(child);
        }
    }

    fn push(&mut self, kind: &'static str, node: &AstNode<'_>) {
        let pos = node.data.borrow().sourcepos.start;
        self.skeleton.positions.push(Pos { kind, line: pos.line, column: pos.column });
    }

    fn block<'a>(&mut self, node: &'a AstNode<'a>) {
        let value = node.data.borrow().value.clone();
        match value {
            NodeValue::Heading(heading) => {
                self.push("section", node);
                let end = node.data.borrow().sourcepos.end.line;
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
                if let Some(r) = literal.strip_prefix("<!--").and_then(|s| s.strip_prefix(PLACEHOLDER)).and_then(|s| s.strip_suffix("-->")) {
                    let r: usize = r.parse().unwrap();
                    let line = self.pre.regions[r].open_line + 1;
                    self.skeleton.positions.push(Pos { kind: "directive", line, column: 1 });
                    if !self.pre.regions[r].unclosed {
                        self.container(Some(r));
                    }
                } else if is_marker(literal) {
                    self.push("directive", node);
                } else if !(literal.starts_with("<!--") && literal.ends_with("-->") && literal.matches("-->").count() == 1) {
                    let pos = node.data.borrow().sourcepos.start;
                    self.skeleton.diag("unsupported_content", Some(pos.line), Some(pos.column));
                }
            }
            NodeValue::FootnoteDefinition(_) => {
                let pos = node.data.borrow().sourcepos.start;
                self.skeleton.diag("unsupported_content", Some(pos.line), Some(pos.column));
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
                if let Some(link) = link {
                    self.skeleton.links.push(link);
                }
            }
            return;
        }

        self.push("paragraph", node);
        let pos = node.data.borrow().sourcepos;
        if pos.end.line > pos.start.line {
            let first = &self.pre.lines[pos.start.line - 1];
            let second = &self.pre.lines[pos.start.line];
            let is_delimiter = second.contains('-') && second.trim().chars().all(|c| matches!(c, '|' | ':' | '-' | ' '));
            if first.contains('|') && is_delimiter && split_cells(first).len() != split_cells(second).len() {
                self.skeleton.diag("table_malformed", Some(pos.start.line), None);
            }
        }
        self.inlines(node);
    }

    fn inlines<'a>(&mut self, node: &'a AstNode<'a>) {
        for child in node.children() {
            let value = child.data.borrow().value.clone();
            let pos = child.data.borrow().sourcepos.start;
            match value {
                NodeValue::Image(_) | NodeValue::FootnoteReference(_) => {
                    self.skeleton.diag("unsupported_content", Some(pos.line), Some(pos.column));
                    continue;
                }
                NodeValue::HtmlInline(html) => {
                    if let Some(name) = tag_name(&html) {
                        let known = HTML_ELEMENTS.contains(&name.to_ascii_lowercase().as_str());
                        if known && !name.eq_ignore_ascii_case("br") {
                            self.skeleton.diag("unsupported_content", Some(pos.line), Some(pos.column));
                        }
                    }
                }
                NodeValue::Link(link) => {
                    if destination_allowed(&link.url) {
                        self.skeleton.links.push(link.url.clone());
                    } else {
                        self.skeleton.diag("link_scheme_blocked", Some(pos.line), Some(pos.column));
                    }
                }
                _ => {}
            }
            self.inlines(child);
        }
    }

    fn table_raws<'a>(&mut self, table: &'a AstNode<'a>) {
        let start = table.data.borrow().sourcepos.start;
        let rows = table.children().count(); // header + data rows
        let mut raws = Vec::new();
        for (k, line_index) in (0..=rows).enumerate() {
            if k == 1 {
                continue; // the delimiter row
            }
            let line = &self.pre.lines[start.line - 1 + line_index];
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

/// markdown-spec.md §4.5 link allowlist.
fn destination_allowed(url: &str) -> bool {
    let scheme_len = url.chars().take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-')).count();
    let has_scheme = scheme_len > 0 && url.chars().next().unwrap().is_ascii_alphabetic() && url[scheme_len..].starts_with(':');
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

/// Document-wide link reference resolution (markdown-spec.md §4.5): references that a container
/// can't resolve on its own are resolved against the whole document with directive fences
/// removed, using comrak's own definition parsing (first definition in document order wins).
fn collect_references(pre: &PrePass) -> HashMap<String, String> {
    let broken: Arc<Mutex<HashSet<String>>> = Arc::default();
    let mut containers = vec![None];
    containers.extend((0..pre.regions.len()).filter(|&r| !pre.regions[r].unclosed).map(Some));
    for container in containers {
        let sink = Arc::clone(&broken);
        let callback = Arc::new(move |r: BrokenLinkReference| {
            sink.lock().unwrap().insert(r.normalized.to_string());
            None
        });
        let arena = Arena::new();
        let _ = parse_document(&arena, &container_text(pre, container), &options(Some(callback)));
    }

    let labels: Vec<String> = broken.lock().unwrap().iter().cloned().collect();
    if labels.is_empty() {
        return HashMap::new();
    }
    let mut global: Vec<String> = (0..pre.lines.len())
        .map(|i| if pre.kinds[i] == Kind::Normal { pre.lines[i].clone() } else { String::new() })
        .collect();
    let probe_start = global.len() + 2;
    global.push(String::new());
    for label in &labels {
        global.push(format!("[probe][{label}]"));
        global.push(String::new());
    }
    let arena = Arena::new();
    let root = parse_document(&arena, &global.join("\n"), &options(None));
    let mut map = HashMap::new();
    for node in root.descendants() {
        let data = node.data.borrow();
        if let NodeValue::Link(link) = &data.value {
            if data.sourcepos.start.line >= probe_start {
                let index = (data.sourcepos.start.line - probe_start) / 2;
                map.insert(labels[index].clone(), link.url.clone());
            }
        }
    }
    map
}

fn analyze(source: &str) -> Skeleton {
    let mut skeleton = Skeleton::default();
    let pre = pre_pass(source, &mut skeleton);
    let references = collect_references(&pre);
    let mut walker = Walker { pre: &pre, references, skeleton };
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
    let mut fixtures: Vec<PathBuf> = fs::read_dir(&root).expect("parse suite").map(|e| e.unwrap().path()).collect();
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
