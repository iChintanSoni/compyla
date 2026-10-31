//! IR paths on diagnostics (markdown-spec.md §10.7, §11.3).
//!
//! Profiles say where a problem is through the positions of the IR nodes involved: every §11.3
//! `position` kind a profile uses derives from a node's position (`node`, `table`, `construct`,
//! `fence`), a table's position plus a row (`row`), or a frontmatter key's line (`key`). Rather than
//! thread paths through every profile, [`renumber`] gives every node, table row, frontmatter key and
//! link a synthetic line of its own before validation. A diagnostic's line then names exactly one of
//! them, and [`Locator::locate`] gives the diagnostic that one's JSON Pointer and restores its line
//! and column from the original, or drops them where the original has none (IR input without
//! positions).

use std::collections::HashSet;

use crate::diagnostic::Diagnostic;
use crate::ir::{Block, Document, Position, Run};
use crate::parse::Sidecar;
use crate::validate::row_line;

/// What a synthetic line stands for.
#[derive(Debug, Clone, Default)]
struct Slot {
    path: Option<String>,
    line: Option<usize>,
    column: Option<usize>,
}

/// Maps diagnostics on a [`renumber`]ed document back to paths and original positions.
#[derive(Debug)]
pub struct Locator {
    /// Indexed by synthetic line; line 0 doesn't exist.
    slots: Vec<Slot>,
}

impl Locator {
    /// Gives each diagnostic with a synthetic line its node's path (unless it has one), and its
    /// original line and column.
    pub fn locate(&self, diagnostics: &mut [Diagnostic]) {
        for diagnostic in diagnostics {
            // Every line a profile reports comes from a renumbered position (see the module docs).
            debug_assert!(
                diagnostic
                    .line
                    .is_none_or(|line| (1..self.slots.len()).contains(&line)),
                "{} at line {:?}, which renumbering didn't assign",
                diagnostic.code,
                diagnostic.line
            );
            let Some(slot) = diagnostic.line.and_then(|line| self.slots.get(line)) else {
                continue;
            };
            if diagnostic.path.is_none() {
                diagnostic.path = slot.path.clone();
            }
            diagnostic.column = diagnostic.column.and(slot.column);
            diagnostic.line = slot.line;
        }
    }
}

/// Replaces every position in `document` with a synthetic one, in place, and returns the sidecar to
/// validate it with. Diagnostics produced from the renumbered document must go through the
/// [`Locator`] before they are reported.
pub fn renumber(document: &mut Document, sidecar: &Sidecar) -> (Locator, Sidecar) {
    let mut numbering = Numbering {
        slots: vec![Slot::default()],
        links: Vec::new(),
    };
    numbering.blocks(&mut document.body, "/body");

    let mut renumbered = Sidecar::default();
    let keys: HashSet<&String> = document
        .meta
        .keys()
        .chain(sidecar.meta_lines.keys())
        .collect();
    let mut keys: Vec<&String> = keys.into_iter().collect();
    keys.sort();
    for key in keys {
        let line = numbering.slot(Slot {
            path: document
                .meta
                .contains_key(key.as_str())
                .then(|| format!("/meta/{}", escape(key))),
            line: sidecar.meta_lines.get(key.as_str()).copied(),
            column: None,
        });
        renumbered.meta_lines.insert(key.clone(), line);
    }
    // Markdown input records each link at its construct (§4.5). IR input has no constructs, so its
    // links are its runs' and images' link destinations, at their nodes.
    let links = if sidecar.links.is_empty() {
        std::mem::take(&mut numbering.links)
    } else {
        sidecar
            .links
            .iter()
            .map(|(url, position)| {
                let slot = Slot {
                    path: None,
                    line: Some(position.line),
                    column: Some(position.column),
                };
                (url.clone(), slot)
            })
            .collect()
    };
    for (url, slot) in links {
        let line = numbering.slot(slot);
        renumbered.links.push((url, Position::new(line, 1)));
    }
    (
        Locator {
            slots: numbering.slots,
        },
        renumbered,
    )
}

struct Numbering {
    slots: Vec<Slot>,
    /// IR links, found while numbering, for documents without a link sidecar.
    links: Vec<(String, Slot)>,
}

