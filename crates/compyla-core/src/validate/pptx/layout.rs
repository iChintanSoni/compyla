//! The house layout set, layout names, block measurement, inference and placeholder fit
//! (markdown-spec.md §8.3, §8.5, §8.6).

use super::slides::Item;
use crate::ir::{Block, Position};
use crate::validate::fold;

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
    /// A supplied template's own layout, outside the house set (§8.5): one slot per content
    /// placeholder, each holding one block (§8.6).
    Own {
        slots: usize,
    },
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
            // Named by the template: messages use the bound layout's name ([`Bound`]).
            Layout::Own { .. } => "template",
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

    /// The OOXML layout type a house layout binds to in a template without its name (§8.5).
    pub fn kind(self) -> Option<&'static str> {
        match self {
            Layout::TitleSlide => Some("title"),
            Layout::TitleAndContent => Some("obj"),
            Layout::SectionHeader => Some("secHead"),
            Layout::TwoContent => Some("twoObj"),
            Layout::Comparison => Some("twoTxTwoObj"),
            Layout::TitleOnly => Some("titleOnly"),
            Layout::Blank => Some("blank"),
            Layout::ContentWithCaption => Some("objTx"),
            Layout::PictureWithCaption => Some("picTx"),
            Layout::Quote | Layout::Own { .. } => None,
        }
    }

    /// The number of content slots (§8.6). `col` is meaningful only with two or more (§8.4).
    pub fn slots(self) -> usize {
        match self {
            Layout::SectionHeader | Layout::TitleOnly | Layout::Blank => 0,
            Layout::TitleSlide | Layout::TitleAndContent | Layout::Quote => 1,
            Layout::TwoContent | Layout::ContentWithCaption | Layout::PictureWithCaption => 2,
            Layout::Comparison => 4,
            Layout::Own { slots } => slots,
        }
    }

    /// What the layout holds, for messages.
    pub fn capacity(self) -> String {
        match self {
            Layout::TitleSlide => "a title and a subtitle of at most one text block".into(),
            Layout::SectionHeader | Layout::TitleOnly => "a title and no content".into(),
            Layout::Blank => "no title and no content".into(),
            Layout::TitleAndContent | Layout::Quote => "a title and one block".into(),
            Layout::TwoContent => "two blocks, or two columns of one block each".into(),
            Layout::Comparison => "two columns, each an optional heading text and one block".into(),
            Layout::ContentWithCaption => "one content block and one text caption".into(),
            Layout::PictureWithCaption => "one image and one text caption".into(),
            Layout::Own { slots: 0 } => "a title and no content".into(),
            Layout::Own { slots: 1 } => "a title and one block".into(),
            Layout::Own { slots } => format!("a title and {slots} blocks, one per placeholder"),
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

/// A supplied template's layouts, as binding sees them (§8.5): in master order, each once.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TemplateLayouts {
    pub layouts: Vec<TemplateLayout>,
    /// `layoutAliases` (§10.2): each a layout name, and the index of the template layout it names.
    pub aliases: Vec<(String, usize)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateLayout {
    pub name: String,
    /// Its OOXML type (`p:sldLayout/@type`).
    pub kind: Option<String>,
    /// Its content placeholders (§8.8): any but the title, date, footer, header and slide number.
    pub content: usize,
}

/// A layout as bound (§8.5): the slots it fills, the name messages give it, and the template
/// layout it is written into (none with the house template).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bound {
    pub layout: Layout,
    pub name: String,
    pub template: Option<usize>,
    /// Bound by the last fallback, the template having no layout for it (`layout_not_in_template`).
    pub fallback: bool,
}

impl Bound {
    /// A house layout with the house template.
    pub fn house(layout: Layout) -> Bound {
        Bound {
            layout,
            name: layout.name().into(),
            template: None,
            fallback: false,
        }
    }
}

impl TemplateLayouts {
    /// A requested layout name (normalized) as bound (§8.5): an alias, a template layout's own
    /// name, or a house-set name; `None` for any other name (`unknown_layout`).
    pub fn resolve(&self, name: &str) -> Option<Bound> {
        let wanted = fold(name);
        let house = Layout::resolve(name);
        if let Some(index) = self.alias(&wanted) {
            return Some(self.bound(house, index, false));
        }
        if let Some(index) = self.layouts.iter().position(|l| fold(&l.name) == wanted) {
            return Some(self.bound(house, index, false));
        }
        house.map(|layout| self.bind(layout))
    }

    /// A house layout as bound (§8.5): by alias, by name, by OOXML type, else the template's Title
    /// and Content layout (type `obj`) or its first, as a fallback.
    pub fn bind(&self, layout: Layout) -> Bound {
        if self.layouts.is_empty() {
            // Nothing to bind to: the facade refuses such a template (`template_invalid`).
            return Bound::house(layout);
        }
        let wanted = fold(layout.name());
        let of_kind = |kind: &str| {
            self.layouts
                .iter()
                .position(|l| l.kind.as_deref() == Some(kind))
        };
        let found = self
            .alias(&wanted)
            .or_else(|| self.layouts.iter().position(|l| fold(&l.name) == wanted))
            .or_else(|| layout.kind().and_then(of_kind));
        match found {
            Some(index) => self.bound(Some(layout), index, false),
            None => self.bound(Some(layout), of_kind("obj").unwrap_or(0), true),
        }
    }

    fn alias(&self, wanted: &str) -> Option<usize> {
        self.aliases
            .iter()
            .find(|(alias, _)| fold(alias) == wanted)
            .map(|&(_, index)| index)
    }

    /// The template layout at `index`, filled as `house` when the name is in the house set, and
    /// otherwise as the template layout's own (§8.6).
    fn bound(&self, house: Option<Layout>, index: usize, fallback: bool) -> Bound {
        let template = &self.layouts[index];
        match house {
            Some(layout) => Bound {
                layout,
                name: layout.name().into(),
                template: Some(index),
                fallback,
            },
            None => Bound {
                layout: Layout::Own {
                    slots: template.content,
                },
                name: template.name.clone(),
                template: Some(index),
                fallback,
            },
        }
    }

    /// The valid layout names, for messages: the house set, then the template's own and aliases,
    /// each once.
    pub fn names(&self) -> String {
        let house = Layout::HOUSE.iter().map(|l| l.name());
        let own = self.layouts.iter().map(|l| l.name.as_str());
        let aliases = self.aliases.iter().map(|(a, _)| a.as_str());
        let mut seen = std::collections::HashSet::new();
        let names: Vec<&str> = house
            .chain(own)
            .chain(aliases)
            .filter(|name| seen.insert(fold(name)))
            .collect();
        names.join(", ")
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
            Layout::Own { slots } => self.count <= slots,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn template() -> TemplateLayouts {
        let layout = |name: &str, kind: Option<&str>, content| TemplateLayout {
            name: name.into(),
            kind: kind.map(str::to_string),
            content,
        };
        TemplateLayouts {
            layouts: vec![
                layout("Titelfolie", Some("title"), 1),
                layout("Titel und Inhalt", Some("obj"), 1),
                layout("Agenda", None, 3),
                layout("Blank", Some("blank"), 0),
            ],
            aliases: vec![("Quote".into(), 2), ("Kunden".into(), 3)],
        }
    }

    #[test]
    fn house_layouts_bind_by_alias_name_type_then_fallback() {
        let template = template();
        let bound = |layout| {
            let b = template.bind(layout);
            (b.template, b.fallback)
        };
        assert_eq!(bound(Layout::Quote), (Some(2), false), "alias");
        assert_eq!(bound(Layout::Blank), (Some(3), false), "name");
        assert_eq!(bound(Layout::TitleSlide), (Some(0), false), "type");
        assert_eq!(bound(Layout::Comparison), (Some(1), true), "fallback: obj");
        // An aliased house name keeps its slots.
        assert_eq!(template.bind(Layout::Quote).layout, Layout::Quote);
    }

    #[test]
    fn names_resolve_to_the_templates_own_layouts() {
        let template = template();
        let agenda = template.resolve("agenda").unwrap();
        assert_eq!(
            (agenda.layout, agenda.name.as_str(), agenda.template),
            (Layout::Own { slots: 3 }, "Agenda", Some(2))
        );
        // An alias to a template layout, under a name of its own.
        assert_eq!(template.resolve("KUNDEN").unwrap().template, Some(3));
        // U+212A KELVIN SIGN folds to `k`: "Blan\u{212A}" is Blank.
        assert_eq!(template.resolve("Blan\u{212A}").unwrap().template, Some(3));
        assert_eq!(template.resolve("Fancy"), None);
        assert!(
            template
                .names()
                .ends_with("Quote, Titelfolie, Titel und Inhalt, Agenda, Kunden")
        );
    }

    #[test]
    fn a_template_without_obj_falls_back_to_its_first_layout() {
        let template = TemplateLayouts {
            layouts: vec![TemplateLayout {
                name: "Only".into(),
                kind: None,
                content: 2,
            }],
            aliases: Vec::new(),
        };
        let bound = template.bind(Layout::TwoContent);
        assert_eq!((bound.template, bound.fallback), (Some(0), true));
    }
}
