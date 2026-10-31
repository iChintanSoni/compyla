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
    /// A template package; otherwise `meta.template` through the template resolver, else the
    /// house template.
    pub template: Option<Vec<u8>>,
    /// pptx layout names → the supplied template's layout each binds to (§8.5).
    pub layout_aliases: Vec<(String, String)>,
}

/// The host's resolvers (markdown-spec.md §10.3). An asset resolver alone converts into one.
#[derive(Clone, Copy)]
pub struct Resolvers<'a> {
    /// Image sources; hosts without one use [`no_assets`].
    pub assets: &'a dyn Fn(&str) -> Asset,
    /// `meta.template` references; `None` when the host has no template resolver.
    pub templates: Option<&'a TemplateResolver<'a>>,
}

/// A template resolver (§10.3): a `meta.template` reference's package bytes, or `None` when not
/// found.
pub type TemplateResolver<'a> = dyn Fn(&str) -> Option<Vec<u8>> + 'a;

impl<'a, F: Fn(&str) -> Asset> From<&'a F> for Resolvers<'a> {
    fn from(assets: &'a F) -> Self {
        Resolvers {
            assets,
            templates: None,
        }
    }
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
pub fn lint<'a>(
    source: &str,
    options: &Options,
    resolvers: impl Into<Resolvers<'a>>,
) -> Result<Vec<Diagnostic>, OperationError> {
    Ok(check(from_markdown(source), options, resolvers.into(), false)?.diagnostics)
}

/// `lint` for IR input (§10.7). Its diagnostics carry IR paths, and lines only where the IR has
/// positions.
pub fn lint_ir<'a>(
    json: &str,
    options: &Options,
    resolvers: impl Into<Resolvers<'a>>,
) -> Result<Vec<Diagnostic>, OperationError> {
    Ok(check(from_ir(json)?, options, resolvers.into(), false)?.diagnostics)
}

/// `convert`: markdown to the output format, with diagnostics from every stage.
pub fn convert<'a>(
    source: &str,
    options: &Options,
    resolvers: impl Into<Resolvers<'a>>,
) -> Result<ConvertResult, OperationError> {
    let resolvers = resolvers.into();
    let assets = converting(resolvers.assets);
    let resolvers = Resolvers {
        assets: &assets,
        ..resolvers
    };
    render(
        check(from_markdown(source), options, resolvers, true)?,
        &assets,
    )
}

/// `convert` for IR input (§10.7).
pub fn convert_ir<'a>(
    json: &str,
    options: &Options,
    resolvers: impl Into<Resolvers<'a>>,
) -> Result<ConvertResult, OperationError> {
    let resolvers = resolvers.into();
    let assets = converting(resolvers.assets);
    let resolvers = Resolvers {
        assets: &assets,
        ..resolvers
    };
    render(check(from_ir(json)?, options, resolvers, true)?, &assets)
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
    /// Whether warnings escalate (§11.1), for the diagnostics rendering adds.
    #[cfg_attr(not(feature = "pptx"), allow(dead_code))]
    strict: bool,
    /// The composed model, its format and the document properties; absent when a limit stopped
    /// processing.
    composed: Option<(Composed, Format, Properties)>,
    /// The supplied template, when one applies (§10.4); otherwise the house template is used.
    #[cfg_attr(not(feature = "pptx"), allow(dead_code))]
    template: Option<compyla_ooxml::template::Template>,
}

/// Document properties from `meta` only (C44): `title`, `author` (a list joined with `, `) and
/// `lang`.
#[cfg_attr(not(feature = "xlsx"), allow(dead_code))]
struct Properties {
    title: Option<String>,
    creator: Option<String>,
    language: Option<String>,
}

