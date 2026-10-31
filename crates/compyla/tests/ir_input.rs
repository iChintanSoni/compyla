//! IR input (markdown-spec.md §10.7).

use std::fs;
use std::path::Path;

use serde_json::{Value, json};

fn options() -> compyla::Options {
    compyla::Options {
        format: Some(compyla::Format::Docx),
        strict: false,
        ..Default::default()
    }
}

fn refusal(ir: Value) -> compyla::OperationError {
    compyla::lint_ir(&ir.to_string(), &options(), &compyla::no_assets).unwrap_err()
}

fn doc(body: Value) -> Value {
    json!({ "irVersion": "0.1", "meta": {}, "body": body })
}

#[test]
fn every_parse_fixtures_ir_is_valid_input() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../spec/conformance/parse");
    for entry in fs::read_dir(root).unwrap() {
        let fixture = entry.unwrap().path();
        if !fixture.join("expected.json").is_file() {
            continue;
        }
        let expected: Value =
            serde_json::from_str(&fs::read_to_string(fixture.join("expected.json")).unwrap())
                .unwrap();
        let ir = expected["ir"].to_string();
        let (document, _) =
            compyla::load_ir(&ir).unwrap_or_else(|e| panic!("{}: {e:?}", fixture.display()));
        assert_eq!(
            serde_json::to_value(&document).unwrap(),
            expected["ir"],
            "{}",
            fixture.display()
        );
    }
}

#[test]
fn malformed_ir_is_refused_with_the_pointer_of_the_first_failure() {
    let cases = [
        (
            doc(json!([{ "type": "paragraph", "runs": [], "extra": 1 }])),
            "/body/0/extra",
        ),
        (
            doc(json!([{ "type": "section", "level": 7, "title": [], "children": [] }])),
            "/body/0/level",
        ),
        (
            doc(json!([{ "type": "paragraph", "runs": [{ "text": "x", "bold": false }] }])),
            "/body/0/runs/0/bold",
        ),
        (
            doc(json!([{ "type": "directive", "name": "Notes", "args": "", "children": [] }])),
            "/body/0/name",
        ),
        (
            doc(
                json!([{ "type": "table", "header": [{ "runs": [], "raw": "a" }], "align": [], "rows": [] }]),
            ),
            "/body/0/align",
        ),
        (
            doc(json!([{ "type": "list", "ordered": false, "start": 3, "items": [] }])),
            "/body/0/start",
        ),
        (
            doc(
                json!([{ "type": "section", "level": 2, "title": [], "children": [{ "type": "section", "level": 1, "title": [], "children": [] }] }]),
            ),
            "/body/0/children/0/level",
        ),
        (
            doc(
                json!([{ "type": "paragraph", "runs": [], "position": { "line": 0, "column": 1 } }]),
            ),
            "/body/0/position/line",
        ),
        (
            doc(
                json!([{ "type": "paragraph", "runs": [], "position": { "line": 4_294_967_296u64, "column": 1 } }]),
            ),
            "/body/0/position/line",
        ),
        (doc(json!([{ "type": "chart" }])), "/body/0/type"),
    ];
    for (ir, pointer) in cases {
        let error = refusal(ir.clone());
        assert_eq!(error.code, "ir_invalid", "{ir}");
        assert_eq!(
            error.pointer.as_deref(),
            Some(pointer),
            "{ir}: {}",
            error.message
        );
    }
}

#[test]
fn ir_versions_are_compatible_within_a_major_version() {
    for (version, accepted) in [
        ("0.0", true),
        ("0.1", true),
        ("0.2", false),
        ("1.0", false),
        ("x", false),
        ("00.1", false),
        ("+0.+1", false),
        ("0.01", false),
    ] {
        let ir = json!({ "irVersion": version, "meta": {}, "body": [] }).to_string();
        match compyla::lint_ir(&ir, &options(), &compyla::no_assets) {
            Ok(_) => assert!(accepted, "{version} should be refused"),
            Err(e) => {
                assert!(!accepted, "{version} should be accepted");
                assert_eq!(e.code, "ir_version_unsupported");
            }
        }
    }
}

