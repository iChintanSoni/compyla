//! The pptx profile (markdown-spec.md §8). `slides` segments the document and structures each
//! slide (§8.1, §8.2); `layout` holds the house set, name resolution, measurement, inference and
//! fit (§8.3, §8.5, §8.6). Validation (§8.7) and the compose summary share one composition, so
//! they can't disagree about a slide's content or layout.

pub mod layout;
pub mod model;
mod slides;

use std::collections::HashSet;
use std::ptr;

use serde_json::{Value, json};

use self::layout::{Bound, Layout, Measure, Misfit, TemplateLayouts};
use self::slides::{Item, Slide};
use super::{Input, Slugger, at, for_each_block, fragment_links, options, title};
use crate::diagnostic::{Diagnostic, Sink};
use crate::ir::{Block, Directive, Document, Position};

/// The profile's diagnostics, and the composed deck the renderer writes.
pub fn validate(input: &Input<'_>, out: &mut Sink) -> model::Deck {
    let deck = slides::deck(&input.document.body);
    // `layout`, `notes` and `col` directives whose placement is settled here: used, or already
    // reported. The rest are nested where they can have no effect.
    let mut settled = HashSet::new();
    for slide in &deck.omitted {
        settled.extend(slide.notes.iter().map(|notes| ptr::from_ref(*notes)));
        if let Some(layout) = slide.layout {
            settled.insert(ptr::from_ref(layout));
            out.push(misplaced(
                layout,
                "`::: layout` is on a slide with no title, content or notes, which is omitted; it is ignored.",
                "Put content after the directive, or remove it.",
            ));
        }
    }
    let overflow = deck.overflow;
    let composed = compose(deck.slides, input.template);
    for slide in &composed {
        slide_rules(slide, input.template, &mut settled, out);
    }
    if let Some(template) = input.template {
        not_in_template(&composed, template, out);
    }
    if let Some(position) = overflow {
        // §12.1: the rest of the document is not processed.
        let mut diagnostic = Diagnostic::new(
            "limit_exceeded",
            format!(
                "The deck has more than {} slides; the rest of the document was not processed.",
                slides::MAX_SLIDES
            ),
        );
        if let Some(p) = position {
            diagnostic = diagnostic.at(p.line, Some(p.column));
        }
        out.push(diagnostic);
        return model::deck(&composed, &input.document.body);
    }
    nested_directives(&input.document.body, &settled, out);
    super::image_directives(&input.document.body, &image_options, out);
    anchors(input, &composed, out);
    model::deck(&composed, &input.document.body)
}

/// The compose summary (spec/conformance/compose.schema.json): slides with their titles, resolved
/// layouts, notes, and content block types.
pub fn summary(document: &Document) -> Value {
    let slides: Vec<Value> = compose(slides::deck(&document.body).slides, None)
        .iter()
        .map(|composed| {
            let slide = &composed.slide;
            json!({
                "title": slide.title.map(|section| title(&section.title)),
                "layout": composed.bound.name,
                "notes": slide.has_notes(),
                "content": slide.content.iter().map(content_type).collect::<Vec<_>>(),
            })
        })
        .collect();
    json!({ "format": "pptx", "slides": slides })
}

/// A content item's type in the summary. Entered directives, a misplaced `layout` among them
/// (§8.1), appear only through their children; the leading `layout` is unwrapped (§8.2).
fn content_type(item: &Item<'_>) -> String {
    match *item {
        Item::Heading(_) => "heading".into(),
        Item::Block(block) => match block {
            Block::Paragraph(_) => "paragraph",
            Block::List(_) => "list",
            Block::Table(_) => "table",
            Block::Media(_) => "media",
            Block::Code(_) => "code",
            Block::Quote(_) => "quote",
            // The walk turns these into other items, never `Item::Block`.
            Block::Section(_) => "heading",
            Block::Break(_) => "break",
            Block::Directive(_) => "directive",
        }
        .into(),
        Item::Col(_) => "col".into(),
        Item::Layout(d) | Item::Notes(d) => format!("directive:{}", d.name),
    }
}

// ---------------------------------------------------------------------------------------------
// Composition: layout selection (§8.3).

