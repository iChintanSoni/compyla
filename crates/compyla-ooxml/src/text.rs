//! A template's text model (markdown-spec.md §8.6, C63): the body properties of each box text goes
//! into, and the styles its paragraph levels inherit, resolved along the placeholder, its master's,
//! the master's text styles and the presentation's default text style. A template can hold far
//! more layouts and placeholders than a deck uses, so [`Model`] reads a box only when compose asks
//! for it, and each part and theme once.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use compyla_core::validate::pptx::text::{
    Autofit, Body, Bullet, BulletSize, Caps, Insets, Level, Levels, Rect, Run, Spacing,
    TemplateText, TextBox,
};

use crate::dom::{Element, MAIN_A, MAIN_P};
use crate::layouts::{self, Index, Layout, Placeholder};
use crate::template::Template;

/// The explicit tab stops a level keeps: ST_TextTabStopList holds at most 32.
const MAX_TABS: usize = 32;

/// Medium Style 2 – Accent 1, which output uses where the template names no table style (§8.8).
const MEDIUM_STYLE_2: &str = "{5C22544A-7EE6-4342-B048-85BDC9FD1C3A}";

/// ST_TextAutonumberScheme's values.
const AUTONUMBER_SCHEMES: [&str; 41] = [
    "alphaLcParenBoth",
    "alphaUcParenBoth",
    "alphaLcParenR",
    "alphaUcParenR",
    "alphaLcPeriod",
    "alphaUcPeriod",
    "arabicParenBoth",
    "arabicParenR",
    "arabicPeriod",
    "arabicPlain",
    "romanLcParenBoth",
    "romanUcParenBoth",
    "romanLcParenR",
    "romanUcParenR",
    "romanLcPeriod",
    "romanUcPeriod",
    "circleNumDbPlain",
    "circleNumWdBlackPlain",
    "circleNumWdWhitePlain",
    "arabicDbPeriod",
    "arabicDbPlain",
    "ea1ChsPeriod",
    "ea1ChsPlain",
    "ea1ChtPeriod",
    "ea1ChtPlain",
    "ea1JpnChsDbPeriod",
    "ea1JpnKorPlain",
    "ea1JpnKorPeriod",
    "arabic1Minus",
    "arabic2Minus",
    "hebrew2Minus",
    "thaiAlphaPeriod",
    "thaiAlphaParenR",
    "thaiAlphaParenBoth",
    "thaiNumPeriod",
    "thaiNumParenR",
    "thaiNumParenBoth",
    "hindiAlphaPeriod",
    "hindiNumPeriod",
    "hindiNumParenR",
    "hindiAlpha1Period",
];

/// ST_Coordinate32's bounds, for insets; tab sizes and stops are held to 0 and above.
const COORDINATE_32: (i64, i64) = (i32::MIN as i64, i32::MAX as i64);

/// A template's text model, read box by box as compose asks for it (§8.6). Each layout and master
/// part is read once, into each placeholder's type, body properties and the levels its list style
/// sets, and each theme part once however many masters share it.
pub struct Model<'t> {
    template: &'t Template,
    layouts: &'t [Layout],
    default_text: ListStyle,
    table_header: Partial,
    table_body: Partial,
    cache: RefCell<Cache>,
}

/// The parts and themes read so far. Boxes aren't kept: resolving one from what its parts set is
/// cheap, and a deck can ask for many.
#[derive(Default)]
struct Cache {
    themes: HashMap<String, Rc<ThemeFonts>>,
    themes_read: usize,
    masters: HashMap<String, Rc<Master>>,
    layouts: HashMap<usize, Rc<PartText>>,
    parts_read: usize,
}

#[derive(Clone, Copy)]
enum Slot {
    Title,
    Content(usize),
}

/// What a placeholder sets for its text: its type, its body properties where it sets any, and its
/// list style.
struct ShapeText {
    kind: String,
    body: Option<Box<PartialBody>>,
    levels: ListStyle,
}

impl ShapeText {
    fn of(shape: &Element, kind: &str) -> ShapeText {
        let text = shape.child(MAIN_P, "txBody");
        let body = body(text.and_then(|t| t.child(MAIN_A, "bodyPr")));
        ShapeText {
            kind: kind.to_string(),
            body: (!body.is_empty()).then(|| Box::new(body)),
            levels: levels(text.and_then(|t| t.child(MAIN_A, "lstStyle"))),
        }
    }
}

/// A layout's title placeholder and content placeholders, in shape order, with the geometry
/// `layouts::read` gave each.
struct PartText {
    title: Option<(ShapeText, Option<Rect>)>,
    content: Vec<(ShapeText, Option<Rect>)>,
}

/// A master's placeholders, found by type and family, its text styles and its theme's fonts.
struct Master {
    placeholders: Vec<ShapeText>,
    index: Index,
    /// Its first title placeholder, in `placeholders`.
    title: Option<usize>,
    title_style: ListStyle,
    body_style: ListStyle,
    other_style: ListStyle,
    fonts: Rc<ThemeFonts>,
}

