//! The house layout set, layout names, block measurement, inference and placeholder fit
//! (markdown-spec.md §8.3, §8.5, §8.6).

use super::slides::Item;
use crate::ir::{Block, Position};

/// A layout of the house set (§8.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    TitleSlide,
    TitleAndContent,
    SectionHeader,
    TwoContent,
    Comparison,
    TitleOnly,
    Blank,
    ContentWithCaption,
    PictureWithCaption,
    Quote,
}

impl Layout {
    /// The house set, in the spec's order.
    pub const HOUSE: [Layout; 10] = [
        Layout::TitleSlide,
        Layout::TitleAndContent,
        Layout::SectionHeader,
        Layout::TwoContent,
        Layout::Comparison,
        Layout::TitleOnly,
        Layout::Blank,
        Layout::ContentWithCaption,
        Layout::PictureWithCaption,
        Layout::Quote,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Layout::TitleSlide => "Title Slide",
            Layout::TitleAndContent => "Title and Content",
            Layout::SectionHeader => "Section Header",
            Layout::TwoContent => "Two Content",
            Layout::Comparison => "Comparison",
            Layout::TitleOnly => "Title Only",
            Layout::Blank => "Blank",
            Layout::ContentWithCaption => "Content with Caption",
            Layout::PictureWithCaption => "Picture with Caption",
            Layout::Quote => "Quote",
        }
    }

    /// Resolves a normalized layout name (see [`normalize`]) case-insensitively. Without a
    /// template, the valid names are exactly the house set (§8.5).
    pub fn resolve(name: &str) -> Option<Layout> {
        let wanted = fold(name);
        Self::HOUSE
            .into_iter()
            .find(|layout| fold(layout.name()) == wanted)
    }

    /// The number of content slots (§8.6). `col` is meaningful only with two or more (§8.4).
    pub fn slots(self) -> usize {
        match self {
            Layout::SectionHeader | Layout::TitleOnly | Layout::Blank => 0,
            Layout::TitleSlide | Layout::TitleAndContent | Layout::Quote => 1,
            Layout::TwoContent | Layout::ContentWithCaption | Layout::PictureWithCaption => 2,
            Layout::Comparison => 4,
        }
    }

    /// What the layout holds, for messages.
    pub fn capacity(self) -> &'static str {
        match self {
            Layout::TitleSlide => "a title and a subtitle of at most one text block",
            Layout::SectionHeader | Layout::TitleOnly => "a title and no content",
            Layout::Blank => "no title and no content",
            Layout::TitleAndContent | Layout::Quote => "a title and one block",
            Layout::TwoContent => "two blocks, or two columns of one block each",
            Layout::Comparison => "two columns, each an optional heading text and one block",
            Layout::ContentWithCaption => "one content block and one text caption",
            Layout::PictureWithCaption => "one image and one text caption",
        }
    }

    /// Infers a layout from the content (§8.3, priority 3): the first match wins.
    pub fn infer(measure: &Measure) -> Layout {
        let blocks = &measure.blocks;
        let only = |kind| blocks.len() == 1 && blocks[0].kind == kind;
        let media = blocks.iter().filter(|b| b.kind == Kind::Media).count();
        if blocks.is_empty() && !measure.columns {
            Layout::SectionHeader
        } else if measure.columns {
            Layout::TwoContent
        } else if only(Kind::Table) {
            Layout::TitleAndContent
        } else if media == 1
            && blocks.len() <= 2
            && blocks
                .iter()
                .all(|b| matches!(b.kind, Kind::Media | Kind::Text))
        {
            Layout::PictureWithCaption
        } else if only(Kind::Quote) {
            Layout::Quote
        } else if blocks.len() == 2 {
            Layout::TwoContent
        } else {
            Layout::TitleAndContent
        }
    }

    /// The first part of a slide that doesn't fit this layout's slots (§8.6), or `None` when
    /// everything fits.
    pub fn misfit(self, has_title: bool, measure: &Measure) -> Option<Misfit> {
        if self == Layout::Blank && has_title {
            return Some(Misfit::Title);
        }
        let mut fill = Fill::default();
        measure
            .blocks
            .iter()
            .find(|block| !fill.admit(self, measure.columns, block))
            .map(|block| Misfit::Block(block.position))
    }
}

/// `<name>` of `::: layout <name>`: the args, trimmed, internal whitespace collapsed (§8.3).
pub fn normalize(args: &str) -> String {
    args.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The valid layout names, for messages.
pub fn names() -> String {
    Layout::HOUSE.map(Layout::name).join(", ")
}

/// Case folding for comparing names with the house set (§8.5). The house names are ASCII, so
/// lowercasing plus the one non-ASCII letter whose simple case folding is an ASCII letter but
/// whose lowercase isn't (U+017F, long s) matches exactly what simple case folding matches.
fn fold(name: &str) -> String {
    name.chars()
        .flat_map(char::to_lowercase)
        .map(|c| if c == '\u{17F}' { 's' } else { c })
        .collect()
}

/// What doesn't fit a layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Misfit {
    /// The slide has a title, and the layout has no title (Blank).
    Title,
    /// The first block that doesn't fit, at its position.
    Block(Option<Position>),
}

