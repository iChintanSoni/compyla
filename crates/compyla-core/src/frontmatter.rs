//! Frontmatter (markdown-spec.md §2): a JSON-compatible subset of YAML 1.2, parsed from saphyr's
//! event stream so that anchors, aliases, tags, merge keys and duplicate keys can be rejected, and
//! each top-level key's line reported.

use std::collections::HashMap;

use saphyr_parser::{Event, Parser, ScalarStyle};
use serde_json::{Map, Number, Value};

use crate::diagnostic::{Diagnostic, Sink};

/// The implemented spec version (markdown-spec.md §2, `spec`).
pub const SPEC_VERSION: &str = "0.2";

const MAX_FRONTMATTER: usize = 64 * 1024;
const MAX_SAFE_INTEGER: i64 = (1 << 53) - 1;
const FORMATS: &[&str] = &["docx", "pdf", "pptx", "xlsx"];

/// The frontmatter's line range: 0-based indices of the opening and closing `---` lines.
pub fn range(lines: &[String]) -> Option<(usize, usize)> {
    let is_delimiter = |line: &String| line.trim_end_matches([' ', '\t']) == "---";
    if !lines.first().is_some_and(is_delimiter) {
        return None;
    }
    let end = lines.iter().skip(1).position(is_delimiter)? + 1;
    Some((0, end))
}

/// Parses and normalizes the frontmatter between `range`'s delimiters into `meta`, with the file line
/// (1-based) of each top-level key, for diagnostics about a key (markdown-spec.md §2).
pub fn parse(
    lines: &[String],
    (open, close): (usize, usize),
    diagnostics: &mut Sink,
) -> (Map<String, Value>, HashMap<String, usize>) {
    let content = lines[open + 1..close].join("\n");
    let parsed = if content.len() > MAX_FRONTMATTER {
        Err("frontmatter is longer than 64 KiB".to_string())
    } else {
        parse_yaml(&content)
    };
    let (raw, key_lines) = match parsed {
        Ok(result) => result,
        Err(reason) => {
            diagnostics.push(
                Diagnostic::new("frontmatter_invalid", format!("Frontmatter can't be read: {reason}. It is ignored."))
                    .at_line(1)
                    .hint("Frontmatter must be a YAML mapping of plain values: no anchors, aliases, tags or duplicate keys."),
            );
            return (Map::new(), HashMap::new());
        }
    };
    // Frontmatter content line k (1-based) is file line open + 1 + k.
    let lines: HashMap<String, usize> = key_lines
        .into_iter()
        .map(|(key, line)| (key, open + 1 + line))
        .collect();
    let locate = |key: &str, d: Diagnostic| match lines.get(key) {
        Some(&line) => d.at_line(line),
        None => d,
    };
    let meta = normalize(raw, &locate, diagnostics);
    (meta, lines)
}

type KeyLines = Vec<(String, usize)>;

fn parse_yaml(text: &str) -> Result<(Map<String, Value>, KeyLines), String> {
    let mut events = Parser::new_from_str(text).map(|e| e.map_err(|err| err.info().to_string()));
    let mut next = move || {
        events
            .next()
            .unwrap_or_else(|| Err("unexpected end of YAML".to_string()))
    };

    match next()?.0 {
        Event::StreamStart => {}
        _ => return Err("not a YAML stream".into()),
    }
    let (event, _) = next()?;
    let value = match event {
        Event::StreamEnd => return Ok((Map::new(), Vec::new())),
        Event::DocumentStart(_) => {
            let mut key_lines = Vec::new();
            let (first, span) = next()?;
            let value = node(first, span.start.line(), &mut next, Some(&mut key_lines))?;
            match next()?.0 {
                Event::DocumentEnd => {}
                _ => return Err("unexpected content after the document".into()),
            }
            match next()?.0 {
                Event::StreamEnd => {}
                _ => return Err("multiple YAML documents".into()),
            }
            (value, key_lines)
        }
        _ => return Err("not a YAML document".into()),
    };
    match value {
        (Value::Object(map), key_lines) => Ok((map, key_lines)),
        (Value::Null, _) => Ok((Map::new(), Vec::new())),
        _ => Err("frontmatter is not a mapping".into()),
    }
}

type Next<'a> = dyn FnMut() -> Result<(Event<'a>, saphyr_parser::Span), String> + 'a;

