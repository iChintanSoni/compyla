//! The docx and pdf profiles (markdown-spec.md §6, §7): flow documents.

use super::{Format, Input, Slugger, at, fragment_links, options, title};
use crate::diagnostic::Diagnostic;
use crate::ir::{Block, Directive};

pub fn validate(input: &Input<'_>, format: Format, out: &mut Vec<Diagnostic>) {
    headings(&input.document.body, 0, out);
    super::image_directives(&input.document.body, &image_options, out);
    page_geometry(input, out);
    anchors(input, format, out);
}

/// §6.3 `heading_skip`: compares headings within one heading tree. A directive body, list item or
/// quote starts a tree of its own (§4.2), where the first heading has no parent.
fn headings(blocks: &[Block], parent: u8, out: &mut Vec<Diagnostic>) {
    for block in blocks {
        match block {
            Block::Section(section) => {
                if parent > 0 && section.level > parent + 1 {
                    out.push(at(
                        Diagnostic::new(
                            "heading_skip",
                            format!("Heading level jumps from H{parent} to H{}.", section.level),
                        )
                        .hint(format!(
                            "Insert an H{} before this heading, or promote it to H{}.",
                            parent + 1,
                            parent + 1
                        )),
                        section.position.map(|p| p.line),
                    ));
                }
                headings(&section.children, section.level, out);
            }
            Block::Quote(quote) => headings(&quote.children, 0, out),
            Block::Directive(directive) => headings(&directive.children, 0, out),
            Block::List(list) => {
                for item in &list.items {
                    headings(&item.children, 0, out);
                }
            }
            _ => {}
        }
    }
}

fn image_options(d: &Directive, out: &mut Vec<Diagnostic>) {
    let line = d.position.map(|p| p.line);
    let parsed = options(&d.args);
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
            "width" => {
                let number = value.as_deref().map(|v| v.strip_suffix('%').unwrap_or(v));
                let valid = number.is_some_and(|n| {
                    !n.starts_with('0')
                        && n.bytes().all(|b| b.is_ascii_digit())
                        && n.parse::<u32>().is_ok_and(|w| (1..=100).contains(&w))
                });
                if !valid {
                    out.push(at(
                        Diagnostic::new(
                            "invalid_directive_args",
                            format!(
                                "`width={}` must be an integer from 1 to 100 (a percentage).",
                                value.unwrap_or_default()
                            ),
                        )
                        .hint("For example `::: image width=50%`."),
                        line,
                    ));
                }
            }
            "fit" => {} // a pptx option, ignored quietly in docx and pdf (§1.6)
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

/// §7: `page_size` and `margins`, reported at their frontmatter key's line.
fn page_geometry(input: &Input<'_>, out: &mut Vec<Diagnostic>) {
    let meta = &input.document.meta;
    let line = |key: &str| input.sidecar.meta_lines.get(key).copied();
    if let Some(value) = meta.get("page_size").and_then(|v| v.as_str())
        && !["a4", "letter"].contains(&value.to_ascii_lowercase().as_str())
    {
        out.push(at(
            Diagnostic::new(
                "invalid_page_size",
                format!("Unknown page_size `{value}`; the template's page size or A4 is used."),
            )
            .hint("Use A4 or Letter."),
            line("page_size"),
        ));
    }
    if let Some(value) = meta.get("margins").and_then(|v| v.as_str())
        && !["normal", "narrow"].contains(&value.to_ascii_lowercase().as_str())
    {
        out.push(at(
            Diagnostic::new(
                "invalid_margins",
                format!("Unknown margins `{value}`; the template's margins or normal is used."),
            )
            .hint("Use normal or narrow."),
            line("margins"),
        ));
    }
}

/// §6.2: fragment links resolve to heading slugs. Slugs are assigned to every section in document
/// order; sections inside list items and quotes take a slug but aren't targets (§5).
fn anchors(input: &Input<'_>, format: Format, out: &mut Vec<Diagnostic>) {
    let mut slugger = Slugger::new();
    let mut targets = Vec::new();
    collect(&input.document.body, true, &mut slugger, &mut targets);
    let what = if format == Format::Pdf {
        "heading in the PDF"
    } else {
        "heading in the document"
    };
    fragment_links(input, &targets, what, out);
}

fn collect(blocks: &[Block], targetable: bool, slugger: &mut Slugger, targets: &mut Vec<String>) {
    for block in blocks {
        match block {
            Block::Section(section) => {
                let slug = slugger.slug(&title(&section.title));
                if targetable {
                    targets.push(slug);
                }
                collect(&section.children, targetable, slugger, targets);
            }
            Block::Directive(d) => collect(&d.children, targetable, slugger, targets),
            Block::Quote(q) => collect(&q.children, false, slugger, targets),
            Block::List(l) => {
                for item in &l.items {
                    collect(&item.children, false, slugger, targets);
                }
            }
            _ => {}
        }
    }
}
