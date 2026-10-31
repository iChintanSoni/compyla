//! IR input (markdown-spec.md §10.7): IR JSON submitted in place of markdown. It is checked against
//! the schema's structure (`spec/ir.schema.json`) and the IR invariants, then put through the same
//! text rules as parsed text. Diagnostics carry the IR `path` (a JSON Pointer), since IR input may
//! have no positions.

use serde_json::{Map, Value};

use crate::diagnostic::{Blocking, Diagnostic, Sink};
use crate::frontmatter;
use crate::ir::{self, Block, Document, Position, Run, canonical_runs};
use crate::parse::{
    MAX_DEPTH, MAX_ROW_CELLS, MAX_TABLE_CELLS, destination_allowed, input_too_large,
};
use crate::validate::row_line;

/// The largest line or column IR input may give, so positions mean the same on 32- and 64-bit
/// hosts (spec/ir.schema.json).
const MAX_POSITION: u64 = u32::MAX as u64;

/// The IR versions this implementation reads: `0.Z` for every `Z ≤ 1` (§10.7).
const SUPPORTED_MAJOR: u64 = 0;
const SUPPORTED_MINOR: u64 = 1;

/// Why IR input was refused: an operation error (§10.6), with the JSON Pointer of the first failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    pub code: &'static str,
    pub pointer: String,
    pub message: String,
}

/// Loaded IR input.
#[derive(Debug)]
pub struct Loaded {
    pub document: Document,
    pub diagnostics: Vec<Diagnostic>,
    /// Whether any load diagnostic, kept or not, blocks `convert` (§11.1).
    pub blocking: Blocking,
}

/// Loads IR JSON: structure, version and invariants, then the limits and the text rules. When a
/// limit is exceeded (§12.1), the diagnostics include `limit_exceeded` and the document must not
/// be processed further.
pub fn load(json: &str) -> Result<Loaded, Refusal> {
    if let Some(diagnostic) = input_too_large(json.len()) {
        let document = Document {
            ir_version: ir::IR_VERSION.to_string(),
            meta: Map::new(),
            body: Vec::new(),
        };
        return Ok(Loaded {
            document,
            diagnostics: vec![diagnostic],
            blocking: Blocking {
                error: true,
                promotable: false,
            },
        });
    }
    let value: Value =
        serde_json::from_str(json).map_err(|e| refuse("", format!("not valid JSON: {e}")))?;
    let root = value
        .as_object()
        .ok_or_else(|| refuse("", "the IR must be a JSON object".into()))?;
    version(root)?;
    check_document(root)?;
    let mut document: Document =
        serde_json::from_value(value).map_err(|e| refuse("", e.to_string()))?;
    invariants(&document.body, "/body")?;

    let mut diagnostics = Sink::new();
    if let Some(diagnostic) = limits(&document.body, "/body", 0, &mut 0) {
        diagnostics.push(diagnostic);
        let (diagnostics, blocking) = diagnostics.into_parts();
        return Ok(Loaded {
            document,
            diagnostics,
            blocking,
        });
    }
    for (key, value) in document.meta.iter_mut() {
        clean_value(value, &format!("/meta/{}", escape(key)), &mut diagnostics);
    }
    let locate = |key: &str, d: Diagnostic| Diagnostic {
        path: Some(format!("/meta/{}", escape(key))),
        ..d
    };
    document.meta = frontmatter::normalize(
        std::mem::take(&mut document.meta),
        &locate,
        &mut diagnostics,
    );
    text_rules(&mut document.body, "/body", &mut diagnostics);
    let (diagnostics, blocking) = diagnostics.into_parts();
    Ok(Loaded {
        document,
        diagnostics,
        blocking,
    })
}

fn refuse(pointer: &str, message: String) -> Refusal {
    Refusal {
        code: "ir_invalid",
        pointer: pointer.to_string(),
        message,
    }
}