/// Builds a JSON value from one YAML node. `key_lines` collects the top-level mapping's keys.
fn node<'a>(
    event: Event<'a>,
    _line: usize,
    next: &mut Next<'a>,
    key_lines: Option<&mut KeyLines>,
) -> Result<Value, String> {
    match event {
        Event::Alias(_) => Err("aliases are not allowed".into()),
        Event::Scalar(text, style, anchor, tag) => {
            if anchor != 0 || tag.is_some() {
                return Err("anchors and tags are not allowed".into());
            }
            scalar(&text, style)
        }
        Event::SequenceStart(anchor, tag) => {
            if anchor != 0 || tag.is_some() {
                return Err("anchors and tags are not allowed".into());
            }
            let mut items = Vec::new();
            loop {
                let (event, span) = next()?;
                if matches!(event, Event::SequenceEnd) {
                    return Ok(Value::Array(items));
                }
                items.push(node(event, span.start.line(), next, None)?);
            }
        }
        Event::MappingStart(anchor, tag) => {
            if anchor != 0 || tag.is_some() {
                return Err("anchors and tags are not allowed".into());
            }
            let mut map = Map::new();
            let mut key_lines = key_lines;
            loop {
                let (event, span) = next()?;
                let key = match event {
                    Event::MappingEnd => return Ok(Value::Object(map)),
                    Event::Scalar(text, _, anchor, tag) => {
                        if anchor != 0 || tag.is_some() {
                            return Err("anchors and tags are not allowed".into());
                        }
                        text.into_owned()
                    }
                    Event::Alias(_) => return Err("aliases are not allowed".into()),
                    _ => return Err("complex keys are not allowed".into()),
                };
                if key == "<<" {
                    return Err("merge keys are not allowed".into());
                }
                if map.contains_key(&key) {
                    return Err(format!("duplicate key `{key}`"));
                }
                if let Some(lines) = key_lines.as_deref_mut() {
                    lines.push((key.clone(), span.start.line()));
                }
                let (value_event, value_span) = next()?;
                let value = node(value_event, value_span.start.line(), next, None)?;
                map.insert(key, value);
            }
        }
        _ => Err("unexpected YAML event".into()),
    }
}

/// Resolves a scalar per the YAML 1.2 core schema; quoted scalars are always strings.
fn scalar(text: &str, style: ScalarStyle) -> Result<Value, String> {
    if style != ScalarStyle::Plain {
        return Ok(Value::String(text.to_string()));
    }
    match text {
        "" | "~" | "null" | "Null" | "NULL" => return Ok(Value::Null),
        "true" | "True" | "TRUE" => return Ok(Value::Bool(true)),
        "false" | "False" | "FALSE" => return Ok(Value::Bool(false)),
        ".inf" | ".Inf" | ".INF" | "+.inf" | "+.Inf" | "+.INF" | "-.inf" | "-.Inf" | "-.INF"
        | ".nan" | ".NaN" | ".NAN" => {
            return Err("non-finite numbers are not allowed".into());
        }
        _ => {}
    }
    let integer = |digits: &str, radix: u32| -> Result<Value, String> {
        let value =
            i64::from_str_radix(digits, radix).map_err(|_| "integer out of range".to_string())?;
        if value.abs() > MAX_SAFE_INTEGER {
            return Err("integers must be within ±(2^53−1)".into());
        }
        Ok(Value::Number(value.into()))
    };
    if let Some(octal) = text
        .strip_prefix("0o")
        .filter(|d| !d.is_empty() && d.bytes().all(|b| (b'0'..=b'7').contains(&b)))
    {
        return integer(octal, 8);
    }
    if let Some(hex) = text
        .strip_prefix("0x")
        .filter(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        return integer(hex, 16);
    }
    let unsigned = text.strip_prefix(['-', '+']).unwrap_or(text);
    if !unsigned.is_empty() && unsigned.bytes().all(|b| b.is_ascii_digit()) {
        return integer(text.strip_prefix('+').unwrap_or(text), 10);
    }
    if is_float(unsigned) {
        let value: f64 = text.parse().map_err(|_| "invalid number".to_string())?;
        return Number::from_f64(value)
            .map(Value::Number)
            .ok_or_else(|| "non-finite numbers are not allowed".into());
    }
    Ok(Value::String(text.to_string()))
}

/// `(\.[0-9]+|[0-9]+(\.[0-9]*)?)([eE][-+]?[0-9]+)?`, with a sign already removed.
fn is_float(text: &str) -> bool {
    let (mantissa, exponent) = match text.find(['e', 'E']) {
        Some(at) => (&text[..at], Some(&text[at + 1..])),
        None => (text, None),
    };
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    let mantissa_ok = match mantissa.split_once('.') {
        Some(("", fraction)) => digits(fraction),
        Some((whole, fraction)) => digits(whole) && (fraction.is_empty() || digits(fraction)),
        None => digits(mantissa),
    };
    let exponent_ok = exponent.is_none_or(|e| digits(e.strip_prefix(['-', '+']).unwrap_or(e)));
    mantissa_ok && exponent_ok && (mantissa.contains('.') || exponent.is_some())
}

