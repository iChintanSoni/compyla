//! Markdown → IR (markdown-spec.md §4), with the parse-stage diagnostics of §1–§4.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use comrak::nodes::{AstNode, ListType, NodeValue, TableAlignment};
use comrak::options::BrokenLinkReference;
use comrak::{Arena, Options, ResolvedReference, parse_document};
use serde_json::Map;

use crate::diagnostic::{Blocking, Diagnostic, Sink};
use crate::frontmatter;
use crate::ir::{self, Align, Block, Cell, Document, Position, Run, canonical_runs, plain_text};
use crate::prepass::{self, Item, Kind, PrePass};

/// markdown-spec.md §12.1 defaults.
// The limits of markdown-spec.md §12.1, shared with IR input.
pub(crate) const MAX_INPUT_BYTES: usize = 10 * 1024 * 1024;
pub(crate) const MAX_DEPTH: usize = 32;
pub(crate) const MAX_TABLE_CELLS: usize = 1_000_000;
pub(crate) const MAX_ROW_CELLS: usize = 16_384;

/// `limit_exceeded` for input larger than [`MAX_INPUT_BYTES`], which isn't processed at all.
pub fn input_too_large(len: usize) -> Option<Diagnostic> {
    (len > MAX_INPUT_BYTES).then(|| {
        Diagnostic::new(
            "limit_exceeded",
            "The input is larger than 10 MiB; it was not processed.",
        )
    })
}

const HTML_ELEMENTS: &[&str] = &[
    "a",
    "abbr",
    "address",
    "area",
    "article",
    "aside",
    "audio",
    "b",
    "base",
    "bdi",
    "bdo",
    "big",
    "blockquote",
    "body",
    "br",
    "button",
    "canvas",
    "caption",
    "center",
    "cite",
    "code",
    "col",
    "colgroup",
    "data",
    "datalist",
    "dd",
    "del",
    "details",
    "dfn",
    "dialog",
    "div",
    "dl",
    "dt",
    "em",
    "embed",
    "fieldset",
    "figcaption",
    "figure",
    "font",
    "footer",
    "form",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "head",
    "header",
    "hgroup",
    "hr",
    "html",
    "i",
    "iframe",
    "img",
    "input",
    "ins",
    "kbd",
    "label",
    "legend",
    "li",
    "link",
    "main",
    "map",
    "mark",
    "menu",
    "meta",
    "meter",
    "nav",
    "noscript",
    "object",
    "ol",
    "optgroup",
    "option",
    "output",
    "p",
    "picture",
    "pre",
    "progress",
    "q",
    "rp",
    "rt",
    "ruby",
    "s",
    "samp",
    "script",
    "search",
    "section",
    "select",
    "slot",
    "small",
    "source",
    "span",
    "strike",
    "strong",
    "style",
    "sub",
    "summary",
    "sup",
    "table",
    "tbody",
    "td",
    "template",
    "textarea",
    "tfoot",
    "th",
    "thead",
    "time",
    "title",
    "tr",
    "track",
    "tt",
    "u",
    "ul",
    "var",
    "video",
    "wbr",
];

pub struct ParseOutput {
    pub document: Document,
    pub diagnostics: Vec<Diagnostic>,
    /// Whether any parse diagnostic, kept or not, blocks `convert` (§11.1).
    pub blocking: Blocking,
    pub sidecar: Sidecar,
}

/// Source positions validation needs that the IR doesn't carry: frontmatter key lines (§2) and
/// inline link positions (for `link_anchor_unresolved`, §4.5). IR input has none (§10.7).
#[derive(Debug, Clone, Default)]
pub struct Sidecar {
    pub meta_lines: HashMap<String, usize>,
    /// Every allowed link destination, in document order, at its construct's position.
    pub links: Vec<(String, Position)>,
}

