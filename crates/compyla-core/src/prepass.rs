//! Input normalization (markdown-spec.md §1.1) and the directive pre-pass (§1.3).
//!
//! The pre-pass classifies every line, builds the directive tree, and splits each container (the
//! document, or a directive body) into **segments** at directive boundaries. Segments are parsed
//! separately, so no markdown block can cross a directive (§1.3 rule 7). This is the design checked
//! by toolchain check 1 (spikes/README.md).

use crate::diagnostic::Diagnostic;
use crate::frontmatter;

const MARKERS: &[&str] = &["toc", "pagebreak", "col"];

/// A directive fence.
#[derive(Debug, Clone)]
pub struct Region {
    pub name: String,
    pub args: String,
    /// The containing directive, after unclosed directives are resolved away.
    pub parent: Option<usize>,
    /// 0-based line index of the opener.
    pub open_line: usize,
    /// 0-based line index of the closer, for directives with a body.
    pub close_line: Option<usize>,
    /// Unclosed at the end of the document (§1.3 rule 8): a leaf, with its lines returned to the parent.
    pub unclosed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Markdown content of its owner container.
    Normal,
    /// A directive opener.
    Opener(usize),
    /// A directive closer.
    Closer,
    /// Excluded from markdown: frontmatter, or a dropped stray closer.
    Blank,
}

pub struct PrePass {
    pub lines: Vec<String>,
    pub kinds: Vec<Kind>,
    /// The directive whose body owns each line; `None` is the document.
    pub owner: Vec<Option<usize>>,
    pub regions: Vec<Region>,
    pub frontmatter: Option<(usize, usize)>,
}

/// One item of a container, in document order.
pub enum Item {
    /// Lines `start..end` (0-based, end exclusive) of the container's own markdown.
    Run { start: usize, end: usize },
    /// A child directive.
    Directive(usize),
}

impl PrePass {
    pub fn segments(&self, container: Option<usize>) -> Vec<Item> {
        let (from, to) = match container {
            None => (0, self.lines.len()),
            Some(r) => (
                self.regions[r].open_line + 1,
                self.regions[r]
                    .close_line
                    .unwrap_or(self.regions[r].open_line + 1),
            ),
        };
        let mut items = Vec::new();
        let mut run: Option<usize> = None;
        for i in from..to {
            if self.kinds[i] == Kind::Normal && self.owner[i] == container {
                run.get_or_insert(i);
                continue;
            }
            if let Some(start) = run.take() {
                items.push(Item::Run { start, end: i });
            }
            if let Kind::Opener(r) = self.kinds[i]
                && self.regions[r].parent == container
            {
                items.push(Item::Directive(r));
            }
        }
        if let Some(start) = run {
            items.push(Item::Run { start, end: to });
        }
        items
    }

    /// Whether a directive has a body to parse.
    pub fn has_body(&self, r: usize) -> bool {
        self.regions[r].close_line.is_some()
    }
}

/// §1.1: line splitting, BOM removal and replacement of characters XML forbids.
fn normalize_input(source: &str, diagnostics: &mut Vec<Diagnostic>) -> Vec<String> {
    let source = source.strip_prefix('\u{FEFF}').unwrap_or(source);
    let normalized = source.replace("\r\n", "\n").replace('\r', "\n");
    let mut lines = Vec::new();
    for (index, raw) in normalized.split('\n').enumerate() {
        let mut line = String::with_capacity(raw.len());
        for (column, ch) in raw.chars().enumerate() {
            if matches!(ch as u32, 0x0..=0x8 | 0xB | 0xC | 0xE..=0x1F | 0xFFFE | 0xFFFF) {
                diagnostics.push(
                    Diagnostic::new("invalid_character", format!("Character U+{:04X} isn't allowed in documents; it was replaced with U+FFFD.", ch as u32))
                        .at(index + 1, Some(column + 1)),
                );
                line.push('\u{FFFD}');
            } else {
                line.push(ch);
            }
        }
        lines.push(line);
    }
    lines
}

fn fence_open(line: &str) -> Option<(char, usize)> {
    let indent = line.len() - line.trim_start_matches(' ').len();
    if indent > 3 {
        return None;
    }
    let rest = &line[indent..];
    let ch = rest.chars().next().filter(|&c| c == '`' || c == '~')?;
    let len = rest.chars().take_while(|&c| c == ch).count();
    if len < 3 || (ch == '`' && rest[len..].contains('`')) {
        return None;
    }
    Some((ch, len))
}

fn fence_closes(line: &str, ch: char, len: usize) -> bool {
    let indent = line.len() - line.trim_start_matches(' ').len();
    if indent > 3 {
        return false;
    }
    let rest = &line[indent..];
    let run = rest.chars().take_while(|&c| c == ch).count();
    run >= len && rest[run..].trim().is_empty()
}

/// A blockquote line's content, with its leading `>` markers removed.
fn quote_content(line: &str) -> &str {
    let mut rest = line.trim_start_matches(' ');
    while let Some(after) = rest.strip_prefix('>') {
        rest = after
            .strip_prefix(' ')
            .unwrap_or(after)
            .trim_start_matches(' ');
    }
    rest
}

