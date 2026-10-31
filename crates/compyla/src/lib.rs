//! The Compyla facade (architecture.md D8): the operations every host exposes, with the uniform
//! diagnostics shape and ordering of markdown-spec.md §11. Hosts call this crate, never
//! `compyla-core` directly.

use compyla_core::diagnostic::{Blocking, Sink};
use compyla_core::validate::Composed;

pub use compyla_core::diagnostic::{CodeInfo, Diagnostic, Severity, code_info};
pub use compyla_core::ir;
pub use compyla_core::{Asset, Format};

/// Diagnostics beyond this are dropped, ending with `diagnostics_truncated` (§11.2).
const DIAGNOSTICS_CAP: usize = compyla_core::diagnostic::CAP;

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

/// The result of `convert` (markdown-spec.md §10.1).
#[derive(Debug, Clone)]
pub struct ConvertResult {
    /// The output file's bytes; absent when a diagnostic is an error after escalation (§11.1).
    pub bytes: Option<Vec<u8>>,
    /// The format rendered: the option's, or the frontmatter's (§10.2). Absent when a limit stopped
    /// processing before the format was resolved.
    pub format: Option<Format>,
    pub diagnostics: Vec<Diagnostic>,
}

/// `lint`: parse and validate diagnostics for markdown, in §11.2 order. `assets` is the host's asset
/// resolver (§10.3); hosts without one pass [`no_assets`].
pub fn lint(
    source: &str,
    options: &Options,
    assets: &dyn Fn(&str) -> Asset,
) -> Result<Vec<Diagnostic>, OperationError> {
    Ok(check(from_markdown(source), options, assets, false)?.diagnostics)
}

/// `lint` for IR input (§10.7). Its diagnostics carry IR paths, and lines only where the IR has
/// positions.
pub fn lint_ir(
    json: &str,
    options: &Options,
    assets: &dyn Fn(&str) -> Asset,
) -> Result<Vec<Diagnostic>, OperationError> {
    Ok(check(from_ir(json)?, options, assets, false)?.diagnostics)
}

/// `convert`: markdown to the output format, with diagnostics from every stage.
pub fn convert(
    source: &str,
    options: &Options,
    assets: &dyn Fn(&str) -> Asset,
) -> Result<ConvertResult, OperationError> {
    let assets = converting(assets);
    render(
        check(from_markdown(source), options, &assets, true)?,
        &assets,
    )
}

/// `convert` for IR input (§10.7).
pub fn convert_ir(
    json: &str,
    options: &Options,
    assets: &dyn Fn(&str) -> Asset,
) -> Result<ConvertResult, OperationError> {
    let assets = converting(assets);
    render(check(from_ir(json)?, options, &assets, true)?, &assets)
}

/// The asset resolver as `convert` sees it (§10.3): an image that exists without bytes can't be
/// written, so it is unresolved.
fn converting(assets: &dyn Fn(&str) -> Asset) -> impl Fn(&str) -> Asset + '_ {
    move |src| match assets(src) {
        Asset::Exists => Asset::NotFound,
        resolved => resolved,
    }
}

/// Whether this build can render `format` (`renderer_unavailable`, §10.6).
pub fn renderer_available(format: Format) -> bool {
    match format {
        Format::Xlsx => cfg!(feature = "xlsx"),
        Format::Pptx => cfg!(feature = "pptx"),
        Format::Docx | Format::Pdf => false,
    }
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
    let loaded = load(json)?;
    Ok((loaded.document, loaded.diagnostics))
}

/// The largest template package accepted (markdown-spec.md §12.1). Hosts need read no more than one
/// byte past it.
pub const TEMPLATE_SIZE_LIMIT: usize = compyla_ooxml::read::MAX_COMPRESSED;

/// `inspect_template` (markdown-spec.md §10.5): the template report
/// (`spec/template-report.schema.json`) for a template package's bytes. An invalid template is
/// reported, not refused: its report has `kind: null` and its `template_invalid` issue.
pub fn inspect_template(bytes: &[u8]) -> serde_json::Value {
    compyla_ooxml::inspect(bytes)
}

/// The resolver of a host with none: in `lint`, every image exists without bytes (§10.3).
pub fn no_assets(_src: &str) -> Asset {
    Asset::Exists
}