impl<'t> Model<'t> {
    /// The text model of `template`, whose layouts `layouts` are (`layouts::read`).
    pub fn new(template: &'t Template, layouts: &'t [Layout]) -> Model<'t> {
        let presentation = template
            .xml(&template.main)
            .ok()
            .flatten()
            .unwrap_or_default();
        // The table style output uses (§8.8): the template's default, else Medium Style 2. A table
        // style part that can't be read is none, as output treats it.
        let table_styles = template
            .related(&template.main, "tableStyles")
            .ok()
            .flatten()
            .and_then(|part| template.xml(&part).ok().flatten());
        let style = table_styles
            .as_ref()
            .and_then(|styles| styles.attr("def"))
            .unwrap_or(MEDIUM_STYLE_2);
        let (table_header, table_body) = table_text(table_styles.as_ref(), style);
        Model {
            template,
            layouts,
            default_text: levels(presentation.child(MAIN_P, "defaultTextStyle")),
            table_header,
            table_body,
            cache: RefCell::default(),
        }
    }

    /// How many theme parts it has read: each once, however many masters share it.
    pub fn themes_read(&self) -> usize {
        self.cache.borrow().themes_read
    }

    /// How many layout and master parts it has read: each once, however many boxes it gives.
    pub fn parts_read(&self) -> usize {
        self.cache.borrow().parts_read
    }

    fn theme(&self, part: Option<String>) -> Rc<ThemeFonts> {
        let Some(part) = part else {
            return Rc::new(ThemeFonts::of(None));
        };
        let key = part.to_ascii_lowercase();
        if let Some(fonts) = self.cache.borrow().themes.get(&key) {
            return fonts.clone();
        }
        let theme = self.template.xml(&part).ok().flatten();
        let fonts = Rc::new(ThemeFonts::of(theme.as_ref()));
        let mut cache = self.cache.borrow_mut();
        cache.themes_read += 1;
        cache.themes.insert(key, fonts.clone());
        fonts
    }

    fn master(&self, part: &str) -> Rc<Master> {
        let key = part.to_ascii_lowercase();
        if let Some(master) = self.cache.borrow().masters.get(&key) {
            return master.clone();
        }
        let xml = self.template.xml(part).ok().flatten().unwrap_or_default();
        let shapes = layouts::tree(&xml)
            .map(|tree| &tree.children[..])
            .unwrap_or_default();
        let found = layouts::placeholders(&xml);
        let placeholders: Vec<ShapeText> = found
            .iter()
            .map(|(position, p)| ShapeText::of(&shapes[*position], &p.kind))
            .collect();
        let styles = xml.child(MAIN_P, "txStyles");
        let style = |name: &str| levels(styles.and_then(|s| s.child(MAIN_P, name)));
        // Found by its type's last segment without case, as the font check finds it (§3).
        let theme = self.template.related_by_type(part, "theme").ok().flatten();
        let master = Rc::new(Master {
            index: Index::new(found.iter().map(|(_, p)| p.kind.as_str())),
            title: found.iter().position(|(_, p)| p.is_title()),
            placeholders,
            title_style: style("titleStyle"),
            body_style: style("bodyStyle"),
            other_style: style("otherStyle"),
            fonts: self.theme(theme),
        });
        let mut cache = self.cache.borrow_mut();
        cache.parts_read += 1;
        cache.masters.insert(key, master.clone());
        master
    }

    /// Layout `layout`'s placeholders' text, read once.
    fn part(&self, layout: usize, geometry: &Layout) -> Rc<PartText> {
        if let Some(part) = self.cache.borrow().layouts.get(&layout) {
            return part.clone();
        }
        let xml = self
            .template
            .xml(&geometry.part)
            .ok()
            .flatten()
            .unwrap_or_default();
        let shapes = layouts::tree(&xml)
            .map(|tree| &tree.children[..])
            .unwrap_or_default();
        let found = layouts::placeholders(&xml);
        let text = |position: usize, p: &Placeholder| ShapeText::of(&shapes[position], &p.kind);
        // Both read the same part, so its placeholders are `geometry`'s, in the same order.
        let title = found
            .iter()
            .find(|(_, p)| p.is_title())
            .map(|(position, p)| (text(*position, p), geometry.title().and_then(|t| t.rect)));
        let content = found
            .iter()
            .filter(|(_, p)| p.is_content())
            .zip(geometry.placeholders.iter().filter(|p| p.is_content()))
            .map(|((position, p), inherited)| (text(*position, p), inherited.rect))
            .collect();
        let part = Rc::new(PartText { title, content });
        // `geometry` must be this template's: its content placeholders are the part's.
        debug_assert_eq!(
            part.content.len(),
            geometry
                .placeholders
                .iter()
                .filter(|p| p.is_content())
                .count()
        );
        let mut cache = self.cache.borrow_mut();
        cache.parts_read += 1;
        cache.layouts.insert(layout, part.clone());
        part
    }

    fn read(&self, layout: usize, slot: Slot) -> Option<TextBox> {
        let geometry = self.layouts.get(layout)?;
        let master = self.master(&geometry.master);
        let part = self.part(layout, geometry);
        let own = match slot {
            Slot::Title => part.title.as_ref(),
            Slot::Content(i) => part.content.get(i),
        };
        match (slot, own) {
            (_, Some((shape, rect))) => {
                let parent = master
                    .index
                    .of(&shape.kind)
                    .map(|i| &master.placeholders[i]);
                Some(self.boxed(Some(shape), parent, Some(&master), &shape.kind, *rect))
            }
            // A layout without a title: the master's title placeholder (§8.8).
            (Slot::Title, None) => Some(self.boxed(
                master.title.map(|i| &master.placeholders[i]),
                None,
                Some(&master),
                "title",
                geometry.master_title,
            )),
            (Slot::Content(_), None) => None,
        }
    }

    /// The box `shape` sets text in, inheriting from `parent` (its master's placeholder), then the
    /// master's text style for its type, then the presentation's (§8.6).
    fn boxed(
        &self,
        shape: Option<&ShapeText>,
        parent: Option<&ShapeText>,
        master: Option<&Master>,
        kind: &str,
        rect: Option<Rect>,
    ) -> TextBox {
        let empty = ListStyle::default();
        let style = match (master, layouts::family(kind)) {
            (Some(master), "title") => &master.title_style,
            (Some(master), "footer") => &master.other_style,
            (Some(master), _) => &master.body_style,
            (None, _) => &empty,
        };
        let shapes: Vec<&ShapeText> = [shape, parent].into_iter().flatten().collect();
        let bodies: Vec<&PartialBody> = shapes.iter().filter_map(|s| s.body.as_deref()).collect();
        let mut lists: Vec<&ListStyle> = shapes.iter().map(|s| &s.levels).collect();
        lists.extend([style, &self.default_text]);
        let fonts = master.map_or_else(|| Rc::new(ThemeFonts::of(None)), |m| m.fonts.clone());
        TextBox {
            rect,
            body: resolve_body(&bodies),
            levels: resolve_levels(&lists, &fonts),
        }
    }
}

/// §8.6's defaults alone, for a layout past the template's: nothing from the template.
fn defaults() -> TextBox {
    TextBox {
        rect: None,
        body: Body::default(),
        levels: resolve_levels(&[], &ThemeFonts::of(None)),
    }
}

impl TemplateText for Model<'_> {
    fn title(&self, layout: usize) -> TextBox {
        self.read(layout, Slot::Title).unwrap_or_else(defaults)
    }

    fn content(&self, layout: usize, slot: usize) -> Option<TextBox> {
        self.read(layout, Slot::Content(slot))
    }

    fn master_body(&self, layout: usize) -> Option<Rect> {
        self.layouts.get(layout).and_then(|l| l.master_body)
    }

    fn text_box(&self, layout: usize) -> Levels {
        match self.layouts.get(layout) {
            Some(geometry) => {
                resolve_levels(&[&self.default_text], &self.master(&geometry.master).fonts)
            }
            None => defaults().levels,
        }
    }

    fn table(&self, layout: usize) -> (Level, Level) {
        let Some(geometry) = self.layouts.get(layout) else {
            let [level, ..] = defaults().levels;
            return (level.clone(), level);
        };
        let master = self.master(&geometry.master);
        let (other, default) = (master.other_style.get(0), self.default_text.get(0));
        let level = |text: &Partial| {
            let chain: Vec<&Partial> = [Some(text), other.as_ref(), default.as_ref()]
                .into_iter()
                .flatten()
                .collect();
            resolve_level(&chain, &master.fonts)
        };
        (level(&self.table_header), level(&self.table_body))
    }
}

/// A level's properties as one style sets them, each `None` where it doesn't.
#[derive(Debug, Clone, Default)]
struct Partial {
    margin: Option<i64>,
    indent: Option<i64>,
    line_spacing: Option<Spacing>,
    space_before: Option<Spacing>,
    space_after: Option<Spacing>,
    tab_size: Option<i64>,
    tabs: Option<Vec<i64>>,
    latin_line_break: Option<bool>,
    bullet: Option<Bullet>,
    bullet_size: Option<BulletSize>,
    /// `Some(None)` for `a:buFontTx`: the bullet follows the text's font.
    bullet_font: Option<Option<Arc<str>>>,
    size: Option<i64>,
    latin: Option<Arc<str>>,
    east_asian: Option<Arc<str>>,
    complex: Option<Arc<str>>,
    bold: Option<bool>,
    italic: Option<bool>,
    caps: Option<Caps>,
    kern: Option<i64>,
    spacing: Option<i64>,
}

/// The levels a list style sets, each by its index (0 for `a:lvl1pPr`) and only the properties it
/// sets: a placeholder costs memory in proportion to what its list style says, not to its levels.
#[derive(Debug, Clone, Default)]
struct ListStyle(Box<[(usize, Box<[Prop]>)]>);

impl ListStyle {
    fn get(&self, level: usize) -> Option<Partial> {
        self.0
            .iter()
            .find(|(i, _)| *i == level)
            .map(|(_, props)| Partial::from_props(props))
    }
}

