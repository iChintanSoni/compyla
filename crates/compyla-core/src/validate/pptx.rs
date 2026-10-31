//! The pptx profile (markdown-spec.md §8).

use serde_json::Value;

use super::Input;
use crate::diagnostic::Diagnostic;
use crate::ir::Document;

pub fn validate(_input: &Input<'_>, _out: &mut Vec<Diagnostic>) {}

/// The compose summary (spec/conformance/compose.schema.json): slides with their titles, resolved
/// layouts, notes, and content block types.
pub fn summary(_document: &Document) -> Value {
    serde_json::json!({ "format": "pptx", "slides": [] })
}