/// The kinds of block that fit decisions distinguish.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Consecutive paragraphs, lists and demoted headings.
    Text,
    Table,
    Media,
    Quote,
    /// A code block.
    Other,
}

/// One measured block (§8.6).
#[derive(Debug, Clone, Copy)]
pub struct Measured {
    pub kind: Kind,
    /// The position of the block's first node.
    pub position: Option<Position>,
    /// The column it is in: the number of `col` markers before it.
    pub column: usize,
}

/// A slide's content measured in blocks (§8.6).
#[derive(Debug)]
pub struct Measure {
    pub blocks: Vec<Measured>,
    /// Whether `col` markers separate the content into columns.
    pub columns: bool,
}

impl Measure {
    /// Consecutive paragraphs, lists and demoted headings form one text block; every other node
    /// is one block; a `col` marker ends the current text block and separates columns. With
    /// `columns` false, `col` markers are misplaced and so ignored, as if absent (§1.5, §8.4).
    pub fn new(content: &[Item<'_>], columns: bool) -> Measure {
        let mut measure = Measure {
            blocks: Vec::new(),
            columns: false,
        };
        let (mut column, mut in_text) = (0, false);
        for item in content {
            let (kind, position) = match *item {
                Item::Col(_) => {
                    if columns {
                        measure.columns = true;
                        column += 1;
                        in_text = false;
                    }
                    continue;
                }
                Item::Heading(section) => (Kind::Text, section.position),
                Item::Block(block) => (kind(block), block.position()),
                Item::Layout(layout) => (Kind::Other, layout.position),
                // Removed from the content (§8.2); never measured.
                Item::Notes(_) => continue,
            };
            let text = kind == Kind::Text;
            if text && in_text {
                continue; // joins the current text block
            }
            in_text = text;
            measure.blocks.push(Measured {
                kind,
                position,
                column,
            });
        }
        measure
    }
}

fn kind(block: &Block) -> Kind {
    match block {
        Block::Paragraph(_) | Block::List(_) => Kind::Text,
        Block::Table(_) => Kind::Table,
        Block::Media(_) => Kind::Media,
        Block::Quote(_) => Kind::Quote,
        _ => Kind::Other,
    }
}

/// The slots filled so far while checking fit, block by block in order (§8.6).
#[derive(Default)]
struct Fill {
    count: usize,
    text: bool,
    media: bool,
    other: bool,
    columns: [Column; 2],
}

/// A Two Content or Comparison column's state.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum Column {
    #[default]
    Empty,
    /// Comparison: the column's heading holds a text block.
    Heading,
    Full,
}

impl Fill {
    /// Whether `block` fits in the slots left, filling one if it does.
    fn admit(&mut self, layout: Layout, columns: bool, block: &Measured) -> bool {
        self.count += 1;
        let text = block.kind == Kind::Text;
        match layout {
            Layout::SectionHeader | Layout::TitleOnly | Layout::Blank => false,
            Layout::TitleSlide => text && first(&mut self.text),
            Layout::TitleAndContent => self.count == 1,
            Layout::Quote => self.count == 1 && matches!(block.kind, Kind::Quote | Kind::Text),
            Layout::ContentWithCaption if text => first(&mut self.text),
            Layout::ContentWithCaption => first(&mut self.other),
            Layout::PictureWithCaption => match block.kind {
                Kind::Text => first(&mut self.text),
                Kind::Media => first(&mut self.media),
                _ => false,
            },
            // Without `col` markers, Comparison is as Two Content: the first block goes left,
            // the second right.
            Layout::TwoContent | Layout::Comparison if !columns => self.count <= 2,
            Layout::TwoContent | Layout::Comparison => {
                let Some(column) = self.columns.get_mut(block.column) else {
                    return false;
                };
                match (*column, layout) {
                    // A Comparison column is an optional text block (its heading), then one block.
                    (Column::Empty, Layout::Comparison) if text => *column = Column::Heading,
                    (Column::Empty | Column::Heading, Layout::Comparison) => *column = Column::Full,
                    (Column::Empty, _) => *column = Column::Full,
                    _ => return false,
                }
                true
            }
        }
    }
}

/// True the first time, for a slot that holds one block.
fn first(filled: &mut bool) -> bool {
    !std::mem::replace(filled, true)
}