/// One property a level sets, as a list style keeps it.
#[derive(Debug, Clone)]
enum Prop {
    Margin(i64),
    Indent(i64),
    LineSpacing(Spacing),
    SpaceBefore(Spacing),
    SpaceAfter(Spacing),
    TabSize(i64),
    Tabs(Box<[i64]>),
    LatinLineBreak(bool),
    Bullet(Bullet),
    BulletSize(BulletSize),
    BulletFont(Option<Arc<str>>),
    Size(i64),
    Latin(Arc<str>),
    EastAsian(Arc<str>),
    Complex(Arc<str>),
    Bold(bool),
    Italic(bool),
    Caps(Caps),
    Kern(i64),
    Spacing(i64),
}

impl Partial {
    /// The properties it sets.
    fn props(&self) -> Box<[Prop]> {
        let mut props = Vec::new();
        macro_rules! push {
            ($($field:ident => $prop:expr),* $(,)?) => {
                $(if let Some(value) = &self.$field {
                    props.push($prop(value.clone()));
                })*
            };
        }
        push!(
            margin => Prop::Margin,
            indent => Prop::Indent,
            line_spacing => Prop::LineSpacing,
            space_before => Prop::SpaceBefore,
            space_after => Prop::SpaceAfter,
            tab_size => Prop::TabSize,
            latin_line_break => Prop::LatinLineBreak,
            bullet => Prop::Bullet,
            bullet_size => Prop::BulletSize,
            bullet_font => Prop::BulletFont,
            size => Prop::Size,
            latin => Prop::Latin,
            east_asian => Prop::EastAsian,
            complex => Prop::Complex,
            bold => Prop::Bold,
            italic => Prop::Italic,
            caps => Prop::Caps,
            kern => Prop::Kern,
            spacing => Prop::Spacing,
        );
        if let Some(tabs) = &self.tabs {
            props.push(Prop::Tabs(tabs.clone().into_boxed_slice()));
        }
        props.into_boxed_slice()
    }

    fn from_props(props: &[Prop]) -> Partial {
        let mut level = Partial::default();
        for prop in props {
            match prop.clone() {
                Prop::Margin(v) => level.margin = Some(v),
                Prop::Indent(v) => level.indent = Some(v),
                Prop::LineSpacing(v) => level.line_spacing = Some(v),
                Prop::SpaceBefore(v) => level.space_before = Some(v),
                Prop::SpaceAfter(v) => level.space_after = Some(v),
                Prop::TabSize(v) => level.tab_size = Some(v),
                Prop::Tabs(v) => level.tabs = Some(v.into_vec()),
                Prop::LatinLineBreak(v) => level.latin_line_break = Some(v),
                Prop::Bullet(v) => level.bullet = Some(v),
                Prop::BulletSize(v) => level.bullet_size = Some(v),
                Prop::BulletFont(v) => level.bullet_font = Some(v),
                Prop::Size(v) => level.size = Some(v),
                Prop::Latin(v) => level.latin = Some(v),
                Prop::EastAsian(v) => level.east_asian = Some(v),
                Prop::Complex(v) => level.complex = Some(v),
                Prop::Bold(v) => level.bold = Some(v),
                Prop::Italic(v) => level.italic = Some(v),
                Prop::Caps(v) => level.caps = Some(v),
                Prop::Kern(v) => level.kern = Some(v),
                Prop::Spacing(v) => level.spacing = Some(v),
            }
        }
        level
    }
}

/// The levels a list style sets (`a:lstStyle`, a master's `p:titleStyle` and siblings,
/// `p:defaultTextStyle`): only its `a:lvlNpPr`, as PowerPoint ignores `a:defPPr` (§8.6).
fn levels(list: Option<&Element>) -> ListStyle {
    let Some(list) = list else {
        return ListStyle::default();
    };
    ListStyle(
        (0..9)
            .filter_map(|i| {
                let props = level(list.child(MAIN_A, &format!("lvl{}pPr", i + 1))?).props();
                (!props.is_empty()).then_some((i, props))
            })
            .collect::<Vec<_>>()
            .into_boxed_slice(),
    )
}

fn level(pr: &Element) -> Partial {
    let mut level = Partial {
        margin: number(pr.attr("marL"), 0, 51_206_400),
        indent: number(pr.attr("indent"), -51_206_400, 51_206_400),
        tab_size: number(pr.attr("defTabSz"), 0, COORDINATE_32.1),
        latin_line_break: pr.attr("latinLnBrk").and_then(boolean),
        line_spacing: pr.child(MAIN_A, "lnSpc").and_then(spacing),
        space_before: pr.child(MAIN_A, "spcBef").and_then(spacing),
        space_after: pr.child(MAIN_A, "spcAft").and_then(spacing),
        ..Partial::default()
    };
    if let Some(tabs) = pr.child(MAIN_A, "tabLst") {
        let mut stops: Vec<i64> = tabs
            .children_named(MAIN_A, "tab")
            .take(MAX_TABS)
            .filter_map(|tab| number(tab.attr("pos"), 0, COORDINATE_32.1))
            .collect();
        stops.sort_unstable();
        stops.dedup();
        level.tabs = Some(stops);
    }
    for child in pr.children.iter().filter(|c| c.namespace == MAIN_A) {
        match child.name.as_str() {
            "buNone" => level.bullet = Some(Bullet::None),
            "buBlip" => level.bullet = Some(Bullet::Picture),
            "buChar" => {
                level.bullet = Some(Bullet::Char(Arc::from(
                    child.attr("char").unwrap_or_default(),
                )))
            }
            // A scheme ST_TextAutonumberScheme doesn't have leaves the bullet unset.
            "buAutoNum" => {
                if let Some(scheme) = child
                    .attr("type")
                    .map(str::trim)
                    .filter(|scheme| AUTONUMBER_SCHEMES.contains(scheme))
                {
                    level.bullet = Some(Bullet::Number {
                        scheme: scheme.to_string(),
                        start: number(child.attr("startAt"), 1, 32_767).unwrap_or(1),
                    });
                }
            }
            "buFont" => level.bullet_font = Some(child.attr("typeface").map(Arc::from)),
            "buFontTx" => level.bullet_font = Some(None),
            "buSzPct" => {
                level.bullet_size = percent(child.attr("val"))
                    .map(|p| BulletSize::Percent(p.clamp(25_000, 400_000)))
            }
            "buSzPts" => {
                level.bullet_size = number(child.attr("val"), 100, 400_000).map(BulletSize::Points)
            }
            "buSzTx" => level.bullet_size = Some(BulletSize::Percent(100_000)),
            "defRPr" => run(child, &mut level),
            _ => {}
        }
    }
    level
}

/// A run's properties (`a:defRPr`).
fn run(pr: &Element, level: &mut Partial) {
    level.size = number(pr.attr("sz"), 100, 400_000);
    level.bold = pr.attr("b").and_then(boolean);
    level.italic = pr.attr("i").and_then(boolean);
    level.caps = pr.attr("cap").and_then(|cap| match cap.trim() {
        "all" => Some(Caps::All),
        "small" => Some(Caps::Small),
        "none" => Some(Caps::None),
        _ => None,
    });
    level.kern = number(pr.attr("kern"), 0, 400_000);
    level.spacing = number(pr.attr("spc"), -400_000, 400_000);
    let typeface = |slot: &str| {
        pr.child(MAIN_A, slot)
            .and_then(|font| font.attr("typeface"))
            .map(Arc::from)
    };
    level.latin = typeface("latin");
    level.east_asian = typeface("ea");
    level.complex = typeface("cs");
}