/// The `parse` operation's core (markdown-spec.md §10.1): markdown to IR and parse diagnostics.
pub fn parse(source: &str) -> ParseOutput {
    if let Some(diagnostic) = input_too_large(source.len()) {
        return ParseOutput {
            document: empty(),
            diagnostics: vec![diagnostic],
            blocking: Blocking {
                error: true,
                promotable: false,
            },
            sidecar: Sidecar::default(),
        };
    }
    let mut diagnostics = Sink::new();
    let pre = prepass::run(source, &mut diagnostics);
    let (meta, meta_lines) = pre
        .frontmatter
        .map(|range| frontmatter::parse(&pre.lines, range, &mut diagnostics))
        .unwrap_or_default();
    let links = global_links(&pre);
    let mut builder = Builder {
        pre: &pre,
        links,
        offset: 0,
        depth: 0,
        cells: 0,
        diagnostics,
        link_sites: Vec::new(),
        char_starts: Default::default(),
    };
    let body = builder.container(None);
    let Builder {
        mut diagnostics,
        link_sites,
        ..
    } = builder;
    document_in_code_fence(&body, &mut diagnostics);
    let (diagnostics, blocking) = diagnostics.into_parts();
    let sidecar = Sidecar {
        meta_lines,
        links: link_sites,
    };
    ParseOutput {
        document: Document {
            ir_version: ir::IR_VERSION.to_string(),
            meta,
            body,
        },
        diagnostics,
        blocking,
        sidecar,
    }
}

fn empty() -> Document {
    Document {
        ir_version: ir::IR_VERSION.to_string(),
        meta: Map::new(),
        body: Vec::new(),
    }
}

fn options<'c>(callback: Option<Arc<dyn comrak::options::BrokenLinkCallback + 'c>>) -> Options<'c> {
    let mut options = Options::default();
    options.extension.table = true;
    options.extension.tasklist = true;
    options.extension.strikethrough = true;
    options.extension.autolink = true;
    options.extension.footnotes = true;
    // Byte columns: comrak's character columns rescan the line for every node, which is quadratic
    // on a long line. `Builder::position` converts them (§0.1).
    options.parse.sourcepos_chars = false;
    options.parse.broken_link_callback = callback;
    options
}

/// Link destinations resolved document-wide (§4.5): one comrak parse of the whole document, fences
/// blanked, resolves every reference with CommonMark's rules, so the first definition in document
/// order wins. Destinations are keyed by absolute position; `by_label` lets segment parses
/// recognize references defined in another segment.
struct Links {
    by_position: HashMap<(usize, usize), String>,
    by_label: HashMap<String, String>,
}

fn global_links(pre: &PrePass) -> Links {
    let labels: Arc<Mutex<HashSet<String>>> = Arc::default();
    let sink = Arc::clone(&labels);
    let record: Arc<dyn comrak::options::BrokenLinkCallback> =
        Arc::new(move |r: BrokenLinkReference| {
            sink.lock().unwrap().insert(r.normalized.to_string());
            None
        });
    for container in std::iter::once(None).chain((0..pre.regions.len()).map(Some)) {
        for item in pre.segments(container) {
            if let Item::Run { start, end } = item {
                let arena = Arena::new();
                let _ = parse_document(
                    &arena,
                    &pre.lines[start..end].join("\n"),
                    &options(Some(record.clone())),
                );
            }
        }
    }
    let mut labels: Vec<String> = labels.lock().unwrap().iter().cloned().collect();
    labels.sort();

    let mut text: Vec<String> = (0..pre.lines.len())
        .map(|i| {
            if pre.kinds[i] == Kind::Normal {
                pre.lines[i].clone()
            } else {
                String::new()
            }
        })
        .collect();
    let probe_start = text.len() + 2;
    text.push(String::new());
    for label in &labels {
        text.push(format!("[probe][{label}]"));
        text.push(String::new());
    }
    let arena = Arena::new();
    let root = parse_document(&arena, &text.join("\n"), &options(None));
    let mut links = Links {
        by_position: HashMap::new(),
        by_label: HashMap::new(),
    };
    for node in root.descendants() {
        let data = node.data.borrow();
        if let NodeValue::Link(link) | NodeValue::Image(link) = &data.value {
            let pos = data.sourcepos.start;
            if pos.line >= probe_start {
                links.by_label.insert(
                    labels[(pos.line - probe_start) / 2].clone(),
                    link.url.clone(),
                );
            } else {
                links
                    .by_position
                    .insert((pos.line, pos.column), link.url.clone());
            }
        }
    }
    links
}

