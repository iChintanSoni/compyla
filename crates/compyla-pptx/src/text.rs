//! DrawingML text (markdown-spec.md §8.6): the paragraphs a text block, code, a quote or the
//! speaker notes become.

use compyla_core::ir::{Block, List, Run};
use compyla_core::validate::pptx::model::Piece;
use compyla_ooxml::xml::Writer;

/// The typeface code is set in.
const MONO: &str = "Consolas";

/// A link's relationship id, and for a jump to a slide, the action.
pub type LinkTarget = (String, Option<&'static str>);

/// Registers a link destination for the current part, or `None` when it goes nowhere (an
/// unresolved fragment).
pub type Links<'a> = dyn FnMut(&str) -> Option<LinkTarget> + 'a;

/// How list items show their bullets.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Bullets {
    /// From the placeholder's level styles: a content placeholder's own look.
    Inherit,
    /// Written out, for placeholders whose styles have none (subtitles, captions, notes).
    Explicit,
}

/// One paragraph's style.
#[derive(Clone, Copy, Default)]
struct Style {
    level: usize,
    bullet: Bullet,
    bold: bool,
    italic: bool,
    mono: bool,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Bullet {
    /// No bullet, text at the margin.
    #[default]
    None,
    /// A list item's continuation: no bullet, text aligned with the item's.
    Continue,
    Char,
    Number(u64),
}

pub struct Text<'w, 'l> {
    pub out: &'w mut Writer,
    pub links: &'w mut Links<'l>,
    pub bullets: Bullets,
    /// Whether anything was written, so an empty body can still hold one paragraph.
    pub wrote: bool,
}

impl Text<'_, '_> {
    /// A text block's pieces (§8.6).
    pub fn pieces(&mut self, pieces: &[Piece]) {
        for piece in pieces {
            match piece {
                Piece::Paragraph(runs) => self.paragraph(runs, Style::default()),
                Piece::Heading(runs) => self.paragraph(
                    runs,
                    Style {
                        bold: true,
                        ..Style::default()
                    },
                ),
                Piece::List(list) => self.list(list, 0, false),
            }
        }
    }

    /// A code block: one paragraph per line, in the monospace typeface.
    pub fn code(&mut self, value: &str, level: usize, bullet_continue: bool) {
        let style = Style {
            level,
            bullet: if bullet_continue {
                Bullet::Continue
            } else {
                Bullet::None
            },
            mono: true,
            ..Style::default()
        };
        for line in value.split('\n') {
            self.paragraph(&[Run::plain(line.to_string())], style);
        }
    }

    /// Blocks inside a quote, a list item or the notes: each as text, in `style`.
    pub fn blocks(&mut self, blocks: &[Block], level: usize, italic: bool, in_item: bool) {
        let base = Style {
            level,
            italic,
            bullet: if in_item {
                Bullet::Continue
            } else {
                Bullet::None
            },
            ..Style::default()
        };
        for block in blocks {
            match block {
                Block::Paragraph(p) => self.paragraph(&p.runs, base),
                Block::Section(s) => {
                    self.paragraph(&s.title, Style { bold: true, ..base });
                    self.blocks(&s.children, level, italic, in_item);
                }
                Block::List(list) => self.list(list, level + usize::from(in_item), italic),
                Block::Code(code) => self.code(&code.value, level, in_item),
                Block::Quote(quote) => self.blocks(&quote.children, level, true, in_item),
                // Tables as tab-separated rows, media as their alt text (§8.2).
                Block::Table(table) => {
                    for row in std::iter::once(&table.header).chain(&table.rows) {
                        let cells: Vec<String> = row
                            .iter()
                            .map(|cell| compyla_core::ir::plain_text(&cell.runs))
                            .collect();
                        self.paragraph(&[Run::plain(cells.join("\t"))], base);
                    }
                }
                Block::Media(media) => self.paragraph(&[alt(&media.alt)], base),
                Block::Directive(d) => self.blocks(&d.children, level, italic, in_item),
                Block::Break(_) => {}
            }
        }
    }

    fn list(&mut self, list: &List, level: usize, italic: bool) {
        let level = level.min(8);
        for (index, item) in list.items.iter().enumerate() {
            let bullet = if list.ordered {
                Bullet::Number(list.start.unwrap_or(1) + index as u64)
            } else {
                Bullet::Char
            };
            let mut first = true;
            for block in &item.children {
                match block {
                    Block::Paragraph(p) if first => {
                        let mut runs = p.runs.clone();
                        if let Some(checked) = item.checked {
                            let mark = if checked { "☑ " } else { "☐ " };
                            runs.insert(0, Run::plain(mark.into()));
                        }
                        self.paragraph(
                            &runs,
                            Style {
                                level,
                                bullet,
                                italic,
                                ..Style::default()
                            },
                        );
                    }
                    other => self.blocks(std::slice::from_ref(other), level, italic, true),
                }
                first = false;
            }
        }
    }