/// An integer attribute held to its schema type's range; one that isn't an integer is unset, so
/// inheritance goes on past it.
fn number(value: Option<&str>, min: i64, max: i64) -> Option<i64> {
    let value = value?.trim().parse::<i128>().ok()?;
    i64::try_from(value.clamp(i128::from(min), i128::from(max))).ok()
}

/// A percentage in thousandths of a percent: an integer (`90000`), or Strict's string, an optional
/// `-`, digits, and optionally `.` and more digits, then `%` (`90%`, `12.5%`).
fn percent(value: Option<&str>) -> Option<i64> {
    let value = value?.trim();
    let Some(text) = value.strip_suffix('%') else {
        return number(Some(value), -1_000_000_000, 1_000_000_000);
    };
    let (negative, text) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    let (whole, fraction) = match text.split_once('.') {
        Some((whole, fraction)) if !fraction.is_empty() => (whole, fraction),
        Some(_) => return None,
        None => (text, ""),
    };
    if whole.is_empty()
        || !whole
            .bytes()
            .chain(fraction.bytes())
            .all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let whole: i64 = whole.parse().ok()?;
    let digits: String = fraction.chars().take(3).collect();
    let fraction: i64 = format!("{digits:0<3}").parse().ok()?;
    let thousandths = whole.checked_mul(1_000)?.checked_add(fraction)?;
    Some(if negative { -thousandths } else { thousandths }.clamp(-1_000_000_000, 1_000_000_000))
}

/// A spacing element's value (`a:spcPct` or `a:spcPts`).
fn spacing(element: &Element) -> Option<Spacing> {
    if let Some(pct) = element.child(MAIN_A, "spcPct") {
        return percent(pct.attr("val")).map(|p| Spacing::Percent(p.clamp(0, 13_200_000)));
    }
    let pts = element.child(MAIN_A, "spcPts")?;
    number(pts.attr("val"), 0, 158_400).map(Spacing::Points)
}

/// An xsd:boolean.
fn boolean(value: &str) -> Option<bool> {
    match value.trim() {
        "1" | "true" => Some(true),
        "0" | "false" => Some(false),
        _ => None,
    }
}

/// An ST_OnOffStyleType (`on`, `off`, `def`): `def` defers to the style below.
fn on_off(value: &str) -> Option<bool> {
    match value.trim() {
        "on" => Some(true),
        "off" => Some(false),
        _ => None,
    }
}

/// A box's body properties as one `a:bodyPr` sets them.
#[derive(Debug, Clone, Default, PartialEq)]
struct PartialBody {
    left: Option<i64>,
    top: Option<i64>,
    right: Option<i64>,
    bottom: Option<i64>,
    wrap: Option<bool>,
    autofit: Option<Autofit>,
    columns: Option<i64>,
    vertical: Option<bool>,
}

impl PartialBody {
    fn is_empty(&self) -> bool {
        *self == PartialBody::default()
    }
}

fn body(pr: Option<&Element>) -> PartialBody {
    let Some(pr) = pr else {
        return PartialBody::default();
    };
    let inset = |name: &str| number(pr.attr(name), COORDINATE_32.0, COORDINATE_32.1);
    let autofit = pr
        .children
        .iter()
        .filter(|c| c.namespace == MAIN_A)
        .find_map(|c| match c.name.as_str() {
            "normAutofit" => Some(Autofit::Shrink),
            // PowerPoint resizes a shape to its text only on editing (§8.6).
            "spAutoFit" | "noAutofit" => Some(Autofit::None),
            _ => None,
        });
    PartialBody {
        left: inset("lIns"),
        top: inset("tIns"),
        right: inset("rIns"),
        bottom: inset("bIns"),
        // ST_TextWrappingType and ST_TextVerticalType; another value is unset.
        wrap: pr.attr("wrap").and_then(|wrap| match wrap.trim() {
            "none" => Some(false),
            "square" => Some(true),
            _ => None,
        }),
        autofit,
        columns: number(pr.attr("numCol"), 1, 16),
        vertical: pr.attr("vert").and_then(|vert| match vert.trim() {
            "horz" => Some(false),
            "vert" | "vert270" | "wordArtVert" | "eaVert" | "mongolianVert" | "wordArtVertRtl" => {
                Some(true)
            }
            _ => None,
        }),
    }
}

/// A box's body properties: the first that sets each, in order, else the defaults (§8.6).
fn resolve_body(chain: &[&PartialBody]) -> Body {
    let defaults = Body::default();
    macro_rules! first {
        ($field:ident) => {
            chain.iter().find_map(|p| p.$field)
        };
    }
    Body {
        insets: Insets {
            left: first!(left).unwrap_or(defaults.insets.left),
            top: first!(top).unwrap_or(defaults.insets.top),
            right: first!(right).unwrap_or(defaults.insets.right),
            bottom: first!(bottom).unwrap_or(defaults.insets.bottom),
        },
        wrap: first!(wrap).unwrap_or(defaults.wrap),
        autofit: first!(autofit).unwrap_or(defaults.autofit),
        measured: first!(columns).unwrap_or(1) <= 1 && !first!(vertical).unwrap_or(false),
    }
}

/// A theme's major and minor typefaces, latin, East Asian and complex, which `+mj-lt` and its
/// siblings name.
#[derive(Debug, Clone)]
struct ThemeFonts {
    major: [Arc<str>; 3],
    minor: [Arc<str>; 3],
}

impl ThemeFonts {
    /// An `a:theme`'s font scheme; Office's (Calibri Light, Calibri) where it has no latin one, or
    /// the part isn't a theme.
    fn of(theme: Option<&Element>) -> ThemeFonts {
        let scheme = theme
            .filter(|theme| theme.is(MAIN_A, "theme"))
            .and_then(|t| t.path(&[(MAIN_A, "themeElements"), (MAIN_A, "fontScheme")]));
        let fonts = |group: &str, latin: &str| {
            let face = |slot: &str| {
                scheme
                    .and_then(|s| s.path(&[(MAIN_A, group), (MAIN_A, slot)]))
                    .and_then(|font| font.attr("typeface"))
                    .unwrap_or_default()
            };
            let first = face("latin");
            [
                Arc::from(if first.is_empty() { latin } else { first }),
                Arc::from(face("ea")),
                Arc::from(face("cs")),
            ]
        };
        ThemeFonts {
            major: fonts("majorFont", "Calibri Light"),
            minor: fonts("minorFont", "Calibri"),
        }
    }

    /// A typeface as text names it, a theme reference resolved; another is shared, not copied.
    fn resolve(&self, typeface: &Arc<str>) -> Arc<str> {
        let (group, slot) = match &**typeface {
            "+mj-lt" => (&self.major, 0),
            "+mj-ea" => (&self.major, 1),
            "+mj-cs" => (&self.major, 2),
            "+mn-lt" => (&self.minor, 0),
            "+mn-ea" => (&self.minor, 1),
            "+mn-cs" => (&self.minor, 2),
            _ => return typeface.clone(),
        };
        group[slot].clone()
    }
}