/// A block-level element before sections are built: headings become sections afterwards (§4.2).
enum Element {
    Heading {
        level: u8,
        title: Vec<Run>,
        position: Position,
    },
    Block(Block),
}

struct Builder<'p> {
    pre: &'p PrePass,
    links: Links,
    /// Line offset of the segment being walked: comrak line + offset = absolute line.
    offset: usize,
    depth: usize,
    cells: usize,
    diagnostics: Sink,
    link_sites: Vec<(String, Position)>,
    /// For non-ASCII lines, the byte offset of each character, to turn comrak's byte columns into
    /// character columns in O(log n).
    char_starts: std::cell::RefCell<HashMap<usize, Vec<usize>>>,
}

/// §4.2: headings open sections governing what follows, up to a heading of the same or higher level.
fn sectionize(elements: Vec<Element>) -> Vec<Block> {
    let mut root: Vec<Block> = Vec::new();
    let mut stack: Vec<ir::Section> = Vec::new();
    fn attach(section: ir::Section, stack: &mut [ir::Section], root: &mut Vec<Block>) {
        match stack.last_mut() {
            Some(parent) => parent.children.push(Block::Section(section)),
            None => root.push(Block::Section(section)),
        }
    }
    for element in elements {
        match element {
            Element::Heading {
                level,
                title,
                position,
            } => {
                while stack.last().is_some_and(|top| top.level >= level) {
                    let section = stack.pop().unwrap();
                    attach(section, &mut stack, &mut root);
                }
                stack.push(ir::Section {
                    level,
                    title,
                    children: Vec::new(),
                    position: Some(position),
                });
            }
            Element::Block(block) => match stack.last_mut() {
                Some(section) => section.children.push(block),
                None => root.push(block),
            },
        }
    }
    while let Some(section) = stack.pop() {
        attach(section, &mut stack, &mut root);
    }
    root
}