fn load(json: &str) -> Result<compyla_core::ir_input::Loaded, OperationError> {
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

/// Input, parsed or loaded.
struct Source {
    document: ir::Document,
    sidecar: compyla_core::Sidecar,
    diagnostics: Vec<Diagnostic>,
    blocking: Blocking,
}

fn from_markdown(source: &str) -> Source {
    let compyla_core::ParseOutput {
        document,
        diagnostics,
        blocking,
        sidecar,
    } = compyla_core::parse(source);
    Source {
        document,
        sidecar,
        diagnostics,
        blocking,
    }
}

fn from_ir(json: &str) -> Result<Source, OperationError> {
    let loaded = load(json)?;
    Ok(Source {
        document: loaded.document,
        sidecar: compyla_core::Sidecar::default(),
        diagnostics: loaded.diagnostics,
        blocking: loaded.blocking,
    })
}

/// Input through validation.
struct Checked {
    /// Escalated, ordered and capped (§11).
    diagnostics: Vec<Diagnostic>,
    /// Whether any diagnostic, including any the cap dropped, is an error after escalation.
    blocked: bool,
    /// The composed model, its format and the document properties; absent when a limit stopped
    /// processing.
    composed: Option<(Composed, Format, Properties)>,
}

/// Document properties from `meta` only (C44): `title`, and `author` (a list joined with `, `).
#[cfg_attr(not(feature = "xlsx"), allow(dead_code))]
struct Properties {
    title: Option<String>,
    creator: Option<String>,
}

/// Parse or load output through validation. A limit stops processing (§12.1) before the format is
/// even resolved. With `rendering`, a format this build can't render is `renderer_unavailable`.
fn check(
    source: Source,
    options: &Options,
    assets: &dyn Fn(&str) -> Asset,
    rendering: bool,
) -> Result<Checked, OperationError> {
    let Source {
        mut document,
        sidecar,
        mut diagnostics,
        blocking,
    } = source;
    if diagnostics.iter().any(|d| d.code == "limit_exceeded") {
        finish(&mut diagnostics, options.strict);
        return Ok(Checked {
            diagnostics,
            blocked: true,
            composed: None,
        });
    }
    let format = resolve_format(options, &document)?;
    if rendering && !renderer_available(format) {
        return Err(OperationError {
            code: "renderer_unavailable",
            message: format!("This build can't render {} yet.", format.name()),
            pointer: None,
        });
    }
    let properties = properties(&document.meta);
    // Each diagnostic gets its node's IR path and original position (§10.7, §11.3).
    let (locator, sidecar) = compyla_core::locate::renumber(&mut document, &sidecar);
    let input = compyla_core::validate::Input {
        document: &document,
        sidecar: &sidecar,
        assets,
    };
    let mut out = Sink::located(locator);
    let composed = compyla_core::validate::validate(&input, format, &mut out);
    let (validated, validate_blocking) = out.into_parts();
    diagnostics.extend(validated);
    finish(&mut diagnostics, options.strict);
    Ok(Checked {
        diagnostics,
        blocked: blocking.merge(validate_blocking).blocks(options.strict),
        composed: Some((composed, format, properties)),
    })
}

#[cfg_attr(not(feature = "pptx"), allow(unused_variables))]
fn render(
    checked: Checked,
    assets: &dyn Fn(&str) -> Asset,
) -> Result<ConvertResult, OperationError> {
    let format = checked.composed.as_ref().map(|(_, format, _)| *format);
    // Rendering may add `render_failed`.
    #[cfg_attr(not(feature = "pptx"), allow(unused_mut))]
    let mut diagnostics = checked.diagnostics;
    let bytes = match checked.composed {
        _ if checked.blocked => None,
        #[cfg(feature = "xlsx")]
        Some((Composed::Xlsx(workbook), _, properties)) => {
            let properties = compyla_ooxml::Properties {
                title: properties.title,
                creator: properties.creator,
            };
            Some(compyla_xlsx::render(&workbook, &properties))
        }
        #[cfg(feature = "pptx")]
        Some((Composed::Pptx(deck), _, properties)) => {
            let properties = compyla_ooxml::Properties {
                title: properties.title,
                creator: properties.creator,
            };
            let images = |src: &str| match assets(src) {
                Asset::Bytes(bytes) => Some(bytes),
                _ => None,
            };
            let rendered =
                compyla_pptx::render(&deck, &compyla_pptx::house(), &properties, &images);
            match rendered {
                Ok(bytes) => Some(bytes),
                // A failure while rendering is the error diagnostic `render_failed` (§11.1).
                Err(why) => {
                    diagnostics.push(Diagnostic::new(
                        "render_failed",
                        format!("The document couldn't be rendered: the template {}.", why.0),
                    ));
                    None
                }
            }
        }
        _ => None,
    };
    Ok(ConvertResult {
        bytes,
        format,
        diagnostics,
    })
}

fn properties(meta: &serde_json::Map<String, serde_json::Value>) -> Properties {
    let text = |value: &serde_json::Value| match value {
        serde_json::Value::String(text) => Some(text.clone()),
        serde_json::Value::Array(items) => Some(
            items
                .iter()
                .filter_map(serde_json::Value::as_str)
                .collect::<Vec<_>>()
                .join(", "),
        ),
        _ => None,
    };
    Properties {
        title: meta.get("title").and_then(text),
        creator: meta.get("author").and_then(text),
    }
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

/// Strict escalation (§11.1), order (§11.2) and the cap, which keeps errors first.
fn finish(diagnostics: &mut Vec<Diagnostic>, strict: bool) {
    compyla_core::diagnostic::escalate(diagnostics, strict);
    compyla_core::diagnostic::sort(diagnostics);
    if compyla_core::diagnostic::cap(diagnostics) {
        diagnostics.push(Diagnostic::new(
            "diagnostics_truncated",
            format!("Only the first {DIAGNOSTICS_CAP} diagnostics are reported."),
        ));
    }
}