/// A slide with its resolved layout.
struct Composed<'a> {
    slide: Slide<'a>,
    request: Request<'a>,
    layout: Layout,
    /// The layout as bound to the template (§8.5).
    bound: Bound,
    /// The content measured against `layout`: `col` markers count only when it has two or more
    /// content slots (§8.4).
    measure: Measure,
}

/// What the slide's leading `layout` directive asks for.
enum Request<'a> {
    None,
    /// `::: layout` with no name (§1.6).
    Missing(&'a Directive),
    /// A name that doesn't resolve (§8.5), normalized.
    Unknown(&'a Directive, String),
    Explicit(&'a Directive, Bound),
}

impl<'a> Request<'a> {
    fn of(slide: &Slide<'a>, template: Option<&TemplateLayouts>) -> Self {
        let Some(directive) = slide.layout else {
            return Request::None;
        };
        let name = layout::normalize(&directive.args);
        if name.is_empty() {
            return Request::Missing(directive);
        }
        let bound = match template {
            Some(template) => template.resolve(&name),
            None => Layout::resolve(&name).map(Bound::house),
        };
        match bound {
            Some(bound) => Request::Explicit(directive, bound),
            None => Request::Unknown(directive, name),
        }
    }
}

/// §8.3, in priority order: explicit; the title slide, when the content fits it; inferred. A name
/// that is missing or doesn't resolve is selected as if there were no layout directive. A selected
/// house layout is bound to the template, if one is supplied (§8.5).
fn compose<'a>(slides: Vec<Slide<'a>>, template: Option<&TemplateLayouts>) -> Vec<Composed<'a>> {
    let count = slides.len();
    slides
        .into_iter()
        .enumerate()
        .map(|(index, slide)| {
            let request = Request::of(&slide, template);
            let bound = match &request {
                Request::Explicit(_, bound) => bound.clone(),
                _ => {
                    let written = Measure::new(&slide.content, true);
                    let title_slide = !matches!(request, Request::Explicit(..))
                        && index == 0
                        && count > 1
                        && Layout::TitleSlide
                            .misfit(slide.title.is_some(), &written)
                            .is_none();
                    let layout = if title_slide {
                        Layout::TitleSlide
                    } else {
                        Layout::infer(&written)
                    };
                    match template {
                        Some(template) => template.bind(layout),
                        None => Bound::house(layout),
                    }
                }
            };
            let layout = bound.layout;
            let measure = Measure::new(&slide.content, layout.slots() >= 2);
            Composed {
                slide,
                request,
                layout,
                bound,
                measure,
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------
// Validation (§8.7).

/// A slide's own rules: its layout name, misplaced `col` markers, demoted headings and fit. A
/// misplaced `layout` is entered by the slide walk (§8.1) and reported with the nested directives.
fn slide_rules(
    composed: &Composed<'_>,
    template: Option<&TemplateLayouts>,
    settled: &mut HashSet<*const Directive>,
    out: &mut Sink,
) {
    let names = || template.map_or_else(layout::names, TemplateLayouts::names);
    let slide = &composed.slide;
    match &composed.request {
        Request::Missing(directive) => out.push(at(
            Diagnostic::new(
                "invalid_directive_args",
                "`::: layout` names no layout; the layout is inferred from the content instead.",
            )
            .hint(format!("Name one of: {}.", names())),
            line(directive.position),
        )),
        Request::Unknown(directive, name) => out.push(at(
            Diagnostic::new(
                "unknown_layout",
                format!(
                    "Unknown layout `{name}`; the layout is inferred from the content instead. Valid layouts: {}.",
                    names()
                ),
            )
            .hint("Use one of the valid layout names."),
            line(directive.position),
        )),
        Request::None | Request::Explicit(..) => {}
    }
    settled.extend(slide.layout.map(ptr::from_ref));
    settled.extend(slide.notes.iter().map(|notes| ptr::from_ref(*notes)));

    let columns = composed.layout.slots() >= 2;
    for item in &slide.content {
        match *item {
            Item::Col(col) => {
                settled.insert(ptr::from_ref(col));
                if !columns {
                    out.push(misplaced(
                        col,
                        &format!(
                            "`col` has no effect in the {} layout, which has fewer than two content slots; it is ignored.",
                            composed.bound.name
                        ),
                        "Use a layout with two content slots, such as `::: layout Two Content`, or remove the marker.",
                    ));
                }
            }
            Item::Block(Block::List(_) | Block::Quote(_)) => {
                if let Item::Block(block) = item {
                    nested_frames(std::slice::from_ref(*block), out);
                }
            }
            Item::Heading(section) if section.level >= 4 => out.push(at(
                Diagnostic::new(
                    "heading_too_deep",
                    format!(
                        "An H{} heading inside a slide is shown as a demoted heading; deep headings suggest a document written for docx.",
                        section.level
                    ),
                )
                .hint("A slide has one heading, its title. Start a new slide with `---`, or use bold text."),
                line(section.position),
            )),
            _ => {}
        }
    }

    if let Some(misfit) = composed
        .layout
        .misfit(slide.title.is_some(), &composed.measure)
    {
        let name = &composed.bound.name;
        let message = format!(
            "This slide's content doesn't fit the {name} layout, which holds {}.",
            composed.layout.capacity()
        );
        let diagnostic = Diagnostic::new("layout_overflow", message);
        out.push(match composed.request {
            Request::Explicit(directive, _) => at(
                diagnostic.hint(
                    "Choose a layout with room for this content, or split the slide with `---`.",
                ),
                line(directive.position),
            ),
            _ => at(
                diagnostic.hint(format!(
                    "Split the slide with `---`; {name} was inferred from the content."
                )),
                line(match misfit {
                    Misfit::Block(position) => position,
                    Misfit::Title => slide.title.and_then(|section| section.position),
                }),
            ),
        });
    }
}

/// §8.5: each house layout the template has no layout for, once, with the layout used instead.
fn not_in_template(composed: &[Composed<'_>], template: &TemplateLayouts, out: &mut Sink) {
    let mut reported = HashSet::new();
    for composed in composed.iter().filter(|c| c.bound.fallback) {
        if reported.insert(composed.bound.name.clone()) {
            out.push(
                Diagnostic::new(
                    "layout_not_in_template",
                    format!(
                        "The template has no {} layout (by name or by type); its slides use the template's `{}` layout instead.",
                        composed.bound.name,
                        composed
                            .bound
                            .template
                            .map_or("", |index| template.layouts[index].name.as_str()),
                    ),
                )
                .hint("Add the layout to the template, or map the name to one of its layouts with `layoutAliases`."),
            );
        }
    }
}

/// §8.8: images and tables inside a list item or a quote, which a text body can't hold.
fn nested_frames(blocks: &[Block], out: &mut Sink) {
    for block in blocks {
        match block {
            Block::List(list) => {
                for item in &list.items {
                    for child in &item.children {
                        report_frame(child, out);
                    }
                }
            }
            Block::Quote(quote) => {
                for child in &quote.children {
                    report_frame(child, out);
                }
            }
            _ => {}
        }
    }
}

fn report_frame(block: &Block, out: &mut Sink) {
    let (what, position) = match block {
        Block::Media(media) => ("An image", media.position),
        Block::Table(table) => ("A table", table.position),
        Block::List(_) | Block::Quote(_) => return nested_frames(std::slice::from_ref(block), out),
        Block::Directive(d) => {
            for child in &d.children {
                report_frame(child, out);
            }
            return;
        }
        _ => return,
    };
    let shown = if matches!(block, Block::Media(_)) {
        "its alt text"
    } else {
        "tab-separated text rows"
    };
    out.push(at(
        Diagnostic::new(
            "pptx_ignored_content",
            format!("{what} inside a list item or a quote can't be placed in a slide's text; it is shown as {shown}."),
        )
        .hint("Put it at the slide's top level, outside the list or quote."),
        line(position),
    ));
}

/// `layout`, `notes` and `col` directives the slide walk never placed, where they can have no
/// effect (§8.2–§8.4): a `layout` that isn't its slide's first block, and any of them inside list
/// items, quotes or `notes` bodies.
fn nested_directives(body: &[Block], settled: &HashSet<*const Directive>, out: &mut Sink) {
    for_each_block(body, &mut |block| {
        let Block::Directive(directive) = block else {
            return;
        };
        if settled.contains(&ptr::from_ref(directive)) {
            return;
        }
        let (message, hint) = match directive.name.as_str() {
            "layout" => (
                "`::: layout` isn't the first block of its slide; it is ignored.",
                "Put `::: layout` first on the slide, right after the title and at its top level, or start a new slide before it with `---`.",
            ),
            "notes" => (
                "`::: notes` is nested inside another block, so it doesn't become speaker notes; it is ignored.",
                "Move `::: notes` to the slide's top level.",
            ),
            "col" => (
                "`col` is nested inside another block, where it can't start a column; it is ignored.",
                "Put the column marker at the slide's top level.",
            ),
            _ => return,
        };
        out.push(misplaced(directive, message, hint));
    });
}

fn image_options(directive: &Directive, out: &mut Sink) {
    let line = line(directive.position);
    let parsed = options(&directive.args);
    if let Some(key) = parsed.repeated {
        out.push(at(
            Diagnostic::new(
                "invalid_directive_args",
                format!("`::: image` repeats `{key}`."),
            ),
            line,
        ));
        return;
    }
    for (key, value) in parsed.entries {
        match key.as_str() {
            "fit" => {
                let value = value.unwrap_or_default();
                if !["contain", "cover"].contains(&value.to_ascii_lowercase().as_str()) {
                    out.push(at(
                        Diagnostic::new(
                            "unknown_directive_option",
                            format!("`fit={value}` isn't `contain` or `cover`; it is ignored."),
                        )
                        .hint("Use `::: image fit=contain` or `::: image fit=cover`."),
                        line,
                    ));
                }
            }
            "width" => {} // a docx and pdf option, ignored quietly in pptx (§1.6)
            other => out.push(at(
                Diagnostic::new(
                    "unknown_directive_option",
                    format!("`::: image` has no option `{other}`; it is ignored."),
                ),
                line,
            )),
        }
    }
}

/// §8.7: fragment links resolve to slide titles' slugs, assigned in slide order (§4.5).
fn anchors(input: &Input<'_>, slides: &[Composed<'_>], out: &mut Sink) {
    let mut slugger = Slugger::new();
    let targets: Vec<String> = slides
        .iter()
        .filter_map(|composed| composed.slide.title)
        .map(|section| slugger.slug(&title(&section.title)))
        .collect();
    fragment_links(input, &targets, "slide title", out);
}

fn misplaced(directive: &Directive, message: &str, hint: &str) -> Diagnostic {
    at(
        Diagnostic::new("directive_misplaced", message).hint(hint),
        line(directive.position),
    )
}

fn line(position: Option<Position>) -> Option<usize> {
    position.map(|p| p.line)
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use crate::validate::{Asset, Format, Input, validate};

    /// `(code, line)` of every parse and validate diagnostic, in §11.2 order.
    fn lint(source: &str) -> Vec<(String, Option<usize>)> {
        let parsed = crate::parse(source);
        let input = Input {
            document: &parsed.document,
            sidecar: &parsed.sidecar,
            assets: &|_| Asset::Exists,
            template: None,
        };
        let mut out = crate::diagnostic::Sink::new();
        validate(&input, Format::Pptx, &mut out);
        let mut diagnostics = parsed.diagnostics.clone();
        diagnostics.extend(out.into_vec());
        crate::diagnostic::sort(&mut diagnostics);
        diagnostics.into_iter().map(|d| (d.code, d.line)).collect()
    }

    fn expect(list: &[(&str, usize)]) -> Vec<(String, Option<usize>)> {
        list.iter()
            .map(|(code, line)| (code.to_string(), Some(*line)))
            .collect()
    }

    fn slides(source: &str) -> Value {
        super::summary(&crate::parse(source).document)["slides"].clone()
    }

    /// Each slide's layout and content types.
    fn shape(source: &str) -> Vec<(String, Vec<String>)> {
        let slides = slides(source);
        let text = |v: &Value| v.as_str().unwrap().to_string();
        slides
            .as_array()
            .unwrap()
            .iter()
            .map(|s| {
                let content = s["content"].as_array().unwrap();
                (text(&s["layout"]), content.iter().map(text).collect())
            })
            .collect()
    }

    #[test]
    fn layout_after_content_is_misplaced_and_ignored() {
        let source = "## S\n\nIntro.\n\n::: layout Two Content\n\nA\n\n---\n\nB\n\n:::\n";
        assert_eq!(lint(source), expect(&[("directive_misplaced", 5)]));
        // Ignored like any misplaced annotation (§1.5): its body is entered, so the `---` inside
        // splits the slide and its paragraphs count in place.
        assert_eq!(
            shape(source),
            [
                (
                    "Title Slide".to_string(),
                    vec!["paragraph".to_string(), "paragraph".to_string()]
                ),
                (
                    "Title and Content".to_string(),
                    vec!["paragraph".to_string()]
                )
            ]
        );
    }

    #[test]
    fn layout_on_an_omitted_slide_is_misplaced() {
        let source = "## A\n\nText.\n\n---\n\n::: layout Two Content :::\n";
        assert_eq!(lint(source), expect(&[("directive_misplaced", 7)]));
        assert_eq!(slides(source).as_array().unwrap().len(), 1);
    }

    #[test]
    fn layout_names_are_normalized_and_validated() {
        let source = "## A\n\n::: layout   two   CONTENT  :::\n\nL\n\n<!-- col -->\n\nR\n";
        assert_eq!(lint(source), []);
        assert_eq!(shape(source)[0].0, "Two Content");

        let source = "## A\n\n::: layout\n\nBody.\n\n:::\n";
        assert_eq!(lint(source), expect(&[("invalid_directive_args", 3)]));
        assert_eq!(shape(source)[0].0, "Title and Content");
    }

    #[test]
    fn an_unresolved_name_is_selected_as_if_absent() {
        // As if there were no layout directive (§8.3): the title-slide rule still applies.
        let source = "# Deck\n\n::: layout Fancy :::\n\nSubtitle.\n\n---\n\n## Next\n\nText.\n";
        assert_eq!(lint(source), expect(&[("unknown_layout", 3)]));
        assert_eq!(shape(source)[0].0, "Title Slide");
    }

    #[test]
    fn the_quote_layout_holds_a_quote_or_text_only() {
        let table = "## Q\n\n::: layout Quote\n\n| A |\n|---|\n| 1 |\n\n:::\n";
        assert_eq!(lint(table), expect(&[("layout_overflow", 3)]));
        let quote = "## Q\n\n::: layout Quote\n\n> Words.\n\n:::\n";
        assert_eq!(lint(quote), []);
    }

    #[test]
    fn inferred_overflow_points_at_the_first_block_that_does_not_fit() {
        // Two Content with three columns: the third column's block doesn't fit.
        let source = "## S\n\nA\n\n<!-- col -->\n\nB\n\n<!-- col -->\n\nC\n";
        assert_eq!(shape(source)[0].0, "Two Content");
        assert_eq!(lint(source), expect(&[("layout_overflow", 11)]));
        // Two blocks in one column.
        let source = "## S\n\nA\n\n| T |\n|---|\n| 1 |\n\n<!-- col -->\n\nB\n";
        assert_eq!(lint(source), expect(&[("layout_overflow", 5)]));
    }

    #[test]
    fn explicit_layouts_check_their_slots() {
        // Comparison: per column, an optional text block, then one block.
        let fits = "## S\n\n::: layout Comparison :::\n\nH\n\n| T |\n|---|\n| 1 |\n\n<!-- col -->\n\nH2\n\n![x](x.png)\n";
        assert_eq!(lint(fits), []);
        let text_after_block = "## S\n\n::: layout Comparison :::\n\n| T |\n|---|\n| 1 |\n\nAfter.\n\n<!-- col -->\n\nB\n";
        assert_eq!(lint(text_after_block), expect(&[("layout_overflow", 3)]));
        // Blank has no title.
        let blank = "## S\n\n::: layout Blank :::\n";
        assert_eq!(lint(blank), expect(&[("layout_overflow", 3)]));
        // Picture with Caption holds media and text only.
        let table = "## S\n\n::: layout Picture with Caption :::\n\n| T |\n|---|\n| 1 |\n";
        assert_eq!(lint(table), expect(&[("layout_overflow", 3)]));
        // Content with Caption: one non-text block and one text block, in either order.
        let caption =
            "## S\n\n::: layout Content with Caption :::\n\nCaption.\n\n| T |\n|---|\n| 1 |\n";
        assert_eq!(lint(caption), []);
    }

    #[test]
    fn col_in_a_layout_without_two_slots_is_misplaced_and_ignored() {
        // Ignored, the marker doesn't end the text block, so the content still fits.
        let source = "## S\n\n::: layout Title and Content :::\n\nA\n\n<!-- col -->\n\nB\n";
        assert_eq!(lint(source), expect(&[("directive_misplaced", 7)]));
    }

    #[test]
    fn the_title_slide_rule_measures_col_markers_as_written() {
        let source = "# Deck\n\nA\n\n<!-- col -->\n\nB\n\n---\n\n## Next\n\nText.\n";
        assert_eq!(shape(source)[0].0, "Two Content");
        assert_eq!(lint(source), []);
    }

    #[test]
    fn col_and_notes_outside_top_level_content_are_misplaced() {
        let source = "## S\n\n- item\n\n  <!-- col -->\n\n::: notes\n\nSay.\n\n::: notes\n\nInner.\n\n:::\n\n:::\n";
        assert_eq!(
            lint(source),
            expect(&[("directive_misplaced", 5), ("directive_misplaced", 11)])
        );
        assert_eq!(slides(source)[0]["notes"], json!(true));
    }

    #[test]
    fn title_and_notes_inside_a_leading_layout() {
        let source = "::: layout Two Content\n\n## T\n\nA\n\n<!-- col -->\n\nB\n\n::: notes\n\nSay.\n\n:::\n\n---\n\n:::\n";
        assert_eq!(lint(source), []);
        assert_eq!(
            slides(source),
            json!([{ "title": "T", "layout": "Two Content", "notes": true,
                     "content": ["paragraph", "col", "paragraph"] }])
        );
    }

    #[test]
    fn empty_notes_are_no_notes() {
        let source = "## S\n\nText.\n\n::: notes\n:::\n\n---\n\n::: notes\n:::\n";
        assert_eq!(slides(source).as_array().unwrap().len(), 1);
        assert_eq!(slides(source)[0]["notes"], json!(false));
    }

    #[test]
    fn breaks_in_lists_and_quotes_do_not_split() {
        let source = "## S\n\n- a\n\n  ***\n\n> b\n>\n> ***\n";
        assert_eq!(slides(source).as_array().unwrap().len(), 1);
    }

    #[test]
    fn ignored_markers_and_unknown_directives_count_in_place() {
        let source = "## S\n\nA\n\n<!-- toc -->\n\n::: aside\n\nB\n\n:::\n";
        assert_eq!(
            shape(source),
            [(
                "Title and Content".to_string(),
                vec!["paragraph".to_string(), "paragraph".to_string()]
            )]
        );
        assert_eq!(lint(source), expect(&[("unknown_directive", 7)]));
    }

    #[test]
    fn heading_too_deep_only_for_demoted_headings() {
        assert_eq!(lint("#### Title\n\nText.\n"), []);
        assert_eq!(lint("## S\n\n> #### Quoted\n"), []);
        assert_eq!(
            lint("## S\n\n##### Deep\n"),
            expect(&[("heading_too_deep", 3)])
        );
    }

    #[test]
    fn image_directive_options_and_placement() {
        let fits = "## S\n\n::: image fit=Cover width=50%\n\n![x](x.png)\n\n:::\n";
        assert_eq!(lint(fits), []);
        let stretch = "## S\n\n::: image fit=stretch\n\n![x](x.png)\n\n:::\n";
        assert_eq!(lint(stretch), expect(&[("unknown_directive_option", 3)]));
        let repeated = "## S\n\n::: image fit=cover fit=contain :::\n\n![x](x.png)\n";
        assert_eq!(lint(repeated), expect(&[("invalid_directive_args", 3)]));
        let nothing = "## S\n\n::: image fit=cover :::\n\nText.\n";
        assert_eq!(lint(nothing), expect(&[("directive_misplaced", 3)]));
    }

    #[test]
    fn fragment_links_target_slide_titles() {
        let source = "## Intro\n\nSee [costs](#costs) and [detail](#detail).\n\n### Detail\n\n---\n\n## Costs\n\nText.\n";
        assert_eq!(lint(source), expect(&[("link_anchor_unresolved", 3)]));
    }
}