impl Builder<'_> {
    fn container(&mut self, container: Option<usize>) -> Vec<Block> {
        let mut elements = Vec::new();
        for item in self.pre.segments(container) {
            match item {
                Item::Run { start, end } => {
                    let by_label = self.links.by_label.clone();
                    let callback = Arc::new(move |r: BrokenLinkReference| {
                        by_label.get(r.normalized).map(|url| ResolvedReference {
                            url: url.clone(),
                            title: String::new(),
                        })
                    });
                    let arena = Arena::new();
                    let root = parse_document(
                        &arena,
                        &self.pre.lines[start..end].join("\n"),
                        &options(Some(callback)),
                    );
                    self.offset = start;
                    for child in root.children() {
                        self.block(child, &mut elements);
                    }
                }
                Item::Directive(r) => {
                    let region = &self.pre.regions[r];
                    let position = Position::new(region.open_line + 1, 1);
                    let (name, args) = (region.name.clone(), region.args.clone());
                    let children = if self.pre.has_body(r) && self.enter(position) {
                        let children = self.container(Some(r));
                        self.depth -= 1;
                        children
                    } else {
                        Vec::new()
                    };
                    elements.push(Element::Block(Block::Directive(ir::Directive {
                        name,
                        args,
                        children,
                        position: Some(position),
                    })));
                }
            }
        }
        sectionize(elements)
    }

    /// Enters one level of nesting, or reports `limit_exceeded` (§12.1) and refuses.
    fn enter(&mut self, position: Position) -> bool {
        if self.depth >= MAX_DEPTH {
            self.diagnostics.push(
                Diagnostic::new(
                    "limit_exceeded",
                    "Nesting is deeper than 32 levels; deeper content was not processed.",
                )
                .at(position.line, Some(position.column)),
            );
            return false;
        }
        self.depth += 1;
        true
    }

    fn position(&self, node: &AstNode<'_>) -> Position {
        let pos = node.data.borrow().sourcepos.start;
        let line = pos.line + self.offset;
        Position::new(line, self.char_column(line, pos.column))
    }

    /// The 1-based character column of 1-based byte column `byte` on absolute line `line`.
    fn char_column(&self, line: usize, byte: usize) -> usize {
        let Some(text) = self.pre.lines.get(line.wrapping_sub(1)) else {
            return byte;
        };
        if text.is_ascii() {
            return byte;
        }
        let mut cache = self.char_starts.borrow_mut();
        let starts = cache
            .entry(line)
            .or_insert_with(|| text.char_indices().map(|(i, _)| i).collect());
        starts.partition_point(|&start| start < byte.saturating_sub(1)) + 1
    }

    fn unsupported(&mut self, position: Position, what: &str) {
        self.diagnostics.push(
            Diagnostic::new(
                "unsupported_content",
                format!("{what} isn't supported and was dropped."),
            )
            .at(position.line, Some(position.column)),
        );
    }

    /// The destination of a link node: the document-wide one at its position (§4.5), else comrak's.
    fn destination(&self, node: &AstNode<'_>, local: &str) -> String {
        // Keyed by comrak's own (byte) position, as `global_links` records it.
        let pos = node.data.borrow().sourcepos.start;
        self.links
            .by_position
            .get(&(pos.line + self.offset, pos.column))
            .cloned()
            .unwrap_or_else(|| local.to_string())
    }

    fn children_blocks<'a>(&mut self, node: &'a AstNode<'a>) -> Vec<Block> {
        let position = self.position(node);
        if !self.enter(position) {
            return Vec::new();
        }
        let mut elements = Vec::new();
        for child in node.children() {
            self.block(child, &mut elements);
        }
        self.depth -= 1;
        sectionize(elements)
    }

    fn block<'a>(&mut self, node: &'a AstNode<'a>, out: &mut Vec<Element>) {
        let value = node.data.borrow().value.clone();
        let position = self.position(node);
        match value {
            NodeValue::Heading(heading) => {
                let end = node.data.borrow().sourcepos.end.line + self.offset;
                if heading.setext && self.pre.lines[end - 1].trim_start().starts_with('-') {
                    self.diagnostics.push(
                        Diagnostic::new("ambiguous_break", "The line above `---` became a heading, not a break.")
                            .at_line(end)
                            .hint("Add a blank line before `---` for a break, or use `## ` for a heading."),
                    );
                }
                let title = self.inlines(node);
                out.push(Element::Heading {
                    level: heading.level,
                    title,
                    position,
                });
            }
            NodeValue::Paragraph => self.paragraph(node, position, out),
            NodeValue::Table(table) => {
                if let Some(block) = self.table(node, &table.alignments, position) {
                    out.push(Element::Block(block));
                }
            }
            NodeValue::List(list) => {
                if !self.enter(position) {
                    return;
                }
                let ordered = list.list_type == ListType::Ordered;
                let start = (ordered && list.start != 1).then_some(list.start as u64);
                let mut items = Vec::new();
                for item in node.children() {
                    let checked = match &item.data.borrow().value {
                        NodeValue::TaskItem(task) => {
                            Some(task.symbol.is_some_and(|c| c == 'x' || c == 'X'))
                        }
                        _ => None,
                    };
                    let item_position = self.position(item);
                    items.push(ir::ListItem {
                        children: self.children_blocks(item),
                        checked,
                        position: Some(item_position),
                    });
                }
                self.depth -= 1;
                out.push(Element::Block(Block::List(ir::List {
                    ordered,
                    start,
                    items,
                    position: Some(position),
                })));
            }
            NodeValue::CodeBlock(code) => {
                let lang = code
                    .info
                    .split_ascii_whitespace()
                    .next()
                    .map(str::to_string);
                let value = code
                    .literal
                    .strip_suffix('\n')
                    .unwrap_or(&code.literal)
                    .to_string();
                out.push(Element::Block(Block::Code(ir::Code {
                    lang,
                    value,
                    position: Some(position),
                })));
            }
            NodeValue::BlockQuote => {
                let children = self.children_blocks(node);
                out.push(Element::Block(Block::Quote(ir::Quote {
                    children,
                    position: Some(position),
                })));
            }
            NodeValue::ThematicBreak => out.push(Element::Block(Block::Break(ir::Break {
                position: Some(position),
            }))),
            NodeValue::HtmlBlock(html) => {
                let literal = html.literal.trim();
                if let Some(name) = marker_name(literal) {
                    out.push(Element::Block(Block::Directive(ir::Directive {
                        name,
                        args: String::new(),
                        children: Vec::new(),
                        position: Some(position),
                    })));
                } else if !is_comment(literal) {
                    self.unsupported(position, "An HTML block");
                }
            }
            NodeValue::FootnoteDefinition(_) => self.unsupported(position, "A footnote definition"),
            _ => {}
        }
    }

    fn paragraph<'a>(&mut self, node: &'a AstNode<'a>, position: Position, out: &mut Vec<Element>) {
        // §4.3/§4.7: a paragraph made only of images (each optionally linked) yields one media each.
        let mut images: Vec<(&'a AstNode<'a>, &'a AstNode<'a>, Option<String>)> = Vec::new();
        let mut only_images = true;
        for child in node.children() {
            match &child.data.borrow().value {
                NodeValue::Image(_) => images.push((child, child, None)),
                NodeValue::Link(link) => {
                    let kids: Vec<_> = child.children().collect();
                    if kids.len() == 1 && matches!(kids[0].data.borrow().value, NodeValue::Image(_))
                    {
                        images.push((child, kids[0], Some(link.url.clone())));
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
            for (anchor, image, link) in images {
                let src = match &image.data.borrow().value {
                    NodeValue::Image(link) => self.destination(image, &link.url),
                    _ => unreachable!(),
                };
                let link = link.and_then(|local| {
                    let url = self.destination(anchor, &local);
                    if destination_allowed(&url) {
                        self.link_sites.push((url.clone(), self.position(anchor)));
                        Some(url)
                    } else {
                        self.blocked(self.position(anchor), &url);
                        None
                    }
                });
                let alt = alt_text(image);
                out.push(Element::Block(Block::Media(ir::Media {
                    src,
                    alt,
                    link,
                    position: Some(self.position(anchor)),
                })));
            }
            return;
        }

        let pos = node.data.borrow().sourcepos;
        let (first, last) = (pos.start.line + self.offset, pos.end.line + self.offset);
        if last > first {
            let header = &self.pre.lines[first - 1];
            let delimiter = &self.pre.lines[first];
            let looks_like_delimiter = delimiter.contains('-')
                && delimiter
                    .trim()
                    .chars()
                    .all(|c| matches!(c, '|' | ':' | '-' | ' ' | '\t'));
            if header.contains('|')
                && looks_like_delimiter
                && split_cells(header).len() != split_cells(delimiter).len()
            {
                self.diagnostics.push(
                    Diagnostic::new("table_malformed", "This looks like a table, but its header and delimiter rows have different cell counts.")
                        .at_line(first)
                        .hint("Give the delimiter row (`|---|`) one cell per header column."),
                );
            }
        }
        let runs = self.inlines(node);
        if !runs.is_empty() {
            out.push(Element::Block(Block::Paragraph(ir::Paragraph {
                runs,
                position: Some(position),
            })));
        }
    }

    fn blocked(&mut self, position: Position, url: &str) {
        self.diagnostics.push(
            Diagnostic::new("link_scheme_blocked", format!("Link destination `{url}` isn't allowed; the link was removed and its text kept."))
                .at(position.line, Some(position.column))
                .hint("Use an http, https, mailto or tel URL, a relative path, or a #fragment."),
        );
    }

    fn inlines<'a>(&mut self, node: &'a AstNode<'a>) -> Vec<Run> {
        let mut runs = Vec::new();
        self.collect_inlines(node, &Run::default(), &mut runs);
        canonical_runs(runs)
    }

    fn collect_inlines<'a>(&mut self, node: &'a AstNode<'a>, marks: &Run, out: &mut Vec<Run>) {
        for child in node.children() {
            let value = child.data.borrow().value.clone();
            let text = |text: &str| Run {
                text: text.to_string(),
                ..marks.clone()
            };
            match value {
                NodeValue::Text(t) => out.push(text(&t)),
                NodeValue::SoftBreak => out.push(text(" ")),
                NodeValue::LineBreak => out.push(text("\n")),
                NodeValue::Code(code) => out.push(Run {
                    text: code.literal,
                    code: true,
                    ..marks.clone()
                }),
                NodeValue::Strong => self.collect_inlines(
                    child,
                    &Run {
                        bold: true,
                        ..marks.clone()
                    },
                    out,
                ),
                NodeValue::Emph => self.collect_inlines(
                    child,
                    &Run {
                        italic: true,
                        ..marks.clone()
                    },
                    out,
                ),
                NodeValue::Strikethrough => self.collect_inlines(
                    child,
                    &Run {
                        strike: true,
                        ..marks.clone()
                    },
                    out,
                ),
                NodeValue::Link(link) => {
                    let url = self.destination(child, &link.url);
                    let allowed = destination_allowed(&url);
                    if allowed {
                        self.link_sites.push((url.clone(), self.position(child)));
                    } else {
                        self.blocked(self.position(child), &url);
                    }
                    let inner = Run {
                        link: allowed.then(|| url.clone()),
                        ..marks.clone()
                    };
                    let before = out.len();
                    self.collect_inlines(child, &inner, out);
                    if allowed && out[before..].iter().all(|r| r.text.is_empty()) {
                        out.push(Run { text: url, ..inner }); // §4.4: empty link text shows the destination
                    }
                }
                NodeValue::Image(_) => {
                    self.unsupported(self.position(child), "An image inside text")
                }
                NodeValue::FootnoteReference(_) => {
                    self.unsupported(self.position(child), "A footnote reference")
                }
                NodeValue::HtmlInline(html) => {
                    if is_br(&html) {
                        out.push(text("\n"));
                    } else if is_comment(&html) {
                    } else if tag_name(&html).is_some_and(|name| {
                        HTML_ELEMENTS.contains(&name.to_ascii_lowercase().as_str())
                    }) {
                        self.unsupported(self.position(child), "An inline HTML tag");
                    } else {
                        out.push(text(&html)); // not an HTML element, e.g. `Promise<void>` (§4.8)
                    }
                }
                _ => self.collect_inlines(child, marks, out),
            }
        }
    }

    fn table<'a>(
        &mut self,
        node: &'a AstNode<'a>,
        alignments: &[TableAlignment],
        position: Position,
    ) -> Option<Block> {
        let start = node.data.borrow().sourcepos.start;
        let first = start.line + self.offset;
        let rows: Vec<_> = node.children().collect();
        let cells_of = |builder: &mut Self, row: &'a AstNode<'a>, line_index: usize| -> Vec<Cell> {
            let line = &builder.pre.lines[first - 1 + line_index];
            // `start.column` is a byte column; container prefixes before a table are ASCII.
            let source: String = match line.get(start.column - 1..) {
                Some(rest) => rest.to_string(),
                None => line
                    .chars()
                    .skip(builder.char_column(first + line_index, start.column) - 1)
                    .collect(),
            };
            let raws = split_cells(&source);
            let parsed: Vec<_> = row.children().collect();
            raws.into_iter()
                .enumerate()
                .map(|(i, raw)| {
                    // §4.6: rows keep their source cell counts; cells a parser padded are dropped, and
                    // cells it truncated are parsed from their own source text.
                    let runs = match parsed.get(i) {
                        Some(cell) => builder.inlines(cell),
                        None => inline_text(&raw),
                    };
                    Cell { runs, raw }
                })
                .collect()
        };
        let header = rows
            .first()
            .map(|row| cells_of(self, row, 0))
            .unwrap_or_default();
        let mut body = Vec::new();
        for (index, row) in rows.iter().enumerate().skip(1) {
            body.push(cells_of(self, row, index + 1));
        }
        if std::iter::once(&header)
            .chain(&body)
            .any(|row| row.len() > MAX_ROW_CELLS)
        {
            self.diagnostics.push(
                Diagnostic::new(
                    "limit_exceeded",
                    "A table row has more than 16,384 cells; the table was not processed.",
                )
                .at(position.line, Some(position.column)),
            );
            return None;
        }
        self.cells += header.len() + body.iter().map(Vec::len).sum::<usize>();
        if self.cells > MAX_TABLE_CELLS {
            self.diagnostics.push(
                Diagnostic::new("limit_exceeded", "The document has more than 1,000,000 table cells; further tables were not processed.")
                    .at(position.line, Some(position.column)),
            );
            return None;
        }
        let align = alignments
            .iter()
            .map(|a| match a {
                TableAlignment::Left => Some(Align::Left),
                TableAlignment::Center => Some(Align::Center),
                TableAlignment::Right => Some(Align::Right),
                TableAlignment::None => None,
            })
            .collect();
        Some(Block::Table(ir::Table {
            header,
            align,
            rows: body,
            position: Some(position),
        }))
    }
}

