//! The Compyla facade (architecture.md D8): the operations every host exposes, with the uniform
//! diagnostics shape and ordering of markdown-spec.md §11. Hosts call this crate, never
//! `compyla-core` directly.

pub use compyla_core::diagnostic::{CodeInfo, Diagnostic, Severity, code_info};
pub use compyla_core::ir;
pub use compyla_core::{Asset, Format};

/// Diagnostics beyond this are dropped, ending with `diagnostics_truncated` (§11.2).
const DIAGNOSTICS_CAP: usize = 1000;

/// The result of `parse` (markdown-spec.md §10.1).
#[derive(Debug, Clone, serde::Serialize)]
pub struct ParseResult {
    pub ir: ir::Document,
    pub diagnostics: Vec<Diagnostic>,
}

/// `parse`: markdown to IR plus parse-stage diagnostics, in §11.2 order.
pub fn parse(source: &str) -> ParseResult {
    let compyla_core::ParseOutput {
        document,
        mut diagnostics,
        ..
    } = compyla_core::parse(source);
    finish(&mut diagnostics, false);
    ParseResult {
        ir: document,
        diagnostics,
    }
}

/// Options shared by `lint` and `convert` (markdown-spec.md §10.2).
#[derive(Debug, Clone, Default)]
pub struct Options {
    pub format: Option<Format>,
    pub strict: bool,
}

/// A failure of the call itself, not of the document (markdown-spec.md §10.6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationError {
    pub code: &'static str,
    pub message: String,
    /// For `ir_invalid` and `ir_version_unsupported`: the JSON Pointer of the first failure.
    pub pointer: Option<String>,
}

/// `lint`: parse and validate diagnostics for markdown, in §11.2 order. `assets` is the host's asset
/// resolver (§10.3); hosts without one pass [`no_assets`].
pub fn lint(
    source: &str,
    options: &Options,
    assets: &dyn Fn(&str) -> Asset,
) -> Result<Vec<Diagnostic>, OperationError> {
    let compyla_core::ParseOutput {
        document,
        mut diagnostics,
        sidecar,
    } = compyla_core::parse(source);
    let format = resolve_format(options, &document)?;
    diagnostics.extend(validate(document, &sidecar, format, assets));
    finish(&mut diagnostics, options.strict);
    Ok(diagnostics)
}

/// The compose summary (spec/conformance/compose.schema.json) of a pptx or xlsx document: the
/// normative projection of slide segmentation, layout selection, xlsx placement and formula
/// translation that the compose suite checks.
pub fn compose_summary(source: &str, format: Format) -> Option<serde_json::Value> {
    let document = compyla_core::parse(source).document;
    match format {
        Format::Pptx => Some(compyla_core::validate::pptx::summary(&document)),
        Format::Xlsx => Some(compyla_core::validate::xlsx::summary(&document)),
        Format::Docx | Format::Pdf => None,
    }
}

/// Loads IR JSON submitted in place of markdown (markdown-spec.md §10.7): structure, version and
/// invariants are checked, then the text rules applied. Returns the IR and its diagnostics.
pub fn load_ir(json: &str) -> Result<(ir::Document, Vec<Diagnostic>), OperationError> {
    compyla_core::ir_input::load(json).map_err(|r| OperationError {
        code: r.code,
        message: if r.pointer.is_empty() {
            r.message
        } else {
            format!("{} (at {})", r.message, r.pointer)
        },
        pointer: Some(r.pointer),
    })
}

/// `lint` for IR input (§10.7). Its diagnostics carry IR paths, and lines only where the IR has
/// positions.
pub fn lint_ir(
    json: &str,
    options: &Options,
    assets: &dyn Fn(&str) -> Asset,
) -> Result<Vec<Diagnostic>, OperationError> {
    let (document, mut diagnostics) = load_ir(json)?;
    let format = resolve_format(options, &document)?;
    let sidecar = compyla_core::Sidecar::default();
    diagnostics.extend(validate(document, &sidecar, format, assets));
    finish(&mut diagnostics, options.strict);
    Ok(diagnostics)
}

/// The profile's diagnostics, each with the IR path of its node and its original position, if any
/// (§10.7, §11.3).
fn validate(
    mut document: ir::Document,
    sidecar: &compyla_core::Sidecar,
    format: Format,
    assets: &dyn Fn(&str) -> Asset,
) -> Vec<Diagnostic> {
    let (locator, sidecar) = compyla_core::locate::renumber(&mut document, sidecar);
    let input = compyla_core::validate::Input {
        document: &document,
        sidecar: &sidecar,
        assets,
    };
    let mut diagnostics = compyla_core::validate::validate(&input, format);
    locator.locate(&mut diagnostics);
    diagnostics
}

/// The resolver of a host with none: in `lint`, every image exists without bytes (§10.3).
pub fn no_assets(_src: &str) -> Asset {
    Asset::Exists
}

/// §10.2: the option, then `meta.format`, else `format_missing`.
fn resolve_format(options: &Options, document: &ir::Document) -> Result<Format, OperationError> {
    if let Some(format) = options.format {
        return Ok(format);
    }
    document
        .meta
        .get("format")
        .and_then(|f| f.as_str())
        .and_then(Format::parse)
        .ok_or_else(|| OperationError {
            code: "format_missing",
            message: "No format was given, and the document's frontmatter has no `format`.".into(),
            pointer: None,
        })
}

/// Strict escalation (§11.1), order (§11.2) and the cap.
fn finish(diagnostics: &mut Vec<Diagnostic>, strict: bool) {
    compyla_core::diagnostic::escalate(diagnostics, strict);
    compyla_core::diagnostic::sort(diagnostics);
    if diagnostics.len() > DIAGNOSTICS_CAP {
        diagnostics.truncate(DIAGNOSTICS_CAP);
        diagnostics.push(Diagnostic::new(
            "diagnostics_truncated",
            format!("Only the first {DIAGNOSTICS_CAP} diagnostics are reported."),
        ));
    }
}
