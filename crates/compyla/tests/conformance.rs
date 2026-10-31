//! The conformance suite (spec/conformance/README.md), run against this implementation.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

fn suite(name: &str) -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../spec/conformance")
        .join(name);
    let mut fixtures: Vec<PathBuf> = fs::read_dir(root)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.join("case.json").is_file())
        .collect();
    fixtures.sort();
    fixtures
}

fn read_json(path: &Path) -> Value {
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

/// markdown-spec.md §11.3: in order, duplicates kept; `line` always compared; `column` only when
/// the expected diagnostic has one.
fn diagnostics_match(actual: &[compyla::Diagnostic], expected: &Value) -> bool {
    let expected = expected.as_array().unwrap();
    actual.len() == expected.len()
        && actual.iter().zip(expected).all(|(a, e)| {
            let severity = serde_json::to_value(a.severity).unwrap();
            a.code == e["code"].as_str().unwrap()
                && severity == e["severity"]
                && a.line.map(|l| l as u64) == e["line"].as_u64()
                && (e["column"].is_null() || a.column.map(|c| c as u64) == e["column"].as_u64())
        })
}

fn summary(actual: &[compyla::Diagnostic]) -> String {
    actual
        .iter()
        .map(|d| format!("{}@{:?}:{:?}", d.code, d.line, d.column))
        .collect::<Vec<_>>()
        .join(", ")
}

#[test]
fn parse_suite() {
    let fixtures = suite("parse");
    assert!(
        fixtures.len() >= 44,
        "expected the full parse suite, found {}",
        fixtures.len()
    );
    let mut failures = Vec::new();
    for fixture in fixtures {
        let name = fixture.file_name().unwrap().to_string_lossy().to_string();
        let input = fs::read_to_string(fixture.join("input.md")).unwrap();
        let expected = read_json(&fixture.join("expected.json"));
        let result = compyla::parse(&input);
        let ir = serde_json::to_value(&result.ir).unwrap();
        if ir != expected["ir"] {
            failures.push(format!(
                "{name}: IR differs\n  got  {ir}\n  want {}",
                expected["ir"]
            ));
        }
        if !diagnostics_match(&result.diagnostics, &expected["diagnostics"]) {
            failures.push(format!(
                "{name}: diagnostics differ\n  got  [{}]\n  want {}",
                summary(&result.diagnostics),
                expected["diagnostics"]
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} parse fixture failure(s):\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// The asset resolver a case describes (spec/conformance/README.md, "Running a case").
fn assets_for(fixture: &Path, case: &Value) -> impl Fn(&str) -> compyla::Asset {
    let listed: Option<Vec<String>> = case["assets"]
        .as_array()
        .map(|a| a.iter().map(|s| s.as_str().unwrap().to_string()).collect());
    let dir = fixture.to_path_buf();
    move |src: &str| match &listed {
        None => compyla::Asset::Exists,
        Some(listed) if listed.iter().any(|s| s == src) => match fs::read(dir.join(src)) {
            Ok(bytes) => compyla::Asset::Bytes(bytes),
            Err(_) => compyla::Asset::Exists,
        },
        Some(_) => compyla::Asset::NotFound,
    }
}

#[test]
fn lint_suite() {
    let fixtures = suite("lint");
    assert!(
        fixtures.len() >= 43,
        "expected the full lint suite, found {}",
        fixtures.len()
    );
    let mut failures = Vec::new();
    for fixture in fixtures {
        let name = fixture.file_name().unwrap().to_string_lossy().to_string();
        let case = read_json(&fixture.join("case.json"));
        let input = fs::read_to_string(fixture.join("input.md")).unwrap();
        let expected = read_json(&fixture.join("expected.json"));
        let options = compyla::Options {
            format: compyla::Format::parse(case["format"].as_str().unwrap()),
            strict: case["strict"].as_bool().unwrap_or(false),
        };
        let diagnostics = compyla::lint(&input, &options, &assets_for(&fixture, &case)).unwrap();
        if !diagnostics_match(&diagnostics, &expected["diagnostics"]) {
            failures.push(format!(
                "{name}: got [{}], want {}",
                summary(&diagnostics),
                expected["diagnostics"]
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} lint fixture failure(s):\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// Removes every `position`, as IR from another producer may lack them (§10.7).
fn strip_positions(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.remove("position");
            map.values_mut().for_each(strip_positions);
        }
        Value::Array(items) => items.iter_mut().for_each(strip_positions),
        _ => {}
    }
}

/// markdown-spec.md §10.7, §11.3: the IR of each lint fixture, without positions, gets the same
/// validate-stage diagnostics as its markdown, each with no line and, unless it is document-level,
/// a path to its node. Paths on markdown diagnostics resolve in the same IR.
#[test]
fn lint_ir_agrees_with_lint() {
    let registry =
        read_json(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../spec/diagnostics.json"));
    let validate_stage: Vec<&str> = registry["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|d| d["stage"] == "validate")
        .map(|d| d["code"].as_str().unwrap())
        .collect();
    // Document-level codes concern no node.
    let document_level: Vec<&str> = registry["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|d| d["position"] == "none")
        .map(|d| d["code"].as_str().unwrap())
        .collect();
    let profile = |diagnostics: Vec<compyla::Diagnostic>| -> Vec<compyla::Diagnostic> {
        diagnostics
            .into_iter()
            .filter(|d| validate_stage.contains(&d.code.as_str()))
            .collect()
    };
    let mut failures = Vec::new();
    for fixture in suite("lint") {
        let name = fixture.file_name().unwrap().to_string_lossy().to_string();
        let case = read_json(&fixture.join("case.json"));
        let input = fs::read_to_string(fixture.join("input.md")).unwrap();
        let options = compyla::Options {
            format: compyla::Format::parse(case["format"].as_str().unwrap()),
            strict: case["strict"].as_bool().unwrap_or(false),
        };
        let assets = assets_for(&fixture, &case);
        let mut ir = serde_json::to_value(compyla::parse(&input).ir).unwrap();
        strip_positions(&mut ir);

        let linted = compyla::lint(&input, &options, &assets).unwrap();
        // A limit stops processing, and the parsed IR lacks what the limit cut: not the same input.
        if linted.iter().any(|d| d.code == "limit_exceeded") {
            continue;
        }
        let from_markdown = profile(linted);
        let from_ir = profile(compyla::lint_ir(&ir.to_string(), &options, &assets).unwrap());
        let codes = |diagnostics: &[compyla::Diagnostic]| {
            let mut codes: Vec<String> = diagnostics.iter().map(|d| d.code.clone()).collect();
            codes.sort();
            codes
        };
        if codes(&from_markdown) != codes(&from_ir) {
            failures.push(format!(
                "{name}: markdown [{}], IR [{}]",
                summary(&from_markdown),
                summary(&from_ir)
            ));
        }
        for d in &from_ir {
            let resolves = match d.path.as_deref() {
                Some(path) => ir.pointer(path).is_some(),
                None => document_level.contains(&d.code.as_str()),
            };
            if d.line.is_some() || !resolves {
                failures.push(format!(
                    "{name}: IR {} at {:?}, path {:?}",
                    d.code, d.line, d.path
                ));
            }
        }
        for d in &from_markdown {
            if let Some(path) = &d.path
                && ir.pointer(path).is_none()
            {
                failures.push(format!(
                    "{name}: markdown {} path {path} doesn't resolve",
                    d.code
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} IR agreement failure(s):\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn compose_suite() {
    let fixtures = suite("compose");
    assert!(
        fixtures.len() >= 14,
        "expected the full compose suite, found {}",
        fixtures.len()
    );
    let mut failures = Vec::new();
    for fixture in fixtures {
        let name = fixture.file_name().unwrap().to_string_lossy().to_string();
        let case = read_json(&fixture.join("case.json"));
        let input = fs::read_to_string(fixture.join("input.md")).unwrap();
        let expected = read_json(&fixture.join("expected.json"));
        let format = compyla::Format::parse(case["format"].as_str().unwrap()).unwrap();
        let summary = compyla::compose_summary(&input, format).unwrap();
        if summary != expected {
            failures.push(format!("{name}:\n  got  {summary}\n  want {expected}"));
        }
    }
    assert!(
        failures.is_empty(),
        "{} compose fixture failure(s):\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// The render suite (spec/conformance/README.md, "Render suite"). Each fixture this build can render
/// is converted twice, and the bytes must be identical (markdown-spec.md §13). The diagnostics are
/// matched as in the lint suite, and the output is checked by the reference extractor,
/// `spec/tools/render_check.py`, run through `uv` with its pinned dependencies.
#[test]
fn render_suite() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let scratch = std::env::temp_dir().join(format!("compyla-render-{}", std::process::id()));
    fs::create_dir_all(&scratch).unwrap();
    let (mut rendered, mut pending, mut failures) = (0, Vec::new(), Vec::new());
    for fixture in suite("render") {
        let name = fixture.file_name().unwrap().to_string_lossy().to_string();
        let case = read_json(&fixture.join("case.json"));
        let format_name = case["format"].as_str().unwrap();
        let format = compyla::Format::parse(format_name).unwrap();
        if !compyla::renderer_available(format) {
            pending.push(name);
            continue;
        }
        let input = fs::read_to_string(fixture.join("input.md")).unwrap();
        let expected = read_json(&fixture.join("expected.json"));
        let options = compyla::Options {
            format: Some(format),
            strict: case["strict"].as_bool().unwrap_or(false),
        };
        let assets = assets_for(&fixture, &case);
        let first = compyla::convert(&input, &options, &assets).unwrap();
        let second = compyla::convert(&input, &options, &assets).unwrap();
        if !diagnostics_match(&first.diagnostics, &expected["diagnostics"]) {
            failures.push(format!(
                "{name}: diagnostics [{}], want {}",
                summary(&first.diagnostics),
                expected["diagnostics"]
            ));
            continue;
        }
        let Some(bytes) = first.bytes else {
            failures.push(format!("{name}: no output"));
            continue;
        };
        if Some(&bytes) != second.bytes.as_ref() {
            failures.push(format!("{name}: two renders differ"));
        }
        let output = scratch.join(format!("{name}.{format_name}"));
        fs::write(&output, &bytes).unwrap();
        let check = std::process::Command::new("uv")
            .current_dir(&root)
            .args([
                "run",
                "--no-project",
                "--with",
                "lxml==6.0.2",
                "--with",
                "pypdf==6.1.1",
                "python",
                "-I",
                "spec/tools/render_check.py",
                "check",
                format_name,
            ])
            .arg(fixture.join("expected.json"))
            .arg(&output)
            .output()
            .expect("the render suite needs uv (https://docs.astral.sh/uv/) on PATH");
        if !check.status.success() {
            failures.push(format!(
                "{name}: {}{}",
                String::from_utf8_lossy(&check.stdout),
                String::from_utf8_lossy(&check.stderr)
            ));
        }
        rendered += 1;
    }
    let _ = fs::remove_dir_all(&scratch);
    eprintln!("render suite: {rendered} rendered; renderer pending for {pending:?}");
    if cfg!(feature = "xlsx") {
        assert!(
            rendered >= 5,
            "expected the xlsx render fixtures, rendered {rendered}"
        );
    }
    assert!(
        failures.is_empty(),
        "{} render fixture failure(s):\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// The inspect suite: each template's report equals `expected.json`, with issues compared as
/// `(code, severity)` in order (spec/conformance/README.md).
#[test]
fn inspect_suite() {
    let fixtures = suite("inspect");
    assert!(
        fixtures.len() >= 10,
        "expected the full inspect suite, found {}",
        fixtures.len()
    );
    let issues = |report: &Value| -> Vec<(String, String)> {
        report["issues"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| (i["code"].to_string(), i["severity"].to_string()))
            .collect()
    };
    let mut failures = Vec::new();
    for fixture in fixtures {
        let name = fixture.file_name().unwrap().to_string_lossy().to_string();
        let case = read_json(&fixture.join("case.json"));
        let bytes = fs::read(fixture.join(case["template"].as_str().unwrap())).unwrap();
        let mut report = compyla::inspect_template(&bytes);
        let mut expected = read_json(&fixture.join("expected.json"));
        if issues(&report) != issues(&expected) {
            failures.push(format!(
                "{name}: issues {:?}, want {:?}",
                issues(&report),
                issues(&expected)
            ));
            continue;
        }
        report.as_object_mut().unwrap().remove("issues");
        expected.as_object_mut().unwrap().remove("issues");
        if report != expected {
            failures.push(format!("{name}:\n  got  {report}\n  want {expected}"));
        }
    }
    assert!(
        failures.is_empty(),
        "{} inspect fixture failure(s):\n{}",
        failures.len(),
        failures.join("\n")
    );
}
