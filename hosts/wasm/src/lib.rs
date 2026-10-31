//! The core-only browser build (architecture.md D7): `parse` and `lint`, with no renderers. JSON in
//! and out, so the JavaScript side needs no generated types.

use wasm_bindgen::prelude::*;

/// `parse(source)` → `{ ir, diagnostics }` as JSON (markdown-spec.md §10.1).
#[wasm_bindgen]
pub fn parse(source: &str) -> String {
    serde_json::to_string(&compyla::parse(source)).expect("parse results serialize")
}

/// `lint(source, format, strict)` → `{ diagnostics }` or `{ error: { code, message } }` as JSON.
/// `format` may be empty to use the frontmatter's `format`. Images can't be fetched in the core-only
/// build, so every image counts as existing (§10.3).
#[wasm_bindgen]
pub fn lint(source: &str, format: &str, strict: bool) -> String {
    let format = if format.is_empty() {
        None
    } else {
        match compyla::Format::parse(format) {
            Some(format) => Some(format),
            None => {
                return error(
                    "format_unknown",
                    &format!("`{format}` isn't docx, pdf, pptx or xlsx"),
                );
            }
        }
    };
    let options = compyla::Options { format, strict };
    match compyla::lint(source, &options, &compyla::no_assets) {
        Ok(diagnostics) => serde_json::json!({ "diagnostics": diagnostics }).to_string(),
        Err(e) => error(e.code, &e.message),
    }
}

fn error(code: &str, message: &str) -> String {
    serde_json::json!({ "error": { "code": code, "message": message } }).to_string()
}