fn version(root: &Map<String, Value>) -> Result<(), Refusal> {
    let Some(text) = root.get("irVersion").and_then(Value::as_str) else {
        return Err(refuse(
            "/irVersion",
            "`irVersion` must be a string such as \"0.1\"".into(),
        ));
    };
    // `0`, or digits without a leading zero: `+1` and `01` aren't version numbers.
    let number = |part: &str| {
        let canonical = part == "0"
            || (!part.is_empty()
                && !part.starts_with('0')
                && part.bytes().all(|b| b.is_ascii_digit()));
        canonical.then(|| part.parse::<u64>().ok()).flatten()
    };
    let parsed = text
        .split_once('.')
        .and_then(|(major, minor)| Some((number(major)?, number(minor)?)));
    match parsed {
        Some((SUPPORTED_MAJOR, minor)) if minor <= SUPPORTED_MINOR => Ok(()),
        _ => Err(Refusal {
            code: "ir_version_unsupported",
            pointer: "/irVersion".into(),
            message: format!(
                "IR version {text} isn't supported; this implementation reads {SUPPORTED_MAJOR}.0–{SUPPORTED_MAJOR}.{SUPPORTED_MINOR}"
            ),
        }),
    }
}

// ---------------------------------------------------------------------------------------------
// Structure: the shape spec/ir.schema.json defines, with the pointer of the first failure.

type Check = Result<(), Refusal>;

fn object<'v>(
    value: &'v Value,
    at: &str,
    required: &[&str],
    optional: &[&str],
) -> Result<&'v Map<String, Value>, Refusal> {
    let map = value
        .as_object()
        .ok_or_else(|| refuse(at, "expected an object".into()))?;
    for key in required {
        if !map.contains_key(*key) {
            return Err(refuse(at, format!("missing `{key}`")));
        }
    }
    for key in map.keys() {
        if !required.contains(&key.as_str()) && !optional.contains(&key.as_str()) {
            return Err(refuse(
                &format!("{at}/{}", escape(key)),
                format!("unexpected property `{key}`"),
            ));
        }
    }
    Ok(map)
}

fn array<'v>(value: &'v Value, at: &str) -> Result<&'v Vec<Value>, Refusal> {
    value
        .as_array()
        .ok_or_else(|| refuse(at, "expected an array".into()))
}

fn string(value: &Value, at: &str) -> Check {
    value
        .as_str()
        .map(|_| ())
        .ok_or_else(|| refuse(at, "expected a string".into()))
}

fn integer(value: &Value, at: &str, min: u64, max: u64) -> Check {
    match value.as_u64() {
        Some(n) if (min..=max).contains(&n) => Ok(()),
        _ => Err(refuse(
            at,
            format!("expected an integer from {min} to {max}"),
        )),
    }
}

fn check_document(root: &Map<String, Value>) -> Check {
    object(
        &Value::Object(root.clone()),
        "",
        &["irVersion", "meta", "body"],
        &[],
    )?;
    root["meta"]
        .as_object()
        .ok_or_else(|| refuse("/meta", "expected an object".into()))?;
    check_blocks(&root["body"], "/body")
}

fn check_position(map: &Map<String, Value>, at: &str) -> Check {
    if let Some(position) = map.get("position") {
        let at = format!("{at}/position");
        let p = object(position, &at, &["line", "column"], &[])?;
        integer(&p["line"], &format!("{at}/line"), 1, MAX_POSITION)?;
        integer(&p["column"], &format!("{at}/column"), 1, MAX_POSITION)?;
    }
    Ok(())
}

fn check_runs(value: &Value, at: &str) -> Check {
    for (i, run) in array(value, at)?.iter().enumerate() {
        let at = format!("{at}/{i}");
        let map = object(
            run,
            &at,
            &["text"],
            &["bold", "italic", "code", "strike", "link"],
        )?;
        string(&map["text"], &format!("{at}/text"))?;
        for mark in ["bold", "italic", "code", "strike"] {
            if let Some(value) = map.get(mark)
                && value != &Value::Bool(true)
            {
                return Err(refuse(
                    &format!("{at}/{mark}"),
                    "a mark is `true` or absent".into(),
                ));
            }
        }
        if let Some(link) = map.get("link") {
            string(link, &format!("{at}/link"))?;
        }
    }
    Ok(())
}

fn check_cells(value: &Value, at: &str) -> Check {
    for (i, cell) in array(value, at)?.iter().enumerate() {
        let at = format!("{at}/{i}");
        let map = object(cell, &at, &["runs", "raw"], &[])?;
        check_runs(&map["runs"], &format!("{at}/runs"))?;
        string(&map["raw"], &format!("{at}/raw"))?;
    }
    Ok(())
}

fn check_blocks(value: &Value, at: &str) -> Check {
    for (i, block) in array(value, at)?.iter().enumerate() {
        check_block(block, &format!("{at}/{i}"))?;
    }
    Ok(())
}

