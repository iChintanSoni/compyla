//! IR input (markdown-spec.md §10.7): IR JSON submitted in place of markdown. It is checked against
//! the schema's structure (`spec/ir.schema.json`) and the IR invariants, then put through the same
//! text rules as parsed text. Diagnostics carry the IR `path` (a JSON Pointer), since IR input may
//! have no positions.

use serde_json::{Map, Value};

use crate::diagnostic::Diagnostic;
use crate::frontmatter;
use crate::ir::{Block, Document, Run, canonical_runs};
use crate::parse::destination_allowed;

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

/// Loads IR JSON: structure, version and invariants, then the text rules.
pub fn load(json: &str) -> Result<(Document, Vec<Diagnostic>), Refusal> {
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

    let mut diagnostics = Vec::new();
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
    Ok((document, diagnostics))
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
    let parsed = text
        .split_once('.')
        .and_then(|(major, minor)| Some((major.parse::<u64>().ok()?, minor.parse::<u64>().ok()?)));
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
        integer(&p["line"], &format!("{at}/line"), 1, u64::MAX)?;
        integer(&p["column"], &format!("{at}/column"), 1, u64::MAX)?;
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
// Text rules: §1.1 character replacement, §4.4 canonical runs, §4.5 link allowlist.

fn clean(text: &mut String, path: &str, diagnostics: &mut Vec<Diagnostic>) {
    if text.chars().any(forbidden) {
        *text = text
            .chars()
            .map(|c| if forbidden(c) { '\u{FFFD}' } else { c })
            .collect();
        diagnostics.push(Diagnostic {
            path: Some(path.to_string()),
            ..Diagnostic::new(
                "invalid_character",
                "Characters XML forbids were replaced with U+FFFD.",
            )
        });
    }
}

fn forbidden(c: char) -> bool {
    matches!(c as u32, 0x0..=0x8 | 0xB | 0xC | 0xE..=0x1F | 0xFFFE | 0xFFFF)
}

fn runs(list: &mut Vec<Run>, at: &str, diagnostics: &mut Vec<Diagnostic>) {
    for (i, run) in list.iter_mut().enumerate() {
        clean(&mut run.text, &format!("{at}/{i}/text"), diagnostics);
        if let Some(link) = &run.link
            && !destination_allowed(link)
        {
            diagnostics.push(Diagnostic {
                    path: Some(format!("{at}/{i}/link")),
                    ..Diagnostic::new("link_scheme_blocked", format!("Link destination `{link}` isn't allowed; the link was removed and its text kept."))
                });
            run.link = None;
        }
    }
    *list = canonical_runs(std::mem::take(list));
}

fn text_rules(blocks: &mut [Block], at: &str, diagnostics: &mut Vec<Diagnostic>) {
    for (i, block) in blocks.iter_mut().enumerate() {
        let at = format!("{at}/{i}");
        match block {
            Block::Section(s) => {
                runs(&mut s.title, &format!("{at}/title"), diagnostics);
                text_rules(&mut s.children, &format!("{at}/children"), diagnostics);
            }
            Block::Paragraph(p) => runs(&mut p.runs, &format!("{at}/runs"), diagnostics),
            Block::Table(t) => {
                for (j, cell) in t.header.iter_mut().enumerate() {
                    clean(&mut cell.raw, &format!("{at}/header/{j}/raw"), diagnostics);
                    runs(
                        &mut cell.runs,
                        &format!("{at}/header/{j}/runs"),
                        diagnostics,
                    );
                }
                for (r, row) in t.rows.iter_mut().enumerate() {
                    for (j, cell) in row.iter_mut().enumerate() {
                        clean(
                            &mut cell.raw,
                            &format!("{at}/rows/{r}/{j}/raw"),
                            diagnostics,
                        );
                        runs(
                            &mut cell.runs,
                            &format!("{at}/rows/{r}/{j}/runs"),
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
                clean(&mut m.alt, &format!("{at}/alt"), diagnostics);
                if let Some(link) = &m.link
                    && !destination_allowed(link)
                {
                    diagnostics.push(Diagnostic {
                        path: Some(format!("{at}/link")),
                        ..Diagnostic::new(
                            "link_scheme_blocked",
                            format!(
                                "Link destination `{link}` isn't allowed; the link was removed."
                            ),
                        )
                    });
                    m.link = None;
                }
            }
            Block::Code(c) => clean(&mut c.value, &format!("{at}/value"), diagnostics),
            Block::Quote(q) => text_rules(&mut q.children, &format!("{at}/children"), diagnostics),
            Block::Directive(d) => {
                clean(&mut d.args, &format!("{at}/args"), diagnostics);
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