/// A level's style: the first that sets each property, in order, else the defaults (§8.6), with
/// theme references resolved. Text no style names a font for is in the theme's minor fonts.
fn resolve_level(chain: &[&Partial], theme: &ThemeFonts) -> Level {
    let defaults = Level::default();
    macro_rules! first {
        ($field:ident) => {
            chain.iter().find_map(|p| p.$field.clone())
        };
    }
    // Text no style names a font for is in the theme's minor fonts.
    let font = |name: Option<Arc<str>>, slot: usize| match name {
        Some(name) => theme.resolve(&name),
        None => theme.minor[slot].clone(),
    };
    Level {
        margin: first!(margin).unwrap_or(defaults.margin),
        indent: first!(indent).unwrap_or(defaults.indent),
        line_spacing: first!(line_spacing).unwrap_or(defaults.line_spacing),
        space_before: first!(space_before).unwrap_or(defaults.space_before),
        space_after: first!(space_after).unwrap_or(defaults.space_after),
        tab_size: first!(tab_size).unwrap_or(defaults.tab_size),
        tabs: first!(tabs).unwrap_or(defaults.tabs),
        latin_line_break: first!(latin_line_break).unwrap_or(defaults.latin_line_break),
        bullet: first!(bullet).unwrap_or(defaults.bullet),
        bullet_size: first!(bullet_size).unwrap_or(defaults.bullet_size),
        bullet_font: first!(bullet_font)
            .flatten()
            .map(|typeface| theme.resolve(&typeface)),
        run: Run {
            size: first!(size).unwrap_or(defaults.run.size),
            latin: font(first!(latin), 0),
            east_asian: font(first!(east_asian), 1),
            complex: font(first!(complex), 2),
            bold: first!(bold).unwrap_or(defaults.run.bold),
            italic: first!(italic).unwrap_or(defaults.run.italic),
            caps: first!(caps).unwrap_or(defaults.run.caps),
            kern: first!(kern).unwrap_or(defaults.run.kern),
            spacing: first!(spacing).unwrap_or(defaults.run.spacing),
        },
    }
}

/// Each level resolved along `chain`.
fn resolve_levels(chain: &[&ListStyle], theme: &ThemeFonts) -> Levels {
    std::array::from_fn(|i| {
        let levels: Vec<Partial> = chain.iter().filter_map(|style| style.get(i)).collect();
        resolve_level(&levels.iter().collect::<Vec<_>>(), theme)
    })
}

/// A table style's text for the header row and for the others (§8.6): `a:tcTxStyle` of its first
/// row over its whole table's, and of its whole table's. A style the part doesn't define, built
/// in, is measured as Medium Style 2: the minor font, the header bold.
fn table_text(styles: Option<&Element>, id: &str) -> (Partial, Partial) {
    let style = styles.and_then(|styles| {
        styles
            .children_named(MAIN_A, "tblStyle")
            .find(|style| style.attr("styleId") == Some(id))
    });
    let part = |name: &str| -> Partial {
        match style {
            Some(style) => style
                .path(&[(MAIN_A, name), (MAIN_A, "tcTxStyle")])
                .map(cell_text)
                .unwrap_or_default(),
            None => Partial {
                latin: Some(Arc::from("+mn-lt")),
                east_asian: Some(Arc::from("+mn-ea")),
                complex: Some(Arc::from("+mn-cs")),
                bold: (name == "firstRow").then_some(true),
                ..Partial::default()
            },
        }
    };
    let whole = part("wholeTbl");
    let header = merge(&part("firstRow"), &whole);
    (header, whole)
}

/// A table style part's text properties: `b` and `i` (`on`, `off`, `def`), and its font or font
/// reference.
fn cell_text(style: &Element) -> Partial {
    let mut text = Partial {
        bold: style.attr("b").and_then(on_off),
        italic: style.attr("i").and_then(on_off),
        ..Partial::default()
    };
    if let Some(font) = style.child(MAIN_A, "font") {
        let typeface = |slot: &str| {
            font.child(MAIN_A, slot)
                .and_then(|f| f.attr("typeface"))
                .map(Arc::from)
        };
        text.latin = typeface("latin");
        text.east_asian = typeface("ea");
        text.complex = typeface("cs");
    } else if let Some(reference) = style.child(MAIN_A, "fontRef") {
        let group = match reference.attr("idx") {
            Some("major") => Some("mj"),
            Some("minor") => Some("mn"),
            _ => None,
        };
        if let Some(group) = group {
            text.latin = Some(Arc::from(format!("+{group}-lt")));
            text.east_asian = Some(Arc::from(format!("+{group}-ea")));
            text.complex = Some(Arc::from(format!("+{group}-cs")));
        }
    }
    text
}