fn check_block(block: &Value, at: &str) -> Check {
    let kind = block
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| refuse(at, "a block needs a string `type`".into()))?;
    let (required, optional): (&[&str], &[&str]) = match kind {
        "section" => (&["type", "level", "title", "children"], &["position"]),
        "paragraph" => (&["type", "runs"], &["position"]),
        "table" => (&["type", "header", "align", "rows"], &["position"]),
        "list" => (&["type", "ordered", "items"], &["start", "position"]),
        "media" => (&["type", "src", "alt"], &["link", "position"]),
        "code" => (&["type", "value"], &["lang", "position"]),
        "quote" => (&["type", "children"], &["position"]),
        "break" => (&["type"], &["position"]),
        "directive" => (&["type", "name", "args", "children"], &["position"]),
        other => {
            return Err(refuse(
                &format!("{at}/type"),
                format!("unknown block type `{other}`"),
            ));
        }
    };
    let map = object(block, at, required, optional)?;
    check_position(map, at)?;
    let field = |key: &str| format!("{at}/{key}");
    match kind {
        "section" => {
            integer(&map["level"], &field("level"), 1, 6)?;
            check_runs(&map["title"], &field("title"))?;
            check_blocks(&map["children"], &field("children"))?;
        }
        "paragraph" => check_runs(&map["runs"], &field("runs"))?,
        "table" => {
            check_cells(&map["header"], &field("header"))?;
            for (i, align) in array(&map["align"], &field("align"))?.iter().enumerate() {
                if !matches!(align.as_str(), Some("left" | "center" | "right")) && !align.is_null()
                {
                    return Err(refuse(
                        &format!("{}/{i}", field("align")),
                        "expected left, center, right or null".into(),
                    ));
                }
            }
            for (i, row) in array(&map["rows"], &field("rows"))?.iter().enumerate() {
                check_cells(row, &format!("{}/{i}", field("rows")))?;
            }
        }
        "list" => {
            if !map["ordered"].is_boolean() {
                return Err(refuse(&field("ordered"), "expected a boolean".into()));
            }
            if let Some(start) = map.get("start") {
                integer(start, &field("start"), 0, u64::MAX)?;
            }
            for (i, item) in array(&map["items"], &field("items"))?.iter().enumerate() {
                let at = format!("{}/{i}", field("items"));
                let item = object(item, &at, &["children", "checked"], &["position"])?;
                check_position(item, &at)?;
                if !item["checked"].is_boolean() && !item["checked"].is_null() {
                    return Err(refuse(
                        &format!("{at}/checked"),
                        "expected true, false or null".into(),
                    ));
                }
                check_blocks(&item["children"], &format!("{at}/children"))?;
            }
        }
        "media" => {
            string(&map["src"], &field("src"))?;
            string(&map["alt"], &field("alt"))?;
            if let Some(link) = map.get("link") {
                string(link, &field("link"))?;
            }
        }
        "code" => {
            string(&map["value"], &field("value"))?;
            if let Some(lang) = map.get("lang") {
                string(lang, &field("lang"))?;
            }
        }
        "quote" => check_blocks(&map["children"], &field("children"))?,
        "directive" => {
            let name = map["name"].as_str().unwrap_or("");
            let valid = name.starts_with(|c: char| c.is_ascii_lowercase())
                && name
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-');
            if !valid {
                return Err(refuse(
                    &field("name"),
                    "a directive name is lowercase: [a-z][a-z0-9_-]*".into(),
                ));
            }
            string(&map["args"], &field("args"))?;
            check_blocks(&map["children"], &field("children"))?;
        }
        _ => {}
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Invariants (§10.7).

fn invariants(blocks: &[Block], at: &str) -> Check {
    for (i, block) in blocks.iter().enumerate() {
        let at = format!("{at}/{i}");
        match block {
            Block::Section(s) => {
                for (j, child) in s.children.iter().enumerate() {
                    if let Block::Section(c) = child
                        && c.level <= s.level
                    {
                        return Err(refuse(
                            &format!("{at}/children/{j}/level"),
                            format!(
                                "a child section's level must be greater than its parent's ({})",
                                s.level
                            ),
                        ));
                    }
                }
                invariants(&s.children, &format!("{at}/children"))?;
            }
            Block::Table(t) => {
                if t.align.len() != t.header.len() {
                    return Err(refuse(
                        &format!("{at}/align"),
                        format!(
                            "`align` has {} entries; the header has {} cells",
                            t.align.len(),
                            t.header.len()
                        ),
                    ));
                }
            }
            Block::List(l) => {
                if l.start.is_some() && !l.ordered {
                    return Err(refuse(
                        &format!("{at}/start"),
                        "`start` appears only on ordered lists".into(),
                    ));
                }
                for (j, item) in l.items.iter().enumerate() {
                    invariants(&item.children, &format!("{at}/items/{j}/children"))?;
                }
            }
            Block::Quote(q) => invariants(&q.children, &format!("{at}/children"))?,
            Block::Directive(d) => invariants(&d.children, &format!("{at}/children"))?,
            _ => {}
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Limits (§12.1), counted as the parser counts them: a list and each of its items, a quote and a
// directive body are one level of nesting each; table cells per row and per document.

fn limits(blocks: &[Block], at: &str, depth: usize, cells: &mut usize) -> Option<Diagnostic> {
    for (i, block) in blocks.iter().enumerate() {
        let at = format!("{at}/{i}");
        let exceeded = |message: &str| Some(limit(message, &at, block.position()));
        let deeper = "Nesting is deeper than 32 levels; the input was not processed further.";
        let found = match block {
            Block::Section(s) => limits(&s.children, &format!("{at}/children"), depth, cells),
            Block::Quote(q) if depth < MAX_DEPTH => {
                limits(&q.children, &format!("{at}/children"), depth + 1, cells)
            }
            Block::Directive(d) if depth < MAX_DEPTH => {
                limits(&d.children, &format!("{at}/children"), depth + 1, cells)
            }
            Block::Quote(_) | Block::Directive(_) => exceeded(deeper),
            Block::List(l) if depth < MAX_DEPTH => {
                l.items.iter().enumerate().find_map(|(j, item)| {
                    let item_at = format!("{at}/items/{j}");
                    if depth + 1 < MAX_DEPTH {
                        limits(
                            &item.children,
                            &format!("{item_at}/children"),
                            depth + 2,
                            cells,
                        )
                    } else {
                        Some(limit(deeper, &item_at, item.position))
                    }
                })
            }
            Block::List(_) => exceeded(deeper),
            Block::Table(t) => {
                if std::iter::once(&t.header)
                    .chain(&t.rows)
                    .any(|row| row.len() > MAX_ROW_CELLS)
                {
                    exceeded(
                        "A table row has more than 16,384 cells; the input was not processed further.",
                    )
                } else {
                    *cells += t.header.len() + t.rows.iter().map(Vec::len).sum::<usize>();
                    (*cells > MAX_TABLE_CELLS).then(|| {
                        limit(
                            "The document has more than 1,000,000 table cells; the input was not processed further.",
                            &at,
                            t.position,
                        )
                    })
                }
            }
            _ => None,
        };
        if found.is_some() {
            return found;
        }
    }
    None
}

fn limit(message: &str, path: &str, position: Option<Position>) -> Diagnostic {
    let diagnostic = Diagnostic {
        path: Some(path.to_string()),
        ..Diagnostic::new("limit_exceeded", message)
    };
    match position {
        Some(p) => diagnostic.at(p.line, Some(p.column)),
        None => diagnostic,
    }
}

// ---------------------------------------------------------------------------------------------
// Text rules: §1.1 character replacement (in every string), §4.4 canonical runs, §4.5 link
// allowlist. A diagnostic carries its node's line when the node has a position (§10.7); a
// replaced character's column isn't known, so it has none.

fn clean(text: &mut String, path: &str, line: Option<usize>, diagnostics: &mut Sink) {
    if text.chars().any(forbidden) {
        *text = text
            .chars()
            .map(|c| if forbidden(c) { '\u{FFFD}' } else { c })
            .collect();
        diagnostics.push(Diagnostic {
            path: Some(path.to_string()),
            line,
            ..Diagnostic::new(
                "invalid_character",
                "Characters XML forbids were replaced with U+FFFD.",
            )
        });
    }
}

/// Every string in a `meta` value, including inside arrays and objects.
fn clean_value(value: &mut Value, path: &str, diagnostics: &mut Sink) {
    match value {
        Value::String(text) => clean(text, path, None, diagnostics),
        Value::Array(items) => {
            for (i, item) in items.iter_mut().enumerate() {
                clean_value(item, &format!("{path}/{i}"), diagnostics);
            }
        }
        Value::Object(map) => {
            for (key, item) in map.iter_mut() {
                clean_value(item, &format!("{path}/{}", escape(key)), diagnostics);
            }
        }
        _ => {}
    }
}

fn forbidden(c: char) -> bool {
    matches!(c as u32, 0x0..=0x8 | 0xB | 0xC | 0xE..=0x1F | 0xFFFE | 0xFFFF)
}

/// A link destination: cleaned, then removed unless the allowlist admits it.
fn link(link: &mut Option<String>, path: &str, line: Option<usize>, diagnostics: &mut Sink) {
    let Some(url) = link else { return };
    clean(url, path, line, diagnostics);
    if !destination_allowed(url) {
        diagnostics.push(Diagnostic {
            path: Some(path.to_string()),
            line,
            ..Diagnostic::new(
                "link_scheme_blocked",
                format!("Link destination `{url}` isn't allowed; the link was removed."),
            )
        });
        *link = None;
    }
}

fn runs(list: &mut Vec<Run>, at: &str, line: Option<usize>, diagnostics: &mut Sink) {
    for (i, run) in list.iter_mut().enumerate() {
        clean(&mut run.text, &format!("{at}/{i}/text"), line, diagnostics);
        link(&mut run.link, &format!("{at}/{i}/link"), line, diagnostics);
    }
    *list = canonical_runs(std::mem::take(list));
}

fn text_rules(blocks: &mut [Block], at: &str, diagnostics: &mut Sink) {
    for (i, block) in blocks.iter_mut().enumerate() {
        let at = format!("{at}/{i}");
        let line = block.position().map(|p| p.line);
        match block {
            Block::Section(s) => {
                runs(&mut s.title, &format!("{at}/title"), line, diagnostics);
                text_rules(&mut s.children, &format!("{at}/children"), diagnostics);
            }
            Block::Paragraph(p) => runs(&mut p.runs, &format!("{at}/runs"), line, diagnostics),
            Block::Table(t) => {
                for (j, cell) in t.header.iter_mut().enumerate() {
                    let cell_at = format!("{at}/header/{j}");
                    clean(&mut cell.raw, &format!("{cell_at}/raw"), line, diagnostics);
                    runs(
                        &mut cell.runs,
                        &format!("{cell_at}/runs"),
                        line,
                        diagnostics,
                    );
                }
                for (r, row) in t.rows.iter_mut().enumerate() {
                    let row_line = t.position.map(|p| row_line(p, r));
                    for (j, cell) in row.iter_mut().enumerate() {
                        let cell_at = format!("{at}/rows/{r}/{j}");
                        clean(
                            &mut cell.raw,
                            &format!("{cell_at}/raw"),
                            row_line,
                            diagnostics,
                        );
                        runs(
                            &mut cell.runs,
                            &format!("{cell_at}/runs"),
                            row_line,
                            diagnostics,
                        );
                    }
                }
            }
            Block::List(l) => {
                for (j, item) in l.items.iter_mut().enumerate() {
                    text_rules(
                        &mut item.children,
                        &format!("{at}/items/{j}/children"),
                        diagnostics,
                    );
                }
            }
            Block::Media(m) => {
                clean(&mut m.src, &format!("{at}/src"), line, diagnostics);
                clean(&mut m.alt, &format!("{at}/alt"), line, diagnostics);
                link(&mut m.link, &format!("{at}/link"), line, diagnostics);
            }
            Block::Code(c) => {
                clean(&mut c.value, &format!("{at}/value"), line, diagnostics);
                if let Some(lang) = &mut c.lang {
                    clean(lang, &format!("{at}/lang"), line, diagnostics);
                }
            }
            Block::Quote(q) => text_rules(&mut q.children, &format!("{at}/children"), diagnostics),
            Block::Directive(d) => {
                clean(&mut d.args, &format!("{at}/args"), line, diagnostics);
                text_rules(&mut d.children, &format!("{at}/children"), diagnostics);
            }
            Block::Break(_) => {}
        }
    }
}

/// RFC 6901 escaping of one reference token.
fn escape(token: &str) -> String {
    token.replace('~', "~0").replace('/', "~1")
}
