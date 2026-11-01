//! The text model compose measures with (markdown-spec.md §8.6, C63): each box a layout's text
//! goes into, its insets and autofit, and the styles its paragraphs inherit there, as a template
//! gives them. Core reads no template (architecture.md D11): `compyla_ooxml::text::Model`
//! implements [`TemplateText`]. Typefaces are shared strings: a template's can be any length, and
//! every box resolved copies its levels.

use std::sync::Arc;

/// ST_Coordinate's and ST_PositiveCoordinate's bounds (ECMA-376 Part 1, §20.1.10.16, §20.1.10.42):
/// a template's geometry is held to them, so arithmetic on it can't overflow.
pub const MIN_COORDINATE: i64 = -27_273_042_329_600;
pub const MAX_COORDINATE: i64 = 27_273_042_316_900;

/// A rectangle in EMU.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i64,
    pub y: i64,
    pub cx: i64,
    pub cy: i64,
}

impl Rect {
    /// The rectangle held to the schema's bounds, as it is written: geometry derived from a
    /// template's own (a slot's share, a centered picture) can pass them.
    pub fn bounded(self) -> Rect {
        Rect {
            x: self.x.clamp(MIN_COORDINATE, MAX_COORDINATE),
            y: self.y.clamp(MIN_COORDINATE, MAX_COORDINATE),
            cx: self.cx.clamp(0, MAX_COORDINATE),
            cy: self.cy.clamp(0, MAX_COORDINATE),
        }
    }
}

/// A template's text model, resolved as compose asks for it (§8.6): a template can hold far more
/// layouts and placeholders than a deck uses, so a box is read only when it is measured. `layout`
/// indexes `TemplateLayouts`; past them, a box is §8.6's defaults alone, nothing from the template.
pub trait TemplateText {
    /// The box a slide's title is set in: the layout's title placeholder, or on a layout without
    /// one, the master's title placeholder, which a title of its own is placed over (§8.8).
    fn title(&self, layout: usize) -> TextBox;
    /// The box slot `slot` is set in: the layout's `slot`-th content placeholder, in shape order,
    /// where it has one.
    fn content(&self, layout: usize, slot: usize) -> Option<TextBox>;
    /// The master's body area, which free shapes are laid out over (§8.8).
    fn master_body(&self, layout: usize) -> Option<Rect>;
    /// A text box's paragraph levels: only `p:defaultTextStyle`.
    fn text_box(&self, layout: usize) -> Levels;
    /// A table cell's first level in the header row, and in the others: its table style's text,
    /// then the master's `p:otherStyle`, then `p:defaultTextStyle`.
    fn table(&self, layout: usize) -> (Level, Level);
}

/// A box text is set in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextBox {
    /// Its geometry, where the template gives one.
    pub rect: Option<Rect>,
    pub body: Body,
    pub levels: Levels,
}

/// Paragraph levels 1 to 9.
pub type Levels = [Level; 9];

/// A box's body properties (`a:bodyPr`), as it inherits them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Body {
    pub insets: Insets,
    /// Whether lines wrap (`wrap="square"`, the default) or break only at line breaks.
    pub wrap: bool,
    pub autofit: Autofit,
    /// Whether it is measured at all: not with several columns or vertical text.
    pub measured: bool,
}

impl Default for Body {
    fn default() -> Body {
        Body {
            insets: Insets::default(),
            wrap: true,
            autofit: Autofit::None,
            measured: true,
        }
    }
}

/// A box's insets in EMU.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Insets {
    pub left: i64,
    pub top: i64,
    pub right: i64,
    pub bottom: i64,
}

impl Default for Insets {
    /// 91,440 EMU left and right and 45,720 EMU top and bottom.
    fn default() -> Insets {
        Insets {
            left: 91_440,
            top: 45_720,
            right: 91_440,
            bottom: 45_720,
        }
    }
}

/// What a box does with text that doesn't fit (§8.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Autofit {
    /// Nothing (`a:noAutofit`, none, or `a:spAutoFit`, which PowerPoint applies only on editing).
    None,
    /// Shrinks it (`a:normAutofit`): the engine picks and writes the scale.
    Shrink,
}

/// A paragraph level's style, as inherited.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Level {
    /// The left margin (`marL`) and first-line indent (`indent`), in EMU.
    pub margin: i64,
    pub indent: i64,
    pub line_spacing: Spacing,
    pub space_before: Spacing,
    pub space_after: Spacing,
    /// The default tab stops' interval (`defTabSz`), in EMU: 0 means none, so a tab past the
    /// paragraph's own stops advances nothing.
    pub tab_size: i64,
    /// The paragraph's own tab stops, in EMU, ascending, each once.
    pub tabs: Vec<i64>,
    /// `latinLnBrk`: a Latin word may break between any two characters.
    pub latin_line_break: bool,
    pub bullet: Bullet,
    pub bullet_size: BulletSize,
    /// The bullet's typeface (`a:buFont`), theme references resolved; `None` follows the text's.
    pub bullet_font: Option<Arc<str>>,
    pub run: Run,
}

impl Default for Level {
    /// What a level is where nothing sets it (§8.6).
    fn default() -> Level {
        Level {
            margin: 0,
            indent: 0,
            line_spacing: Spacing::Percent(100_000),
            space_before: Spacing::Points(0),
            space_after: Spacing::Points(0),
            tab_size: 914_400,
            tabs: Vec::new(),
            latin_line_break: false,
            bullet: Bullet::None,
            bullet_size: BulletSize::Percent(100_000),
            bullet_font: None,
            run: Run::default(),
        }
    }
}

/// A spacing: a percentage in thousandths of a percent (`spcPct`), or points in hundredths of a
/// point (`spcPts`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spacing {
    Percent(i64),
    Points(i64),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Bullet {
    None,
    /// A character (`a:buChar`).
    Char(Arc<str>),
    /// A number (`a:buAutoNum`), by its scheme, from its start.
    Number {
        scheme: String,
        start: i64,
    },
    /// A picture (`a:buBlip`): 0.7 em of the bullet's size wide, as PowerPoint draws a square one.
    Picture,
}

/// A bullet's size: a percentage of the text's in thousandths of a percent (`a:buSzPct`), or
/// points in hundredths of a point (`a:buSzPts`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BulletSize {
    Percent(i64),
    Points(i64),
}

/// A level's run properties (`a:defRPr`), as inherited.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    /// The size in hundredths of a point (`sz`).
    pub size: i64,
    /// The latin, East Asian and complex typefaces, theme references resolved; an empty East
    /// Asian or complex one means the latin (§3).
    pub latin: Arc<str>,
    pub east_asian: Arc<str>,
    pub complex: Arc<str>,
    pub bold: bool,
    pub italic: bool,
    pub caps: Caps,
    /// `kern`: kerning from this size up, in hundredths of a point; 0 is none.
    pub kern: i64,
    /// `spc`: character spacing in hundredths of a point.
    pub spacing: i64,
}

impl Default for Run {
    /// 18 pt; the reader names the theme's minor fonts (`+mn-lt` and its siblings).
    fn default() -> Run {
        Run {
            size: 1_800,
            latin: Arc::from(""),
            east_asian: Arc::from(""),
            complex: Arc::from(""),
            bold: false,
            italic: false,
            caps: Caps::None,
            kern: 0,
            spacing: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Caps {
    None,
    /// `cap="all"`.
    All,
    /// `cap="small"`.
    Small,
}