/// `over`'s properties, with `under`'s where it sets none.
fn merge(over: &Partial, under: &Partial) -> Partial {
    macro_rules! either {
        ($($field:ident),*) => {
            Partial { $($field: over.$field.clone().or_else(|| under.$field.clone()),)* }
        };
    }
    either!(
        margin,
        indent,
        line_spacing,
        space_before,
        space_after,
        tab_size,
        tabs,
        latin_line_break,
        bullet,
        bullet_size,
        bullet_font,
        size,
        latin,
        east_asian,
        complex,
        bold,
        italic,
        caps,
        kern,
        spacing
    )
}
#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";

    fn element(xml: &str) -> Element {
        crate::dom::parse(format!(r#"<a:x xmlns:a="{A}">{xml}</a:x>"#).as_bytes()).unwrap()
    }

    #[test]
    fn a_level_reads_each_property() {
        let list = element(concat!(
            r#"<a:lvl1pPr marL="99999999999" indent="-5" defTabSz="100" latinLnBrk="1">"#,
            r#"<a:lnSpc><a:spcPts val="3600"/></a:lnSpc><a:spcBef><a:spcPct val="50%"/></a:spcBef>"#,
            r#"<a:buFontTx/><a:buSzPts val="1400"/><a:buAutoNum type="romanUcPeriod" startAt="3"/>"#,
            r#"<a:tabLst><a:tab pos="900"/><a:tab pos="x"/><a:tab pos="300"/><a:tab pos="900"/></a:tabLst>"#,
            r#"<a:defRPr sz="1" b="1" i="0" cap="small" kern="1200" spc="-50"><a:latin typeface="+mj-lt"/><a:ea typeface="Brand Mincho"/></a:defRPr>"#,
            r#"</a:lvl1pPr><a:lvl2pPr><a:buFont typeface="+mn-lt"/><a:buSzPct val="80000"/><a:buChar char="-"/></a:lvl2pPr>"#,
            r#"<a:lvl3pPr><a:buBlip><a:blip/></a:buBlip></a:lvl3pPr><a:lvl4pPr><a:buAutoNum type="bogus"/></a:lvl4pPr>"#,
        ));
        let theme = ThemeFonts {
            major: [Arc::from("Major"), Arc::from("Major EA"), Arc::from("")],
            minor: [Arc::from("Minor"), Arc::from(""), Arc::from("")],
        };
        let levels = resolve_levels(&[&levels(Some(&list))], &theme);
        let first = &levels[0];
        // Held to ST_TextMargin's bound; sizes to ST_TextFontSize's.
        assert_eq!(
            (first.margin, first.indent, first.tab_size),
            (51_206_400, -5, 100)
        );
        assert!(first.latin_line_break);
        assert_eq!(first.line_spacing, Spacing::Points(3_600));
        assert_eq!(first.space_before, Spacing::Percent(50_000));
        assert_eq!(first.bullet_font, None);
        assert_eq!(first.bullet_size, BulletSize::Points(1_400));
        assert_eq!(
            first.bullet,
            Bullet::Number {
                scheme: "romanUcPeriod".into(),
                start: 3
            }
        );
        assert_eq!(first.tabs, [300, 900]);
        let run = &first.run;
        assert_eq!((run.size, run.bold, run.italic), (100, true, false));
        assert_eq!((run.caps, run.kern, run.spacing), (Caps::Small, 1_200, -50));
        // Theme references resolve; an unset one is the minor font's.
        assert_eq!(
            (&*run.latin, &*run.east_asian, &*run.complex),
            ("Major", "Brand Mincho", "")
        );
        let second = &levels[1];
        assert_eq!(second.bullet, Bullet::Char("-".into()));
        assert_eq!(second.bullet_font.as_deref(), Some("Minor"));
        assert_eq!(second.bullet_size, BulletSize::Percent(80_000));
        assert_eq!(&*second.run.latin, "Minor");
        // A picture bullet; a number of no scheme the schema has is no bullet set.
        assert_eq!(levels[2].bullet, Bullet::Picture);
        assert_eq!(levels[3].bullet, Bullet::None);
        // A level nothing sets is the defaults.
        assert_eq!(levels[4].line_spacing, Spacing::Percent(100_000));
        assert_eq!(levels[4].run.size, 1_800);
    }

    #[test]
    fn a_level_keeps_each_property_it_sets() {
        // Every property, through the compact form a list style keeps, and back.
        let full = Partial {
            margin: Some(1),
            indent: Some(-2),
            line_spacing: Some(Spacing::Percent(3)),
            space_before: Some(Spacing::Points(4)),
            space_after: Some(Spacing::Percent(5)),
            tab_size: Some(6),
            tabs: Some(vec![7, 8]),
            latin_line_break: Some(true),
            bullet: Some(Bullet::Number {
                scheme: "arabicPeriod".into(),
                start: 9,
            }),
            bullet_size: Some(BulletSize::Points(10)),
            bullet_font: Some(None),
            size: Some(11),
            latin: Some("Latin".into()),
            east_asian: Some("East".into()),
            complex: Some("Complex".into()),
            bold: Some(false),
            italic: Some(true),
            caps: Some(Caps::Small),
            kern: Some(12),
            spacing: Some(-13),
        };
        let props = full.props();
        assert_eq!(props.len(), 20);
        assert_eq!(
            format!("{:?}", Partial::from_props(&props)),
            format!("{full:?}")
        );
        assert!(Partial::default().props().is_empty());
    }

    #[test]
    fn a_body_reads_wrap_columns_and_direction() {
        let body = |xml: &str| resolve_body(&[&super::body(Some(&element(xml).children[0]))]);
        assert!(!body(r#"<a:bodyPr wrap="none"/>"#).wrap);
        // Insets are held to ST_Coordinate32.
        let far = body(r#"<a:bodyPr lIns="99999999999" rIns="-99999999999"/>"#).insets;
        assert_eq!(
            (far.left, far.right),
            (i64::from(i32::MAX), i64::from(i32::MIN))
        );
        assert!(body(r#"<a:bodyPr wrap="square"/>"#).wrap);
        assert!(!body(r#"<a:bodyPr numCol="2"/>"#).measured);
        assert!(!body(r#"<a:bodyPr vert="eaVert"/>"#).measured);
        assert!(body(r#"<a:bodyPr vert="horz" numCol="1"/>"#).measured);
        // Growing to fit is measured as fixed (§8.6).
        assert_eq!(
            body(r#"<a:bodyPr><a:spAutoFit/></a:bodyPr>"#).autofit,
            Autofit::None
        );
        assert_eq!(
            body(r#"<a:bodyPr><a:normAutofit/></a:bodyPr>"#).autofit,
            Autofit::Shrink
        );
    }

    #[test]
    fn percentages_read_both_ways() {
        assert_eq!(percent(Some("90000")), Some(90_000));
        assert_eq!(percent(Some("90%")), Some(90_000));
        assert_eq!(percent(Some("12.5%")), Some(12_500));
        assert_eq!(percent(Some("-7.25%")), Some(-7_250));
        assert_eq!(percent(Some("99999999999999999999%")), None);
        assert_eq!(percent(Some("x%")), None);
        for odd in ["+5%", "--5%", "5.%", ".5%", "%"] {
            assert_eq!(percent(Some(odd)), None, "{odd}");
        }
        assert_eq!(number(Some("99999999999999999999999"), 0, 10), Some(10));
    }

    /// `package` with `edit` applied to its text parts, and `extra` parts added.
    fn edited(
        package: &[u8],
        edit: impl Fn(&str, String) -> String,
        extra: Vec<(&str, String)>,
    ) -> Vec<u8> {
        let mut entries: Vec<(String, Vec<u8>)> = crate::read::entries(package)
            .unwrap()
            .into_iter()
            .map(|(name, data)| match String::from_utf8(data) {
                Ok(text) => {
                    let text = edit(&name, text);
                    (name, text.into_bytes())
                }
                Err(bytes) => (name, bytes.into_bytes()),
            })
            .collect();
        entries.extend(
            extra
                .into_iter()
                .map(|(n, t)| (n.to_string(), t.into_bytes())),
        );
        crate::zip::write(&entries)
    }

    const HOUSE: &[u8] = include_bytes!("../../compyla-pptx/house/house.potx");
    const BRAND: &[u8] =
        include_bytes!("../../../spec/conformance/lint/139-pptx-template-binding/brand.potx");

    #[test]
    fn the_house_templates_text_is_its_master_styles() {
        let template = Template::read(HOUSE).unwrap();
        let layouts = layouts::read(&template).unwrap();
        let model = Model::new(&template, &layouts);
        let at = |name: &str| layouts.iter().position(|l| l.name == name).unwrap();
        let content = at("Title and Content");
        let body = model.content(content, 0).unwrap();
        assert_eq!(body.body, Body::default());
        let first = &body.levels[0];
        assert_eq!((first.margin, first.indent), (228_600, -228_600));
        assert_eq!(first.line_spacing, Spacing::Percent(90_000));
        assert_eq!(first.space_before, Spacing::Points(1_000));
        assert_eq!(first.space_after, Spacing::Points(0));
        assert_eq!(first.bullet, Bullet::Char("\u{2022}".into()));
        assert_eq!(first.bullet_font.as_deref(), Some("Arial"));
        assert_eq!(
            (first.run.size, &*first.run.latin, first.run.kern),
            (2_800, "Calibri", 1_200)
        );
        assert_eq!(
            (body.levels[1].run.size, body.levels[1].margin),
            (2_400, 685_800)
        );
        assert_eq!(body.levels[3].run.size, 1_800);
        assert!(model.content(content, 1).is_none());
        // Titles take the master's title style in the theme's major font.
        let title = model.title(content);
        assert_eq!(
            (title.levels[0].run.size, &*title.levels[0].run.latin),
            (4_000, "Calibri Light")
        );
        assert_eq!(
            title.rect,
            Some(Rect {
                x: 838_200,
                y: 365_125,
                cx: 10_515_600,
                cy: 1_325_563
            })
        );
        // A subtitle has no bullet; Blank has no title of its own, so its title is the master's.
        let cover = at("Title Slide");
        assert_eq!(model.title(cover).levels[0].run.size, 6_000);
        let subtitle = &model.content(cover, 0).unwrap().levels[0];
        assert_eq!((subtitle.run.size, subtitle.margin), (2_400, 0));
        assert_eq!(subtitle.bullet, Bullet::None);
        // Comparison's slots in shape order: a heading, its content, a heading, its content.
        let comparison = at("Comparison");
        let slots: Vec<(i64, bool, Option<i64>)> = (0..4)
            .map(|slot| {
                let text = model.content(comparison, slot).unwrap();
                let first = &text.levels[0];
                (first.run.size, first.run.bold, text.rect.map(|r| r.x))
            })
            .collect();
        assert_eq!(
            slots,
            [
                (2_400, true, Some(839_788)),
                (2_800, false, Some(839_788)),
                (2_400, true, Some(6_172_200)),
                (2_800, false, Some(6_172_200))
            ]
        );
        assert!(model.content(comparison, 4).is_none());
        let blank = at("Blank");
        assert!(model.content(blank, 0).is_none());
        assert_eq!(model.title(blank).rect, title.rect);
        assert_eq!(model.title(blank).levels[0].run.size, 4_000);
        // Nothing in the house chain sizes text boxes or table cells: 18 pt, the header bold.
        let text_box = model.text_box(content);
        assert_eq!(
            (text_box[0].run.size, &*text_box[0].run.latin),
            (1_800, "Calibri")
        );
        let (header, cell) = model.table(content);
        assert!(header.run.bold && !cell.run.bold);
        assert_eq!(cell.run.size, 1_800);
        assert_eq!(model.master_body(content), layouts[content].master_body);
    }

    /// The brand template (lint/139) with a master body placeholder and text styles, a layout's
    /// content placeholder that overrides some of them, a subtitle with an invalid `wrap`, a
    /// second master with its own theme, and the master's theme related by a type in another
    /// case.
    fn brand() -> Vec<u8> {
        let body = concat!(
            r#"<p:sp><p:nvSpPr><p:cNvPr id="7" name="Body"/><p:cNvSpPr/><p:nvPr><p:ph type="body" idx="1"/></p:nvPr></p:nvSpPr>"#,
            r#"<p:spPr><a:xfrm><a:off x="1" y="2"/><a:ext cx="3" cy="4"/></a:xfrm></p:spPr>"#,
            r#"<p:txBody><a:bodyPr lIns="0" tIns="0" wrap="none" numCol="2"><a:normAutofit/></a:bodyPr>"#,
            r#"<a:lstStyle><a:lvl1pPr marL="100"><a:lnSpc><a:spcPct val="80%"/></a:lnSpc><a:buFont typeface="Arial"/><a:defRPr sz="3000"/></a:lvl1pPr></a:lstStyle><a:p/></p:txBody></p:sp>"#,
        );
        let master_title = r#"<p:ph type="title"/></p:nvPr></p:nvSpPr><p:spPr/><p:txBody><a:bodyPr/><a:lstStyle><a:lvl1pPr><a:defRPr sz="5000"/></a:lvl1pPr></a:lstStyle>"#;
        let theme2 = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" name="Second"><a:themeElements><a:fontScheme name="Second"><a:majorFont><a:latin typeface="Second Major"/></a:majorFont><a:minorFont><a:latin typeface="Second Minor"/></a:minorFont></a:fontScheme></a:themeElements></a:theme>"#;
        edited(
            BRAND,
            |name, xml| match name {
                "ppt/slideMasters/slideMaster1.xml" => xml
                    .replacen("</p:spTree>", &format!("{body}</p:spTree>"), 1)
                    .replacen(
                        r#"<p:ph type="title"/></p:nvPr></p:nvSpPr><p:spPr/><p:txBody><a:bodyPr/>"#,
                        master_title,
                        1,
                    )
                    .replacen(
                        "<p:otherStyle/>",
                        r#"<p:otherStyle><a:lvl1pPr><a:defRPr sz="1000"/></a:lvl1pPr></p:otherStyle>"#,
                        1,
                    ),
                // The content placeholder of Titel und Inhalt: its own top inset, wrap and one
                // column, no autofit, a margin over the master's and the text's font for its
                // bullet; its `a:defPPr` doesn't count, so the master's size does.
                "ppt/slideLayouts/slideLayout2.xml" => {
                    let at = xml.find(r#"name="Inhaltsplatzhalter 2""#).unwrap();
                    let (head, tail) = xml.split_at(at);
                    let tail = tail.replacen(
                        "<a:bodyPr/>",
                        concat!(
                            r#"<a:bodyPr tIns="7" wrap="square" numCol="1"><a:noAutofit/></a:bodyPr><a:lstStyle>"#,
                            r#"<a:defPPr><a:defRPr sz="9900"/></a:defPPr><a:lvl1pPr marL="555"><a:buFontTx/></a:lvl1pPr></a:lstStyle>"#
                        ),
                        1,
                    );
                    format!("{head}{tail}")
                }
                // The subtitle sets its left inset; its `wrap` and `vert` aren't values of their
                // types, so the master's apply.
                "ppt/slideLayouts/slideLayout1.xml" => {
                    let at = xml.find(r#"name="Subtitle 2""#).unwrap();
                    let (head, tail) = xml.split_at(at);
                    format!("{head}{}", tail.replacen("<a:bodyPr/>", r#"<a:bodyPr lIns="5" wrap="Square" vert="sideways"/>"#, 1))
                }
                "ppt/slideMasters/_rels/slideMaster1.xml.rels" => {
                    xml.replace("relationships/theme\"", "relationships/Theme\"")
                }
                // A second master, with its own theme and its own copy of the Blank layout.
                "ppt/presentation.xml" => xml.replacen(
                    "</p:sldMasterIdLst>",
                    r#"<p:sldMasterId id="2147483900" r:id="rId9"/></p:sldMasterIdLst>"#,
                    1,
                ),
                "ppt/_rels/presentation.xml.rels" => xml.replacen(
                    "</Relationships>",
                    r#"<Relationship Id="rId9" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster" Target="slideMasters/slideMaster2.xml"/></Relationships>"#,
                    1,
                ),
                "[Content_Types].xml" => xml.replacen(
                    "</Types>",
                    concat!(
                        r#"<Override PartName="/ppt/slideMasters/slideMaster2.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml"/>"#,
                        r#"<Override PartName="/ppt/slideLayouts/slideLayout9.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml"/>"#,
                        r#"<Override PartName="/ppt/theme/theme9.xml" ContentType="application/vnd.openxmlformats-officedocument.theme+xml"/></Types>"#
                    ),
                    1,
                ),
                _ => xml,
            },
            vec![
                (
                    "ppt/slideMasters/slideMaster2.xml",
                    String::from_utf8(
                        crate::read::entries(BRAND)
                            .unwrap()
                            .into_iter()
                            .find(|(n, _)| n == "ppt/slideMasters/slideMaster1.xml")
                            .unwrap()
                            .1,
                    )
                    .unwrap()
                    .replace(
                        r#"<p:sldLayoutIdLst>"#,
                        r#"<p:sldLayoutIdLst><p:sldLayoutId id="2147483901" r:id="rId1"/></p:sldLayoutIdLst><p:removed>"#,
                    )
                    .replace("</p:sldLayoutIdLst><p:txStyles>", "</p:removed><p:txStyles>"),
                ),
                (
                    "ppt/slideMasters/_rels/slideMaster2.xml.rels",
                    concat!(
                        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
                        r#"<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout9.xml"/>"#,
                        r#"<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme" Target="../theme/theme9.xml"/></Relationships>"#
                    )
                    .to_string(),
                ),
                (
                    "ppt/slideLayouts/slideLayout9.xml",
                    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><p:sldLayout xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" type="blank"><p:cSld name="Second Blank"><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/></p:spTree></p:cSld></p:sldLayout>"#
                        .to_string(),
                ),
                ("ppt/theme/theme9.xml", theme2.to_string()),
            ],
        )
    }

    #[test]
    fn each_property_comes_from_the_first_that_sets_it() {
        let bytes = brand();
        let template = Template::read(&bytes).unwrap();
        let layouts = layouts::read(&template).unwrap();
        let model = Model::new(&template, &layouts);
        let at = |name: &str| layouts.iter().position(|l| l.name == name).unwrap();

        let titled = at("Titel und Inhalt");
        let content = model.content(titled, 0).unwrap();
        assert_eq!(
            content.rect,
            Some(Rect {
                x: 1,
                y: 2,
                cx: 3,
                cy: 4
            })
        );
        assert_eq!(
            content.body.insets,
            Insets {
                left: 0,
                top: 7,
                ..Insets::default()
            }
        );
        // Its own `wrap` and one column over the master's; its `noAutofit` over `normAutofit`.
        assert!(content.body.wrap && content.body.measured);
        assert_eq!(content.body.autofit, Autofit::None);
        let first = &content.levels[0];
        assert_eq!((first.run.size, first.margin), (3_000, 555));
        assert_eq!(first.line_spacing, Spacing::Percent(80_000));
        // `buFontTx` over the master's `buFont`: the bullet follows the text.
        assert_eq!(first.bullet_font, None);
        // The master's body style names the font.
        assert_eq!(&*first.run.latin, "Source Serif 4");

        // The subtitle has only its left inset: the rest is the master's body placeholder, which
        // shrinks, doesn't wrap, has two columns (so it isn't measured) and names Arial.
        let subtitle = model.content(at("Title Slide"), 0).unwrap();
        assert_eq!(subtitle.levels[0].run.size, 3_000);
        assert_eq!(subtitle.levels[0].bullet_font.as_deref(), Some("Arial"));
        assert_eq!(subtitle.body.autofit, Autofit::Shrink);
        assert_eq!(
            (subtitle.body.insets.left, subtitle.body.insets.top),
            (5, 0)
        );
        assert!(!subtitle.body.wrap && !subtitle.body.measured);

        // Titles take the master title placeholder's own list style, with or without a title
        // placeholder of their own (Blank).
        assert_eq!(model.title(titled).levels[0].run.size, 5_000);
        assert_eq!(model.title(at("Blank")).levels[0].run.size, 5_000);

        // The theme, related by a type in another case, names Brand Display and Brand Sans.
        assert_eq!(&*model.title(titled).levels[0].run.latin, "Brand Display");
        let text_box = model.text_box(titled);
        assert_eq!(&*text_box[0].run.latin, "Brand Sans");
        // Text boxes don't take the master's other style; table cells do.
        assert_eq!(text_box[0].run.size, 1_800);
        let (header, cell) = model.table(titled);
        assert_eq!((header.run.size, cell.run.size), (1_000, 1_000));
        // No table style part: Medium Style 2, in the minor font, its header bold.
        assert_eq!(&*header.run.latin, "Brand Sans");
        assert!(header.run.bold && !cell.run.bold);

        // A layout of the second master takes that master's theme.
        let second = at("Second Blank");
        assert_eq!(&*model.title(second).levels[0].run.latin, "Second Major");
        assert_eq!(&*model.text_box(second)[0].run.latin, "Second Minor");

        // Typefaces are shared, not copied, however many boxes resolve them: a style's and the
        // theme's alike.
        let again = model.content(titled, 0).unwrap();
        assert!(Arc::ptr_eq(
            &content.levels[0].run.latin,
            &again.levels[0].run.latin
        ));
        assert!(Arc::ptr_eq(
            &model.text_box(titled)[0].run.latin,
            &model.text_box(titled)[0].run.latin
        ));

        // Past the layouts: §8.6's defaults alone, nothing from the template.
        let past = layouts.len();
        assert!(model.content(past, 0).is_none());
        let title = model.title(past);
        assert_eq!(
            (title.levels[0].run.size, &*title.levels[0].run.latin),
            (1_800, "Calibri")
        );
        assert_eq!(&*model.text_box(past)[0].run.latin, "Calibri");
        assert!(!model.table(past).0.run.bold);
        assert_eq!(model.master_body(past), None);
    }

    #[test]
    fn a_part_that_isnt_a_theme_names_office_fonts() {
        // A theme's structure under another root: not a theme, so Office's fonts.
        let other = element(concat!(
            r#"<a:notTheme><a:themeElements><a:fontScheme><a:majorFont><a:latin typeface="Odd"/>"#,
            r#"</a:majorFont></a:fontScheme></a:themeElements></a:notTheme>"#
        ));
        let fonts = ThemeFonts::of(Some(&other.children[0]));
        assert_eq!(&*fonts.resolve(&Arc::from("+mj-lt")), "Calibri Light");
        assert_eq!(&*fonts.resolve(&Arc::from("+mn-lt")), "Calibri");
        // The same under `a:theme` is read.
        let theme = element(concat!(
            r#"<a:theme><a:themeElements><a:fontScheme><a:majorFont><a:latin typeface="Odd"/>"#,
            r#"</a:majorFont></a:fontScheme></a:themeElements></a:theme>"#
        ));
        assert_eq!(
            &*ThemeFonts::of(Some(&theme.children[0])).resolve(&Arc::from("+mj-lt")),
            "Odd"
        );
    }

    #[test]
    fn a_templates_table_style_and_default_text_reach_tables_and_text_boxes() {
        let fonts: &[u8] =
            include_bytes!("../../../spec/conformance/lint/144-pptx-fonts-unresolved/brand.potx");
        // lint/144: its table style's whole table names Brand Table, and its first row now
        // Brand Header over it; its default text style names Brand Default.
        let header = edited(
            fonts,
            |name, xml| {
                match name {
                "ppt/tableStyles.xml" => xml.replacen(
                    "</a:wholeTbl>",
                    r#"</a:wholeTbl><a:firstRow><a:tcTxStyle b="off"><a:font><a:latin typeface="Brand Header"/></a:font></a:tcTxStyle></a:firstRow>"#,
                    1,
                ),
                _ => xml,
            }
            },
            vec![],
        );
        let template = Template::read(&header).unwrap();
        let layouts = layouts::read(&template).unwrap();
        let model = Model::new(&template, &layouts);
        let (header, cell) = model.table(0);
        assert_eq!(&*header.run.latin, "Brand Header");
        assert!(!header.run.bold);
        assert_eq!(&*cell.run.latin, "Brand Table");
        assert_eq!(&*model.text_box(0)[0].run.latin, "Brand Default");
        // Past the layouts, nothing from the template: not its default text style or table style.
        let past = layouts.len();
        assert_eq!(&*model.text_box(past)[0].run.latin, "Calibri");
        assert_eq!(&*model.title(past).levels[0].run.latin, "Calibri");
        assert_eq!(&*model.table(past).1.run.latin, "Calibri");

        // A built-in default style the part doesn't define is measured as Medium Style 2.
        let builtin = edited(
            fonts,
            |name, xml| match name {
                "ppt/tableStyles.xml" => xml.replacen(
                    r#"def="{5C22544A-7EE6-4342-B048-85BDC9FD1C3A}""#,
                    r#"def="{F5AB1C69-6EDB-4FF4-983F-18BD219EF322}""#,
                    1,
                ),
                _ => xml,
            },
            vec![],
        );
        let template = Template::read(&builtin).unwrap();
        let layouts = layouts::read(&template).unwrap();
        let (header, cell) = Model::new(&template, &layouts).table(0);
        assert!(header.run.bold && !cell.run.bold);
    }
}
