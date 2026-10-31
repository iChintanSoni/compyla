//! Slide segmentation and slide structure (markdown-spec.md §8.1, §8.2).

use crate::ir::{Block, Directive, Section};
use crate::validate::{DirectiveKind, directive};

/// A node the slide walk reaches (§8.1). A section contributes its heading, then its children;
/// annotations other than `layout` and unknown directives contribute their children in their
/// place; ignored markers and containers contribute nothing (§1.5).
#[derive(Debug, Clone, Copy)]
pub enum Item<'a> {
    /// A section's heading: the slide title, or a demoted heading in the content.
    Heading(&'a Section),
    /// A paragraph, list, table, media, code block or quote.
    Block(&'a Block),
    /// A `col` marker (§8.4).
    Col(&'a Directive),
    /// A `layout` directive. The walk doesn't enter its body (§8.1).
    Layout(&'a Directive),
    /// A `notes` container. The walk doesn't enter its body (§8.1).
    Notes(&'a Directive),
}

/// One slide (§8.2).
#[derive(Debug)]
pub struct Slide<'a> {
    /// The first heading reached, including inside a leading `layout` directive.
    pub title: Option<&'a Section>,
    /// The leading `layout` directive: the slide's first content block (§8.3).
    pub layout: Option<&'a Directive>,
    /// Every other item reached, in order, with the leading `layout` unwrapped and `notes`
    /// removed: demoted headings, blocks, `col` markers and misplaced `layout` directives.
    pub content: Vec<Item<'a>>,
    /// The `notes` directives among the content's top-level blocks.
    pub notes: Vec<&'a Directive>,
}

impl Slide<'_> {
    /// Whether the slide has speaker notes: the `notes` children, concatenated, aren't empty.
    pub fn has_notes(&self) -> bool {
        self.notes.iter().any(|notes| !notes.children.is_empty())
    }

    fn is_empty(&self) -> bool {
        self.title.is_none() && self.content.is_empty() && !self.has_notes()
    }
}

/// A document segmented into slides.
pub struct Deck<'a> {
    pub slides: Vec<Slide<'a>>,
    /// Slides with no title, no content and no notes, which are omitted (§8.1). Only their
    /// directives matter: a leading `layout` on one has no effect.
    pub omitted: Vec<Slide<'a>>,
}

/// Segments the body into slides in document order (§8.1) and structures each one (§8.2).
pub fn deck(body: &[Block]) -> Deck<'_> {
    let mut walk = Walk::new(true);
    walk.blocks(body);
    let (slides, omitted) = walk
        .finish()
        .into_iter()
        .map(structure)
        .partition(|slide| !slide.is_empty());
    Deck { slides, omitted }
}

/// The walk of §8.1. With `split`, every `break` reached ends the current slide; without it (inside
/// a leading `layout`'s body, which segmentation doesn't enter) a `break` has no effect.
struct Walk<'a> {
    split: bool,
    done: Vec<Vec<Item<'a>>>,
    current: Vec<Item<'a>>,
}

impl<'a> Walk<'a> {
    fn new(split: bool) -> Self {
        Self {
            split,
            done: Vec::new(),
            current: Vec::new(),
        }
    }

    /// Whether a `layout` reached now would be the slide's leading one: nothing but notes and the
    /// title heading precede it (§8.3).
    fn leads(&self) -> bool {
        let mut title = false;
        for item in &self.current {
            match item {
                Item::Notes(_) => {}
                Item::Heading(_) if !title => title = true,
                _ => return false,
            }
        }
        true
    }

    fn finish(mut self) -> Vec<Vec<Item<'a>>> {
        self.done.push(self.current);
        self.done
    }

    fn blocks(&mut self, blocks: &'a [Block]) {
        for block in blocks {
            match block {
                Block::Section(section) => {
                    self.current.push(Item::Heading(section));
                    self.blocks(&section.children);
                }
                Block::Break(_) => {
                    if self.split {
                        self.done.push(std::mem::take(&mut self.current));
                    }
                }
                Block::Directive(d) => self.directive(d),
                // List items and quotes aren't entered: a `break` inside them has no effect.
                _ => self.current.push(Item::Block(block)),
            }
        }
    }

    fn directive(&mut self, d: &'a Directive) {
        match d.name.as_str() {
            "col" => self.current.push(Item::Col(d)),
            "layout" if self.split && self.leads() => self.current.push(Item::Layout(d)),
            // A `layout` that isn't leading is misplaced and so ignored (§1.5): its children count
            // in its place, like any ignored annotation's (§8.1, §8.6).
            "layout" => self.blocks(&d.children),
            "notes" => self.current.push(Item::Notes(d)),
            name => match directive(name) {
                None | Some((DirectiveKind::Annotation, _)) => self.blocks(&d.children),
                Some((DirectiveKind::Marker | DirectiveKind::Container, _)) => {}
            },
        }
    }
}

/// §8.2: the title, the content after unwrapping a leading `layout`, and the notes.
fn structure(items: Vec<Item<'_>>) -> Slide<'_> {
    let leading = leading(&items);
    let mut flat = Vec::with_capacity(items.len());
    for (index, item) in items.into_iter().enumerate() {
        match leading {
            // With children, those are the content; with none, the blocks after it are (§8.3).
            Some((at, layout)) if at == index => {
                let mut walk = Walk::new(false);
                walk.blocks(&layout.children);
                flat.extend(walk.finish().into_iter().flatten());
            }
            _ => flat.push(item),
        }
    }
    let mut slide = Slide {
        title: None,
        layout: leading.map(|(_, layout)| layout),
        content: Vec::new(),
        notes: Vec::new(),
    };
    for item in flat {
        match item {
            Item::Heading(section) if slide.title.is_none() => slide.title = Some(section),
            Item::Notes(notes) => slide.notes.push(notes),
            other => slide.content.push(other),
        }
    }
    slide
}

/// The leading `layout` directive and its index: the slide's first content block, so the first
/// item other than `notes` and the title heading (§8.3).
fn leading<'a>(items: &[Item<'a>]) -> Option<(usize, &'a Directive)> {
    let mut title = false;
    for (index, item) in items.iter().enumerate() {
        match *item {
            Item::Notes(_) => {}
            Item::Heading(_) if !title => title = true,
            Item::Layout(layout) => return Some((index, layout)),
            _ => return None,
        }
    }
    None
}