/// Known-key checking and normalization (markdown-spec.md §2). `locate` positions a diagnostic about
/// a key: at its frontmatter line for markdown, at its IR path for IR input (§10.7).
pub(crate) fn normalize(
    raw: Map<String, Value>,
    locate: &dyn Fn(&str, Diagnostic) -> Diagnostic,
    diagnostics: &mut Sink,
) -> Map<String, Value> {
    let mut meta = Map::new();
    for (key, value) in raw {
        let invalid = |diagnostics: &mut Sink, expected: &str| {
            let d = Diagnostic::new(
                "meta_value_invalid",
                format!("Frontmatter `{key}` must be {expected}; it is ignored."),
            );
            diagnostics.push(locate(&key, d));
        };
        let normalized = match key.as_str() {
            "format" => match text_of(&value).map(|s| s.to_ascii_lowercase()) {
                Some(format) if FORMATS.contains(&format.as_str()) => Some(Value::String(format)),
                _ => {
                    invalid(diagnostics, "one of docx, pdf, pptx, xlsx");
                    None
                }
            },
            "template" | "title" | "page_size" | "margins" => match text_of(&value) {
                Some(text) => Some(Value::String(text)),
                None => {
                    invalid(diagnostics, "a string");
                    None
                }
            },
            "author" => match &value {
                Value::Array(items) => {
                    match items.iter().map(text_of).collect::<Option<Vec<_>>>() {
                        Some(names) => Some(Value::String(names.join(", "))),
                        None => {
                            invalid(diagnostics, "a string or a list of strings");
                            None
                        }
                    }
                }
                other => match text_of(other) {
                    Some(text) => Some(Value::String(text)),
                    None => {
                        invalid(diagnostics, "a string or a list of strings");
                        None
                    }
                },
            },
            "lang" => match text_of(&value).filter(|tag| is_language_tag(tag)) {
                Some(tag) => Some(Value::String(tag)),
                None => {
                    invalid(
                        diagnostics,
                        "a BCP 47 language tag, such as `en` or `de-CH`",
                    );
                    None
                }
            },
            "spec" => match text_of(&value) {
                Some(version) => {
                    if version != SPEC_VERSION {
                        let d = Diagnostic::new(
                            "spec_version_unsupported",
                            format!(
                                "The document targets spec {version}; it is processed under spec {SPEC_VERSION}."
                            ),
                        );
                        diagnostics.push(locate(&key, d));
                    }
                    Some(Value::String(version))
                }
                None => {
                    invalid(diagnostics, "a version string, such as \"0.2\"");
                    None
                }
            },
            _ => Some(value),
        };
        if let Some(value) = normalized {
            meta.insert(key, value);
        }
    }
    meta
}

/// A string-typed key's value: strings as-is; numbers and booleans as their shortest text.
fn text_of(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Bool(b) => Some(b.to_string()),
        Value::Number(n) => Some(match (n.as_i64(), n.as_f64()) {
            (Some(i), _) => i.to_string(),
            (None, Some(f)) => f.to_string(),
            _ => n.to_string(),
        }),
        _ => None,
    }
}

/// `[A-Za-z]{2,3}(-[A-Za-z0-9]{1,8})*`
fn is_language_tag(tag: &str) -> bool {
    let mut parts = tag.split('-');
    let primary = parts.next().unwrap_or("");
    (2..=3).contains(&primary.len())
        && primary.bytes().all(|b| b.is_ascii_alphabetic())
        && parts.all(|p| (1..=8).contains(&p.len()) && p.bytes().all(|b| b.is_ascii_alphanumeric()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(yaml: &str) -> (Map<String, Value>, Vec<Diagnostic>) {
        let lines: Vec<String> = format!("---\n{yaml}\n---")
            .lines()
            .map(String::from)
            .collect();
        let mut diagnostics = Sink::new();
        let (meta, _) = parse(&lines, range(&lines).unwrap(), &mut diagnostics);
        (meta, diagnostics.into_vec())
    }

    #[test]
    fn rejects_aliases_tags_merge_keys_and_duplicates() {
        for yaml in [
            "a: &x 1\nb: *x",
            "a: !!str 1",
            "<<: {a: 1}",
            "a: 1\na: 2",
            "- 1\n- 2",
            "a: .nan",
            "a: 9007199254740993",
        ] {
            let (meta, diagnostics) = meta(yaml);
            assert!(meta.is_empty(), "{yaml}");
            assert_eq!(diagnostics[0].code, "frontmatter_invalid", "{yaml}");
            assert_eq!(diagnostics[0].line, Some(1));
        }
    }

    #[test]
    fn resolves_core_schema_scalars() {
        let (meta, diagnostics) =
            meta("i: 42\nf: 0.5\nb: true\nn: ~\ns: \"42\"\nx: 0x1F\nlist: [1, two]");
        assert!(diagnostics.is_empty());
        assert_eq!(
            serde_json::to_string(&meta).unwrap(),
            r#"{"i":42,"f":0.5,"b":true,"n":null,"s":"42","x":31,"list":[1,"two"]}"#
        );
    }

    #[test]
    fn reports_key_lines() {
        let (_, diagnostics) = meta("title: ok\nlang: not a tag!");
        assert_eq!(diagnostics[0].code, "meta_value_invalid");
        assert_eq!(diagnostics[0].line, Some(3));
    }
}