/// Parse or load output through validation. A limit stops processing (§12.1) before the format is
/// even resolved. With `rendering`, a format this build can't render is `renderer_unavailable`.
fn check(
    source: Source,
    options: &Options,
    resolvers: Resolvers<'_>,
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
            strict: options.strict,
            composed: None,
            template: None,
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
    let mut out = Sink::located(locator);
    let template = template(options, &document, &sidecar, resolvers, format, &mut out)?;
    // `layoutAliases` binds pptx layouts (§8.5); other formats ignore it. A template that was asked
    // for but can't be used has already been reported, and blocks the output.
    let requested = options.template.is_some() || document.meta.contains_key("template");
    let layouts = match (&template, format) {
        (Some(template), Format::Pptx) => Some(template_layouts(template, options)?),
        (None, Format::Pptx) if !requested && !options.layout_aliases.is_empty() => {
            return Err(option_invalid(
                "`layoutAliases` names a supplied pptx template's layouts, and there is none.",
            ));
        }
        _ => None,
    };
    let input = compyla_core::validate::Input {
        document: &document,
        sidecar: &sidecar,
        assets: resolvers.assets,
        template: layouts.as_ref(),
    };
    let composed = compyla_core::validate::validate(&input, format, &mut out);
    let (validated, validate_blocking) = out.into_parts();
    diagnostics.extend(validated);
    finish(&mut diagnostics, options.strict);
    Ok(Checked {
        diagnostics,
        blocked: blocking.merge(validate_blocking).blocks(options.strict),
        strict: options.strict,
        composed: Some((composed, format, properties)),
        // pptx renders into the template, and xlsx takes its theme tokens.
        template: template.filter(|_| matches!(format, Format::Pptx | Format::Xlsx)),
    })
}

/// The template this call uses (§10.2–§10.4): the `template` option, else `meta.template` through
/// the template resolver, else none (the house template). One that can't be resolved or read, or
/// whose kind doesn't suit `format`, is an error and never silently the house style.
fn template(
    options: &Options,
    document: &ir::Document,
    sidecar: &compyla_core::Sidecar,
    resolvers: Resolvers<'_>,
    format: Format,
    out: &mut Sink,
) -> Result<Option<compyla_ooxml::template::Template>, OperationError> {
    use compyla_ooxml::template::Kind;
    let bytes = match (&options.template, document.meta.get("template")) {
        (Some(bytes), _) => bytes.clone(),
        (None, None) => return Ok(None),
        (None, Some(reference)) => {
            let resolved = reference
                .as_str()
                .zip(resolvers.templates)
                .and_then(|(reference, resolve)| resolve(reference));
            match resolved {
                Some(bytes) => bytes,
                None => {
                    let mut diagnostic = Diagnostic::new(
                        "template_unresolved",
                        format!(
                            "The template `{}` can't be found; nothing is rendered without it.",
                            reference
                                .as_str()
                                .map_or_else(|| reference.to_string(), str::to_string)
                        ),
                    )
                    .hint("Check the `template` path, or supply the template with the call.");
                    if let Some(&line) = sidecar.meta_lines.get("template") {
                        diagnostic = diagnostic.at_line(line);
                    }
                    out.push(diagnostic);
                    return Ok(None);
                }
            }
        }
    };
    let template = match compyla_ooxml::template::Template::read(&bytes) {
        Ok(template) => template,
        Err(why) => {
            out.push(
                Diagnostic::new("template_invalid", format!("The template {}.", why.0))
                    .hint("Use a .docx, .dotx, .pptx or .potx file saved by Office, without macros or a password."),
            );
            return Ok(None);
        }
    };
    // §10.4: the kinds each format accepts.
    let accepted = match format {
        Format::Docx => template.kind == Kind::Docx,
        Format::Pdf => matches!(template.kind, Kind::Docx | Kind::Pptx),
        Format::Pptx => template.kind == Kind::Pptx,
        // A workbook takes a docx or pptx template's theme tokens (§9.2); a workbook template
        // is never used.
        Format::Xlsx if template.kind == Kind::Xlsx => {
            out.push(
                Diagnostic::new(
                    "xlsx_template_unsupported",
                    "A workbook template isn't used; a workbook takes its theme from a docx or pptx template.",
                )
                .hint("Supply a .docx, .dotx, .pptx or .potx template, or none."),
            );
            return Ok(None);
        }
        Format::Xlsx => true,
    };
    if !accepted {
        out.push(
            Diagnostic::new(
                "template_invalid",
                format!(
                    "The template is a {} package, which a {} document can't use.",
                    template.kind.name(),
                    format.name()
                ),
            )
            .hint(match format {
                Format::Docx => "Use a .docx or .dotx template.",
                Format::Pdf => "Use a .docx, .dotx, .pptx or .potx template.",
                _ => "Use a .pptx or .potx template.",
            }),
        );
        return Ok(None);
    }
    let invalid = |why: String| Diagnostic::new("template_invalid", format!("The template {why}."));
    match compyla_ooxml::inspect::warnings(&template) {
        // Styles matter to docx output only (§10.4).
        Ok(warnings) => out.extend(
            warnings
                .into_iter()
                .filter(|w| w.code != "no_styles_xml" || format == Format::Docx),
        ),
        Err(why) => {
            out.push(invalid(why.0));
            return Ok(None);
        }
    }
    // A deck is written into the template's layouts (§8.5): it needs some, readable.
    if format == Format::Pptx {
        match compyla_ooxml::layouts::read(&template) {
            Ok(layouts) if layouts.is_empty() => {
                out.push(invalid("has no slide layouts".into()));
                return Ok(None);
            }
            Ok(_) => {}
            Err(why) => {
                out.push(invalid(why.0));
                return Ok(None);
            }
        }
    }
    Ok(Some(template))
}

