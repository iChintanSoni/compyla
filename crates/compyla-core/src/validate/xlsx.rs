//! The xlsx profile (markdown-spec.md §9).

use serde_json::Value;

use super::Input;
use crate::diagnostic::Diagnostic;
use crate::ir::Document;

pub fn validate(_input: &Input<'_>, _out: &mut Vec<Diagnostic>) {}

/// The compose summary (spec/conformance/compose.schema.json): sheets with their placed ranges,
/// column types and translated formulas.
pub fn summary(_document: &Document) -> Value {
    serde_json::json!({ "format": "xlsx", "sheets": [] })
}