impl Numbering {
    /// A fresh line for `slot`.
    fn slot(&mut self, slot: Slot) -> usize {
        self.slots.push(slot);
        self.slots.len() - 1
    }

    /// Gives a node at `path` a fresh line, returning its original line.
    fn node(&mut self, position: &mut Option<Position>, path: &str) -> Option<usize> {
        let original = *position;
        let line = self.slot(Slot {
            path: Some(path.to_string()),
            line: original.map(|p| p.line),
            column: original.map(|p| p.column),
        });
        *position = Some(Position::new(line, 1));
        original.map(|p| p.line)
    }

    fn blocks(&mut self, blocks: &mut [Block], path: &str) {
        for (index, block) in blocks.iter_mut().enumerate() {
            self.block(block, &format!("{path}/{index}"));
        }
    }

    fn block(&mut self, block: &mut Block, path: &str) {
        match block {
            Block::Section(section) => {
                let line = self.node(&mut section.position, path);
                self.runs(&section.title, &format!("{path}/title"), line);
                self.blocks(&mut section.children, &format!("{path}/children"));
            }
            Block::Paragraph(paragraph) => {
                let line = self.node(&mut paragraph.position, path);
                self.runs(&paragraph.runs, &format!("{path}/runs"), line);
            }
            Block::Table(table) => {
                let original = table.position;
                let line = self.node(&mut table.position, path);
                // The delimiter row's line, then one line per data row, so `row_line` lands on
                // the row's own slot.
                self.slot(Slot {
                    path: Some(path.to_string()),
                    line: original.map(|p| p.line.saturating_add(1)),
                    column: None,
                });
                for r in 0..table.rows.len() {
                    self.slot(Slot {
                        path: Some(format!("{path}/rows/{r}")),
                        line: original.map(|p| row_line(p, r)),
                        column: None,
                    });
                }
                for (c, cell) in table.header.iter().enumerate() {
                    self.runs(&cell.runs, &format!("{path}/header/{c}/runs"), line);
                }
                for (r, row) in table.rows.iter().enumerate() {
                    let row_line = original.map(|p| row_line(p, r));
                    for (c, cell) in row.iter().enumerate() {
                        self.runs(&cell.runs, &format!("{path}/rows/{r}/{c}/runs"), row_line);
                    }
                }
            }
            Block::List(list) => {
                self.node(&mut list.position, path);
                for (index, item) in list.items.iter_mut().enumerate() {
                    let item_path = format!("{path}/items/{index}");
                    self.node(&mut item.position, &item_path);
                    self.blocks(&mut item.children, &format!("{item_path}/children"));
                }
            }
            Block::Media(media) => {
                let line = self.node(&mut media.position, path);
                if let Some(link) = &media.link {
                    let slot = Slot {
                        path: Some(format!("{path}/link")),
                        line,
                        column: None,
                    };
                    self.links.push((link.clone(), slot));
                }
            }
            Block::Code(code) => {
                self.node(&mut code.position, path);
            }
            Block::Quote(quote) => {
                self.node(&mut quote.position, path);
                self.blocks(&mut quote.children, &format!("{path}/children"));
            }
            Block::Break(node) => {
                self.node(&mut node.position, path);
            }
            Block::Directive(directive) => {
                self.node(&mut directive.position, path);
                self.blocks(&mut directive.children, &format!("{path}/children"));
            }
        }
    }

    /// Records the links in `runs`: consecutive runs with the same destination are one link, at
    /// its first run. `line` is the containing node's original line.
    fn runs(&mut self, runs: &[Run], path: &str, line: Option<usize>) {
        let mut previous: Option<&str> = None;
        for (index, run) in runs.iter().enumerate() {
            let link = run.link.as_deref();
            if let Some(url) = link
                && previous != Some(url)
            {
                let slot = Slot {
                    path: Some(format!("{path}/{index}")),
                    line,
                    column: None,
                };
                self.links.push((url.to_string(), slot));
            }
            previous = link;
        }
    }
}