/// A pptx template's layouts as the core binds them (§8.5), with `layoutAliases` resolved to them.
fn template_layouts(
    template: &compyla_ooxml::template::Template,
    options: &Options,
) -> Result<compyla_core::validate::pptx::layout::TemplateLayouts, OperationError> {
    use compyla_core::validate::pptx::layout::{TemplateLayout, TemplateLayouts};
    let fold = compyla_core::validate::fold;
    // Read, and found not to be empty, when the template was accepted.
    let layouts: Vec<TemplateLayout> = compyla_ooxml::layouts::read(template)
        .unwrap_or_default()
        .into_iter()
        .map(|layout| TemplateLayout {
            content: layout.content().len(),
            name: layout.name,
            kind: layout.kind,
        })
        .collect();
    let mut aliases = Vec::new();
    for (name, target) in &options.layout_aliases {
        let wanted = fold(target);
        match layouts.iter().position(|l| fold(&l.name) == wanted) {
            Some(index) => aliases.push((name.clone(), index)),
            None => {
                return Err(option_invalid(&format!(
                    "`layoutAliases` maps `{name}` to `{target}`, which isn't a layout of the template."
                )));
            }
        }
    }
    Ok(TemplateLayouts { layouts, aliases })
}

fn option_invalid(message: &str) -> OperationError {
    OperationError {
        code: "option_invalid",
        message: message.into(),
        pointer: None,
    }
}