/// The inline runs of a cell's source text, for cells a parser's table tree dropped.
fn inline_text(raw: &str) -> Vec<Run> {
    let arena = Arena::new();
    let root = parse_document(&arena, raw, &options(None));
    let mut runs = Vec::new();
    for node in root.descendants() {
        match &node.data.borrow().value {
            NodeValue::Text(t) => runs.push(Run {
                text: t.to_string(),
                ..Run::default()
            }),
            NodeValue::Code(c) => runs.push(Run {
                text: c.literal.clone(),
                code: true,
                ..Run::default()
            }),
            _ => {}
        }
    }
    canonical_runs(runs)
}

/// The plain text of an image's description (§4.7).
fn alt_text<'a>(image: &'a AstNode<'a>) -> String {
    let mut alt = String::new();
    for node in image.descendants().skip(1) {
        match &node.data.borrow().value {
            NodeValue::Text(t) => alt.push_str(t),
            NodeValue::Code(c) => alt.push_str(&c.literal),
            NodeValue::SoftBreak | NodeValue::LineBreak => alt.push(' '),
            _ => {}
        }
    }
    alt
}

fn is_comment(html: &str) -> bool {
    let html = html.trim();
    html.starts_with("<!--") && html.ends_with("-->") && html.matches("-->").count() == 1
}