/// A JSON Pointer reference token (RFC 6901): `~` as `~0`, `/` as `~1`.
fn escape(token: &str) -> String {
    token.replace('~', "~0").replace('/', "~1")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::validate::{Asset, Format, Input, validate};

    fn no_assets(_: &str) -> Asset {
        Asset::Exists
    }

    /// Validates `document` as the facade does: renumbered, then located and sorted.
    fn lint(mut document: Document, sidecar: &Sidecar, format: Format) -> Vec<Diagnostic> {
        let (locator, sidecar) = renumber(&mut document, sidecar);
        let input = Input {
            document: &document,
            sidecar: &sidecar,
            assets: &no_assets,
        };
        let mut out = crate::diagnostic::Sink::located(locator);
        validate(&input, format, &mut out);
        let mut diagnostics = out.into_vec();
        crate::diagnostic::sort(&mut diagnostics);
        diagnostics
    }

    fn lint_ir(json: &str, format: Format) -> Vec<(String, Option<usize>, Option<String>)> {
        let document = crate::ir_input::load(json).expect("valid IR").document;
        lint(document, &Sidecar::default(), format)
            .into_iter()
            .map(|d| (d.code, d.line, d.path))
            .collect()
    }

    fn owned(
        expected: &[(&str, Option<usize>, Option<&str>)],
    ) -> Vec<(String, Option<usize>, Option<String>)> {
        expected
            .iter()
            .map(|(code, line, path)| (code.to_string(), *line, path.map(String::from)))
            .collect()
    }

    #[test]
    fn ir_without_positions_gets_paths_and_no_lines() {
        let json = r##"{"irVersion":"0.1","meta":{"page_size":"Tabloid"},"body":[
            {"type":"section","level":1,"title":[{"text":"A"}],"children":[
                {"type":"section","level":3,"title":[{"text":"Deep"}],"children":[]},
                {"type":"paragraph","runs":[{"text":"see "},{"text":"there","link":"#nowhere"}]}
            ]},
            {"type":"directive","name":"bogus","args":"","children":[]}
        ]}"##;
        assert_eq!(
            lint_ir(json, Format::Docx),
            owned(&[
                ("heading_skip", None, Some("/body/0/children/0")),
                ("invalid_page_size", None, Some("/meta/page_size")),
                (
                    "link_anchor_unresolved",
                    None,
                    Some("/body/0/children/1/runs/1")
                ),
                ("unknown_directive", None, Some("/body/1")),
            ])
        );
    }

    #[test]
    fn ir_rows_and_positions_are_kept() {
        // A table at line 3 has its second data row at line 6.
        let json = r##"{"irVersion":"0.1","meta":{},"body":[
            {"type":"section","level":1,"title":[{"text":"S"}],"position":{"line":1,"column":1},"children":[
                {"type":"table","position":{"line":3,"column":1},
                 "header":[{"runs":[{"text":"A"}],"raw":"A"}],"align":[null],
                 "rows":[[{"runs":[{"text":"1"}],"raw":"1"}],[{"runs":[{"text":"=SUM("}],"raw":"=SUM("}]]}
            ]}
        ]}"##;
        assert_eq!(
            lint_ir(json, Format::Xlsx),
            owned(&[("formula_error", Some(6), Some("/body/0/children/0/rows/1"))])
        );
    }

    #[test]
    fn markdown_keeps_lines_and_gains_paths() {
        let source = "---\npage_size: Tabloid\n---\n\n# A\n\n### Deep\n\n[x](#nope)\n";
        let output = crate::parse(source);
        let diagnostics = lint(output.document, &output.sidecar, Format::Docx);
        let found: Vec<_> = diagnostics
            .iter()
            .map(|d| (d.code.as_str(), d.line, d.column, d.path.as_deref()))
            .collect();
        assert_eq!(
            found,
            vec![
                ("invalid_page_size", Some(2), None, Some("/meta/page_size")),
                ("heading_skip", Some(7), None, Some("/body/0/children/0")),
                ("link_anchor_unresolved", Some(9), Some(1), None),
            ]
        );
    }

    #[test]
    fn pointer_tokens_are_escaped() {
        assert_eq!(escape("a/b~c"), "a~1b~0c");
    }
}