#[cfg_attr(not(feature = "pptx"), allow(unused_variables))]
fn render(
    checked: Checked,
    assets: &dyn Fn(&str) -> Asset,
) -> Result<ConvertResult, OperationError> {
    let format = checked.composed.as_ref().map(|(_, format, _)| *format);
    let mut diagnostics = checked.diagnostics;
    // Rendering may add `render_failed` and `template_content_removed`.
    #[cfg_attr(not(feature = "pptx"), allow(unused_mut))]
    let mut rendered: Vec<Diagnostic> = Vec::new();
    let bytes = match checked.composed {
        _ if checked.blocked => None,
        #[cfg(feature = "xlsx")]
        Some((Composed::Xlsx(workbook), _, properties)) => {
            let properties = compyla_ooxml::Properties {
                title: properties.title,
                creator: properties.creator,
                language: properties.language,
            };
            // A template without a theme keeps the house style (`no_theme`).
            let theme = checked
                .template
                .as_ref()
                .and_then(|template| compyla_ooxml::theme::Theme::of(template).ok().flatten());
            Some(compyla_xlsx::render(&workbook, &properties, theme.as_ref()))
        }
        #[cfg(feature = "pptx")]
        Some((Composed::Pptx(deck), _, properties)) => {
            let properties = compyla_ooxml::Properties {
                title: properties.title,
                creator: properties.creator,
                language: properties.language,
            };
            let images = |src: &str| match assets(src) {
                Asset::Bytes(bytes) => Some(bytes),
                _ => None,
            };
            let template = checked.template.unwrap_or_else(compyla_pptx::house);
            match compyla_pptx::render(&deck, &template, &properties, &images) {
                Ok(output) => {
                    rendered.extend(
                        output
                            .removed
                            .iter()
                            .map(|(part, what)| content_removed(part, what)),
                    );
                    Some(output.bytes)
                }
                // A failure while rendering is the error diagnostic `render_failed` (§11.1).
                Err(why) => {
                    rendered.push(Diagnostic::new(
                        "render_failed",
                        format!(
                            "The document couldn't be rendered: the template {}.",
                            escaped(&why.0)
                        ),
                    ));
                    None
                }
            }
        }
        _ => None,
    };
    // Render-stage diagnostics join the others in §11.2 order, under the cap.
    if !rendered.is_empty() {
        if diagnostics
            .last()
            .is_some_and(|d| d.code == "diagnostics_truncated")
        {
            diagnostics.pop();
        }
        diagnostics.extend(rendered);
        finish(&mut diagnostics, checked.strict);
    }
    Ok(ConvertResult {
        bytes,
        format,
        diagnostics,
    })
}

/// `template_content_removed` (§10.4): what a kept part of the template had removed.
#[cfg(feature = "pptx")]
fn content_removed(part: &str, what: &compyla_ooxml::sanitize::Removed) -> Diagnostic {
    use compyla_ooxml::sanitize::Removed;
    let part = shown(part);
    let message = match what {
        Removed::OleObject => format!(
            "An OLE object in the template's `{part}` was removed: output keeps no embedded or linked objects."
        ),
        Removed::ActiveX => format!(
            "An ActiveX control in the template's `{part}` was removed: output keeps no active content."
        ),
        Removed::External { kind, target } if kind.eq_ignore_ascii_case("hyperlink") => format!(
            "The template's `{part}` refers to `{}` through a hyperlink relationship but not from a hyperlink; it was removed: output keeps no external links but hyperlinks.",
            shown(target)
        ),
        Removed::External { kind, target } => format!(
            "The template's `{part}` links to `{}` through an external `{}` relationship, which was removed: output keeps no external links but hyperlinks.",
            shown(target),
            shown(kind)
        ),
        Removed::Hyperlink { target } => format!(
            "A hyperlink to `{}` in the template's `{part}` was removed: its destination isn't one §4.5 allows.",
            shown(target)
        ),
        Removed::Action { action } => format!(
            "A hyperlink action in the template's `{part}` (`{}`) was removed: output keeps no actions that run a program, a macro or an object.",
            shown(action)
        ),
    };
    Diagnostic::new("template_content_removed", message)
        .hint("Remove it from the template, and this warning goes away.")
}

/// Text from a template, as a message shows it: at most 100 characters, escaped.
#[cfg(feature = "pptx")]
fn shown(text: &str) -> String {
    const MOST: usize = 100;
    let mut out = escaped(&text.chars().take(MOST).collect::<String>());
    if text.chars().nth(MOST).is_some() {
        out.push('…');
    }
    out
}

/// `text` with control and bidi formatting characters escaped, so a template's text can't write
/// to a terminal or reorder a message's line.
#[cfg(feature = "pptx")]
fn escaped(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        let formatting = matches!(c, '\u{200E}' | '\u{200F}' | '\u{061C}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}');
        if c.is_control() || formatting {
            out.extend(c.escape_unicode());
        } else {
            out.push(c);
        }
    }
    out
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
        language: meta
            .get("lang")
            .and_then(|l| l.as_str())
            .map(str::to_string),
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