/// `colons [ws] name [ws args] [[ws] colons]` → (name, args, one_line).
fn parse_opener(line: &str) -> Option<(String, String, bool)> {
    let colons = line.chars().take_while(|&c| c == ':').count();
    if colons < 3 {
        return None;
    }
    let rest = line[colons..].trim_start_matches([' ', '\t']);
    let name_len = rest
        .char_indices()
        .take_while(|&(i, c)| {
            if i == 0 {
                c.is_ascii_alphabetic()
            } else {
                c.is_ascii_alphanumeric() || c == '_' || c == '-'
            }
        })
        .count();
    if name_len == 0 {
        return None;
    }
    let name = rest[..name_len].to_ascii_lowercase();
    let after = rest[name_len..].trim_end_matches([' ', '\t']);
    let trailing = after.len() - after.trim_end_matches(':').len();
    let one_line = trailing >= 3;
    let body = if one_line {
        &after[..after.len() - trailing]
    } else {
        after
    };
    if !body.is_empty() && !body.starts_with([' ', '\t']) {
        return None;
    }
    Some((name, body.trim_matches([' ', '\t']).to_string(), one_line))
}

fn is_closer(line: &str) -> bool {
    let colons = line.chars().take_while(|&c| c == ':').count();
    colons >= 3 && line[colons..].trim_matches([' ', '\t']).is_empty()
}

pub fn run(source: &str, diagnostics: &mut Vec<Diagnostic>) -> PrePass {
    let lines = normalize_input(source, diagnostics);
    let n = lines.len();
    let mut kinds = vec![Kind::Normal; n];
    let mut owner: Vec<Option<usize>> = vec![None; n];
    let mut regions: Vec<Region> = Vec::new();

    let frontmatter = frontmatter::range(&lines);
    let start = match frontmatter {
        Some((open, close)) => {
            for kind in &mut kinds[open..=close] {
                *kind = Kind::Blank;
            }
            close + 1
        }
        None => 0,
    };

    let mut stack: Vec<usize> = Vec::new();
    let mut fence: Option<(char, usize)> = None;
    let mut quoted_fence: Option<(char, usize)> = None;
    let mut marker_open: Option<usize> = None;
    let malformed = |line: usize, column: usize| {
        Diagnostic::new(
            "directive_malformed",
            "This line looks like a directive fence but isn't one; it is kept as text.",
        )
        .at(line + 1, Some(column))
        .hint("Directive fences start at column 1: `::: name args` … `:::`.")
    };

    for i in start..n {
        let line = &lines[i];
        owner[i] = stack.last().copied();
        let marker_line = marker_open.take();

        if let Some((ch, len)) = fence {
            if fence_closes(line, ch, len) {
                fence = None;
            }
            continue;
        }
        if let Some(open) = fence_open(line) {
            fence = Some(open);
            continue;
        }

        if line.starts_with(":::") {
            if is_closer(line) {
                if marker_line.is_some_and(|m| m + 1 == i) {
                    kinds[i] = Kind::Closer; // the closer of the marker on the previous line (rule 5)
                } else if let Some(r) = stack.pop() {
                    kinds[i] = Kind::Closer;
                    owner[i] = Some(r);
                    regions[r].close_line = Some(i);
                } else {
                    diagnostics.push(malformed(i, 1));
                    kinds[i] = Kind::Blank; // a stray closer is dropped but still ends the previous block
                }
            } else if let Some((name, args, one_line)) = parse_opener(line) {
                let r = regions.len();
                let marker = MARKERS.contains(&name.as_str());
                regions.push(Region {
                    name,
                    args,
                    parent: stack.last().copied(),
                    open_line: i,
                    close_line: None,
                    unclosed: false,
                });
                kinds[i] = Kind::Opener(r);
                if marker && !one_line {
                    marker_open = Some(i);
                } else if !one_line {
                    stack.push(r);
                }
            } else {
                diagnostics.push(malformed(i, 1));
            }
            continue;
        }

        // Fence-like lines off column 1 (rule 9), outside fenced code, including quoted fences.
        let stripped = line.trim_start_matches(' ');
        let indent = line.len() - stripped.len();
        let quoted = indent <= 3 && stripped.starts_with('>');
        let content = quote_content(line);
        if quoted {
            if let Some((ch, len)) = quoted_fence {
                if fence_closes(content, ch, len) {
                    quoted_fence = None;
                }
                continue;
            }
            if let Some(open) = fence_open(content) {
                quoted_fence = Some(open);
                continue;
            }
        } else {
            quoted_fence = None;
        }
        let candidate = if quoted {
            Some(content)
        } else if (1..=3).contains(&indent) {
            Some(stripped)
        } else {
            None
        };
        if let Some(rest) = candidate
            && rest.starts_with(":::")
            && (is_closer(rest) || parse_opener(rest).is_some())
        {
            let column = line[..line.find(":::").unwrap_or(0)].chars().count() + 1;
            diagnostics.push(malformed(i, column));
        }
    }

    for &r in stack.iter().rev() {
        regions[r].unclosed = true;
        diagnostics.push(
            Diagnostic::new(
                "directive_unclosed",
                format!(
                    "`::: {}` has no closing `:::`; it applies to what follows.",
                    regions[r].name
                ),
            )
            .at_line(regions[r].open_line + 1)
            .hint("Close the directive with a line containing only `:::`."),
        );
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
    for slot in &mut owner {
        *slot = resolve(*slot, &regions);
    }
    for r in 0..regions.len() {
        regions[r].parent = resolve(regions[r].parent, &regions);
        if regions[r].unclosed {
            regions[r].close_line = None;
        }
    }

    PrePass {
        lines,
        kinds,
        owner,
        regions,
        frontmatter,
    }
}
