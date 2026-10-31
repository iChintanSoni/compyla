//! The composed deck (architecture.md D15): what the pptx renderer writes, resolved from the slides
//! and their layouts (markdown-spec.md §8.2–§8.6). Content is owned, so the deck outlives the
//! document it came from.

use std::collections::HashMap;

use super::Composed;
use super::layout::Layout;
use super::slides::Item;
use crate::ir::{Block, Code, Directive, List, Media, Quote, Run, Table};
use crate::validate::{Slugger, options, title};

/// A composed deck.
#[derive(Debug, Clone, PartialEq)]
pub struct Deck {
    pub slides: Vec<Slide>,
    /// Slide-title slugs → the slide's index, for fragment links (§8.7).
    pub anchors: HashMap<String, usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Slide {
    pub layout: Layout,
    /// The supplied template's layout the slide is written into (§8.5), by its index in the
    /// template's layouts; `None` with the house template, which the renderer binds itself.
    pub template: Option<usize>,
    pub title: Option<Vec<Run>>,
    /// The content, slot by slot in the layout's order (§8.6). Content that fits no slot is in the
    /// last one; a layout without slots gets one slot for content it can't hold (§8.8).
    pub slots: Vec<Vec<Content>>,
    /// The speaker notes: the `notes` directives' children, in order (§8.2).
    pub notes: Vec<Block>,
}

/// One block of content (§8.6).
#[derive(Debug, Clone, PartialEq)]
pub enum Content {
    /// Consecutive paragraphs, lists and demoted headings.
    Text(Vec<Piece>),
    Table(Table),
    Media(Media, Fit),
    Code(Code),
    Quote(Quote),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Piece {
    Paragraph(Vec<Run>),
    List(List),
    /// A demoted heading (§8.2).
    Heading(Vec<Run>),
}

/// How an image fills its placeholder (§8.4): whole inside it, or covering it, cropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Fit {
    #[default]
    Contain,
    Cover,
}

pub(super) fn deck(composed: &[Composed<'_>], body: &[Block]) -> Deck {
    let fits = fits(body);
    let mut slugger = Slugger::new();
    let mut anchors = HashMap::new();
    let mut slides = Vec::with_capacity(composed.len());
    for (index, composed) in composed.iter().enumerate() {
        let slide = &composed.slide;
        if let Some(section) = slide.title {
            anchors.insert(slugger.slug(&title(&section.title)), index);
        }
        slides.push(Slide {
            layout: composed.layout,
            template: composed.bound.template,
            title: slide.title.map(|section| section.title.clone()),
            slots: slots(composed, &fits),
            notes: slide
                .notes
                .iter()
                .flat_map(|notes| notes.children.iter().cloned())
                .collect(),
        });
    }
    Deck { slides, anchors }
}

/// The blocks of a slide's content, each with its column (§8.4, §8.6).
fn blocks(composed: &Composed<'_>, fits: &HashMap<*const Media, Fit>) -> Vec<(usize, Content)> {
    let columns = composed.measure.columns;
    let (mut out, mut column) = (Vec::new(), 0);
    let mut text: Vec<Piece> = Vec::new();
    let flush = |text: &mut Vec<Piece>, out: &mut Vec<(usize, Content)>, column: usize| {
        if !text.is_empty() {
            out.push((column, Content::Text(std::mem::take(text))));
        }
    };
    for item in &composed.slide.content {
        let block = match *item {
            Item::Col(_) => {
                if columns {
                    flush(&mut text, &mut out, column);
                    column += 1;
                }
                continue;
            }
            Item::Heading(section) => {
                text.push(Piece::Heading(section.title.clone()));
                continue;
            }
            Item::Block(Block::Paragraph(p)) => {
                text.push(Piece::Paragraph(p.runs.clone()));
                continue;
            }
            Item::Block(Block::List(l)) => {
                text.push(Piece::List(l.clone()));
                continue;
            }
            Item::Block(Block::Table(t)) => Content::Table(t.clone()),
            Item::Block(Block::Media(m)) => {
                let fit = fits
                    .get(&std::ptr::from_ref(m))
                    .copied()
                    .unwrap_or_default();
                Content::Media(m.clone(), fit)
            }
            Item::Block(Block::Code(c)) => Content::Code(c.clone()),
            Item::Block(Block::Quote(q)) => Content::Quote(q.clone()),
            // The walk turns sections, breaks and directives into other items; a leading layout
            // is unwrapped and notes are removed (§8.2).
            Item::Block(_) | Item::Layout(_) | Item::Notes(_) => continue,
        };
        flush(&mut text, &mut out, column);
        out.push((column, block));
    }
    flush(&mut text, &mut out, column);
    out
}

/// What a slot holds (§8.6).
#[derive(Clone, Copy)]
enum Accepts {
    Text,
    NonText,
    Media,
    QuoteOrText,
    Any,
    Nothing,
}

impl Accepts {
    fn admits(self, content: &Content) -> bool {
        let text = matches!(content, Content::Text(_));
        match self {
            Accepts::Text => text,
            Accepts::NonText => !text,
            Accepts::Media => matches!(content, Content::Media(..)),
            Accepts::QuoteOrText => text || matches!(content, Content::Quote(_)),
            Accepts::Any => true,
            Accepts::Nothing => false,
        }
    }
}

/// §8.6: each block goes to the first empty slot that holds its kind; a block that fits none goes
/// to the last slot. With `col` markers, a column's blocks go to its slots.
fn slots(composed: &Composed<'_>, fits: &HashMap<*const Media, Fit>) -> Vec<Vec<Content>> {
    let layout = composed.layout;
    let blocks = blocks(composed, fits);
    // A template's own layout can have any number of placeholders, filled in order: slots past the
    // content stay empty, so they aren't kept. House layouts have at most four, placed by column.
    let count = match layout {
        Layout::Own { slots } => slots.min(blocks.len().max(1)),
        _ => layout.slots(),
    };
    if layout.slots() == 0 {
        // Content a layout has no slot for (`layout_overflow`) is still shown, never dropped.
        let overflow: Vec<Content> = blocks.into_iter().map(|(_, c)| c).collect();
        return if overflow.is_empty() {
            Vec::new()
        } else {
            vec![overflow]
        };
    }
    let mut slots: Vec<Vec<Content>> = vec![Vec::new(); count];
    let columns = composed.measure.columns;
    let accepts: Vec<Accepts> = match layout {
        Layout::TitleSlide => vec![Accepts::Text],
        Layout::Quote => vec![Accepts::QuoteOrText],
        Layout::ContentWithCaption => vec![Accepts::NonText, Accepts::Text],
        Layout::PictureWithCaption => vec![Accepts::Media, Accepts::Text],
        // Without columns, Comparison is as Two Content: its content slots, left then right.
        Layout::Comparison if !columns => {
            vec![
                Accepts::Nothing,
                Accepts::Any,
                Accepts::Nothing,
                Accepts::Any,
            ]
        }
        _ => vec![Accepts::Any; count],
    };
    for (column, content) in blocks {
        let slot = match layout {
            Layout::TwoContent if columns => column.min(1),
            Layout::Comparison if columns => {
                let base = 2 * column.min(1);
                let heading_free = slots[base].is_empty() && slots[base + 1].is_empty();
                if heading_free && matches!(content, Content::Text(_)) {
                    base
                } else {
                    base + 1
                }
            }
            _ => (0..count)
                .find(|&i| slots[i].is_empty() && accepts[i].admits(&content))
                .unwrap_or(count - 1),
        };
        slots[slot].push(content);
    }
    slots
}

/// The fit `::: image fit=…` gives each image it applies to: the images it wraps, or, when empty,
/// the immediately following media block (§8.4).
fn fits(body: &[Block]) -> HashMap<*const Media, Fit> {
    let mut out = HashMap::new();
    walk(body, &mut out);
    out
}

fn walk(blocks: &[Block], out: &mut HashMap<*const Media, Fit>) {
    for (index, block) in blocks.iter().enumerate() {
        match block {
            Block::Directive(d) if d.name == "image" => {
                if let Some(fit) = fit_of(d) {
                    if d.children.is_empty() {
                        if let Some(Block::Media(media)) = blocks.get(index + 1) {
                            out.insert(std::ptr::from_ref(media), fit);
                        }
                    } else {
                        crate::validate::for_each_block(&d.children, &mut |b| {
                            if let Block::Media(media) = b {
                                out.insert(std::ptr::from_ref(media), fit);
                            }
                        });
                    }
                }
                walk(&d.children, out);
            }
            Block::Section(s) => walk(&s.children, out),
            Block::Directive(d) => walk(&d.children, out),
            Block::Quote(q) => walk(&q.children, out),
            Block::List(l) => {
                for item in &l.items {
                    walk(&item.children, out);
                }
            }
            _ => {}
        }
    }
}

fn fit_of(directive: &Directive) -> Option<Fit> {
    let parsed = options(&directive.args);
    if parsed.repeated.is_some() {
        return None;
    }
    parsed
        .entries
        .iter()
        .find(|(key, _)| key == "fit")
        .and_then(
            |(_, value)| match value.as_deref()?.to_ascii_lowercase().as_str() {
                "contain" => Some(Fit::Contain),
                "cover" => Some(Fit::Cover),
                _ => None,
            },
        )
}
