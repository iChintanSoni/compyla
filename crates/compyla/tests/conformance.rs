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
        fixtures.len() >= 35,
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

#[test]
fn compose_suite() {
    let fixtures = suite("compose");
    assert!(
        fixtures.len() >= 11,
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
