//! The Rust types agree with the spec's schemas (roadmap Phase B), proven on the whole fixture corpus:
//! every expected IR round-trips through the Rust types unchanged (no field lost or invented), and
//! every IR and diagnostic this implementation produces validates against the spec's schemas.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

fn spec(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../spec")
        .join(path)
}

fn json(path: &Path) -> Value {
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

fn parse_fixtures() -> Vec<PathBuf> {
    let mut fixtures: Vec<PathBuf> = fs::read_dir(spec("conformance/parse"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.join("case.json").is_file())
        .collect();
    fixtures.sort();
    fixtures
}

#[test]
fn expected_ir_round_trips_through_the_rust_types() {
    for fixture in parse_fixtures() {
        let expected = json(&fixture.join("expected.json"))["ir"].clone();
        let document: compyla::ir::Document = serde_json::from_value(expected.clone())
            .unwrap_or_else(|e| {
                panic!(
                    "{}: expected IR doesn't fit the Rust types: {e}",
                    fixture.display()
                )
            });
        assert_eq!(
            serde_json::to_value(&document).unwrap(),
            expected,
            "{}: IR changed in a round trip",
            fixture.display()
        );
    }
}

#[test]
fn produced_ir_and_diagnostics_validate_against_the_spec_schemas() {
    let ir_schema = jsonschema::validator_for(&json(&spec("ir.schema.json"))).unwrap();
    let diagnostic_schema =
        jsonschema::validator_for(&json(&spec("diagnostic.schema.json"))).unwrap();
    for fixture in parse_fixtures() {
        let result = compyla::parse(&fs::read_to_string(fixture.join("input.md")).unwrap());
        let ir = serde_json::to_value(&result.ir).unwrap();
        if let Err(error) = ir_schema.validate(&ir) {
            panic!(
                "{}: produced IR violates spec/ir.schema.json: {error}",
                fixture.display()
            );
        }
        for diagnostic in &result.diagnostics {
            let value = serde_json::to_value(diagnostic).unwrap();
            if let Err(error) = diagnostic_schema.validate(&value) {
                panic!(
                    "{}: diagnostic {value} violates spec/diagnostic.schema.json: {error}",
                    fixture.display()
                );
            }
        }
    }
}

#[test]
fn every_registered_code_is_known_to_the_core() {
    for entry in json(&spec("diagnostics.json"))["diagnostics"]
        .as_array()
        .unwrap()
    {
        let code = entry["code"].as_str().unwrap();
        let info = compyla_core_code_info(code);
        assert_eq!(
            serde_json::to_value(info.0).unwrap(),
            entry["severity"],
            "{code}"
        );
        assert_eq!(
            info.1,
            entry["strictPromotable"].as_bool().unwrap(),
            "{code}"
        );
    }
}

fn compyla_core_code_info(code: &str) -> (compyla::Severity, bool) {
    let d = compyla::Diagnostic::new(code, "");
    (d.severity, compyla::code_info(code).strict_promotable)
}