    fn paragraph(&mut self, runs: &[Run], style: Style) {
        self.wrote = true;
        let out = &mut *self.out;
        out.open("a:p", &[]);
        let level = style.level.to_string();
        let explicit = self.bullets == Bullets::Explicit;
        // Explicit levels: text at 0.5 inch per level, a bullet hanging 0.25 inch before it.
        let margin = (342_900 + 457_200 * style.level as i64).to_string();
        match style.bullet {
            Bullet::None => {
                let mut attrs = vec![("marL", "0"), ("indent", "0")];
                if style.level > 0 {
                    attrs.insert(0, ("lvl", level.as_str()));
                }
                out.open("a:pPr", &attrs);
                out.empty("a:buNone", &[]);
                out.close("a:pPr");
            }
            Bullet::Continue => {
                let mut attrs = vec![("lvl", level.as_str()), ("indent", "0")];
                if explicit {
                    attrs.push(("marL", margin.as_str()));
                }
                out.open("a:pPr", &attrs);
                out.empty("a:buNone", &[]);
                out.close("a:pPr");
            }
            Bullet::Char if !explicit => out.empty("a:pPr", &[("lvl", &level)]),
            Bullet::Char => {
                out.open(
                    "a:pPr",
                    &[("lvl", &level), ("marL", &margin), ("indent", "-342900")],
                );
                out.empty("a:buFont", &[("typeface", "Arial")]);
                out.empty("a:buChar", &[("char", "•")]);
                out.close("a:pPr");
            }
            Bullet::Number(start) => {
                let mut attrs = vec![("lvl", level.as_str())];
                if explicit {
                    attrs.extend([("marL", margin.as_str()), ("indent", "-342900")]);
                }
                out.open("a:pPr", &attrs);
                out.empty("a:buFont", &[("typeface", "+mj-lt")]);
                let start = start.to_string();
                out.empty(
                    "a:buAutoNum",
                    &[("type", "arabicPeriod"), ("startAt", &start)],
                );
                out.close("a:pPr");
            }
        }
        for run in runs {
            let mut lines = run.text.split('\n').peekable();
            while let Some(line) = lines.next() {
                if !line.is_empty() {
                    self.run(line, run, style);
                }
                if lines.peek().is_some() {
                    self.out.empty("a:br", &[]);
                }
            }
        }
        self.out.close("a:p");
    }

    fn run(&mut self, text: &str, run: &Run, style: Style) {
        let link = run.link.as_deref().and_then(|url| (self.links)(url));
        let out = &mut *self.out;
        out.open("a:r", &[]);
        let mut attrs = vec![("lang", "en-US")];
        if run.bold || style.bold {
            attrs.push(("b", "1"));
        }
        if run.italic || style.italic {
            attrs.push(("i", "1"));
        }
        if run.strike {
            attrs.push(("strike", "sngStrike"));
        }
        attrs.push(("dirty", "0"));
        out.open("a:rPr", &attrs);
        if run.code || style.mono {
            out.empty("a:latin", &[("typeface", MONO)]);
        }
        if let Some((id, action)) = &link {
            let mut attrs = vec![("r:id", id.as_str())];
            if let Some(action) = action {
                attrs.push(("action", action));
            }
            out.empty("a:hlinkClick", &attrs);
        }
        out.close("a:rPr");
        out.element("a:t", &[], text);
        out.close("a:r");
    }

    /// One paragraph, with no bullet, aligned (`l`, `ctr`, `r`) when `align` is given: a table cell
    /// or an image's alt text.
    pub fn cell(&mut self, runs: &[Run], align: Option<&'static str>) {
        self.wrote = true;
        self.out.open("a:p", &[]);
        if let Some(align) = align {
            self.out.empty("a:pPr", &[("algn", align)]);
        }
        for run in runs {
            let mut lines = run.text.split('\n').peekable();
            while let Some(line) = lines.next() {
                if !line.is_empty() {
                    self.run(line, run, Style::default());
                }
                if lines.peek().is_some() {
                    self.out.empty("a:br", &[]);
                }
            }
        }
        self.out.close("a:p");
    }

    /// An empty paragraph, so a text body is never empty.
    pub fn finish(self) {
        if !self.wrote {
            self.out.empty("a:p", &[]);
        }
    }
}

/// An image that can't be shown, as text (§5): its alt text in brackets, or `[image]`.
pub fn alt(alt: &str) -> Run {
    let text = if alt.trim().is_empty() {
        "[image]".to_string()
    } else {
        format!("[{alt}]")
    };
    Run {
        italic: true,
        ..Run::plain(text)
    }
}