#[test]
fn ir_input_gets_the_text_rules_with_paths() {
    let ir = json!({
        "irVersion": "0.1",
        "meta": { "format": "PPTX", "lang": "not a tag!" },
        "body": [{ "type": "paragraph", "runs": [
            { "text": "a\u{000B}b" }, { "text": "c" },
            { "text": "bad", "link": "javascript:alert(1)" }
        ] }]
    });
    let (document, diagnostics) = compyla::load_ir(&ir.to_string()).unwrap();
    let paths: Vec<(String, Option<String>)> = diagnostics
        .iter()
        .map(|d| (d.code.clone(), d.path.clone()))
        .collect();
    assert!(
        paths.contains(&(
            "invalid_character".into(),
            Some("/body/0/runs/0/text".into())
        )),
        "{paths:?}"
    );
    assert!(
        paths.contains(&(
            "link_scheme_blocked".into(),
            Some("/body/0/runs/2/link".into())
        )),
        "{paths:?}"
    );
    assert!(
        paths.contains(&("meta_value_invalid".into(), Some("/meta/lang".into()))),
        "{paths:?}"
    );
    assert_eq!(document.meta["format"], "pptx");
    let compyla::ir::Block::Paragraph(p) = &document.body[0] else {
        panic!()
    };
    assert_eq!(p.runs.len(), 1, "runs are canonical: {:?}", p.runs);
    assert_eq!(p.runs[0].text, "a\u{FFFD}bcbad");
}

fn lint_ir(ir: &Value, format: compyla::Format) -> Vec<compyla::Diagnostic> {
    let options = compyla::Options {
        format: Some(format),
        strict: false,
        ..Default::default()
    };
    compyla::lint_ir(&ir.to_string(), &options, &compyla::no_assets).unwrap()
}

fn cell(text: &str) -> Value {
    json!({ "runs": [{ "text": text }], "raw": text })
}

#[test]
fn limits_hold_for_ir_input() {
    // 33 nested quotes: the innermost is past the limit.
    let mut deep = json!({ "type": "paragraph", "runs": [{ "text": "x" }] });
    for _ in 0..33 {
        deep = json!({ "type": "quote", "children": [deep] });
    }
    let wide = json!({
        "type": "table",
        "header": vec![cell("a"); 16_385],
        "align": vec![Value::Null; 16_385],
        "rows": [],
        "position": { "line": 7, "column": 1 }
    });
    let mut nested_path = "/body/0".to_string();
    for _ in 0..32 {
        nested_path.push_str("/children/0");
    }
    for (body, path, line) in [
        (json!([deep]), nested_path.as_str(), None),
        (json!([wide]), "/body/0", Some(7)),
    ] {
        let diagnostics = lint_ir(&doc(body), compyla::Format::Xlsx);
        let found: Vec<_> = diagnostics
            .iter()
            .map(|d| (d.code.as_str(), d.path.as_deref(), d.line))
            .collect();
        // Nothing is processed after the limit: no validate-stage diagnostics.
        assert_eq!(found, vec![("limit_exceeded", Some(path), line)]);
    }
}

#[test]
fn text_rules_cover_every_string_and_keep_lines() {
    let ir = json!({
        "irVersion": "0.1",
        "meta": { "title": "T\u{1}", "keywords": ["a\u{2}"] },
        "body": [
            { "type": "paragraph", "position": { "line": 5, "column": 1 }, "runs": [
                { "text": "x", "link": "https://example.com/\u{3}" },
                { "text": "y", "link": "javascript:alert(1)" }
            ] },
            { "type": "code", "lang": "rs\u{4}", "value": "" },
            { "type": "media", "src": "a\u{5}.png", "alt": "" }
        ]
    });
    let (document, diagnostics) = compyla::load_ir(&ir.to_string()).unwrap();
    let found: Vec<_> = diagnostics
        .iter()
        .map(|d| (d.code.as_str(), d.path.as_deref(), d.line))
        .collect();
    for expected in [
        ("invalid_character", Some("/meta/title"), None),
        ("invalid_character", Some("/meta/keywords/0"), None),
        ("invalid_character", Some("/body/0/runs/0/link"), Some(5)),
        ("link_scheme_blocked", Some("/body/0/runs/1/link"), Some(5)),
        ("invalid_character", Some("/body/1/lang"), None),
        ("invalid_character", Some("/body/2/src"), None),
    ] {
        assert!(found.contains(&expected), "{expected:?} in {found:?}");
    }
    assert_eq!(document.meta["title"], "T\u{FFFD}");
}