/// `<!-- name -->` (§1.4): the lowercase name of a marker directive.
fn marker_name(literal: &str) -> Option<String> {
    let name = literal.strip_prefix("<!--")?.strip_suffix("-->")?.trim();
    let mut chars = name.chars();
    let valid = chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    valid.then(|| name.to_ascii_lowercase())
}

/// `<br>`, `<br/>`, `<br />`, any case (§4.4).
fn is_br(html: &str) -> bool {
    let lower = html.to_ascii_lowercase();
    let Some(rest) = lower.strip_prefix("<br") else {
        return false;
    };
    let rest = rest.trim_start();
    rest == ">" || rest == "/>"
}

fn tag_name(html: &str) -> Option<&str> {
    let rest = html.strip_prefix("</").or_else(|| html.strip_prefix('<'))?;
    let len = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '-')
        .count();
    (len > 0 && rest.chars().next()?.is_ascii_alphabetic()).then(|| &rest[..len])
}

/// The §4.5 link allowlist.
pub fn destination_allowed(url: &str) -> bool {
    let scheme_len = url
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-'))
        .count();
    let has_scheme = scheme_len > 0
        && url.starts_with(|c: char| c.is_ascii_alphabetic())
        && url[scheme_len..].starts_with(':');
    if has_scheme {
        return matches!(
            url[..scheme_len].to_ascii_lowercase().as_str(),
            "http" | "https" | "mailto" | "tel"
        );
    }
    url.starts_with('#') || !(url.starts_with('/') || url.starts_with('\\') || url.contains('\\'))
}

/// GFM 0.29 cell splitting for `raw` (§4.6): unescaped pipes split, outer pipes optional, cells
/// trimmed of spaces and tabs, `\|` unescaped.
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
    if trimmed.len() > 1 && trimmed.ends_with('|') && !trimmed.ends_with("\\|") {
        cells.pop();
    }
    cells
        .into_iter()
        .map(|c| c.trim_matches([' ', '\t']).to_string())
        .collect()
}

/// §4.9 `document_in_code_fence`: the whole body is one fenced markdown code block.
fn document_in_code_fence(body: &[Block], diagnostics: &mut Sink) {
    if let [Block::Code(code)] = body
        && matches!(code.lang.as_deref(), Some("markdown" | "md"))
        && let Some(position) = code.position
    {
        diagnostics.push(
            Diagnostic::new(
                "document_in_code_fence",
                "The whole document is one markdown code block.",
            )
            .at_line(position.line)
            .hint("Remove the ```markdown fence to render the content."),
        );
    }
}

/// The plain text of a section title or paragraph, for slugs and sheet names.
pub fn text_of(runs: &[Run]) -> String {
    plain_text(runs)
}
