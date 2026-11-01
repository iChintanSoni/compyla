//! A template's text model (markdown-spec.md §8.6, C63): the body properties of each box text goes
//! into, and the styles its paragraph levels inherit, resolved along the placeholder, its layout's,
//! its master's, the master's text styles and the presentation's default text style.

use compyla_core::validate::pptx::text::{
    Autofit, Body, Bullet, BulletSize, Caps, Insets, Level, Levels, Run, Spacing,
};

use crate::dom::{Element, MAIN_A};

/// The explicit tab stops a level keeps: ST_TextTabStopList holds at most 32.
const MAX_TABS: usize = 32;

/// Medium Style 2 – Accent 1, which output uses where the template names no table style (§8.8).
pub(crate) const MEDIUM_STYLE_2: &str = "{5C22544A-7EE6-4342-B048-85BDC9FD1C3A}";

/// A level's properties as one style sets them, each `None` where it doesn't.
#[derive(Debug, Clone, Default)]
pub(crate) struct Partial {
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
    bullet_font: Option<Option<String>>,
    size: Option<i64>,
    latin: Option<String>,
    east_asian: Option<String>,
    complex: Option<String>,
    bold: Option<bool>,
    italic: Option<bool>,
    caps: Option<Caps>,
    kern: Option<i64>,
    spacing: Option<i64>,
}

pub(crate) type PartialLevels = [Partial; 9];

/// The levels a list style sets (`a:lstStyle`, a master's `p:titleStyle` and siblings,
/// `p:defaultTextStyle`): only its `a:lvlNpPr`, as PowerPoint ignores `a:defPPr` (§8.6).
pub(crate) fn levels(list: Option<&Element>) -> PartialLevels {
    std::array::from_fn(|i| {
        list.and_then(|list| list.child(MAIN_A, &format!("lvl{}pPr", i + 1)))
            .map(level)
            .unwrap_or_default()
    })
}

fn level(pr: &Element) -> Partial {
    let mut level = Partial {
        margin: number(pr.attr("marL"), 0, 51_206_400),
        indent: number(pr.attr("indent"), -51_206_400, 51_206_400),
        tab_size: number(pr.attr("defTabSz"), 0, i64::from(i32::MAX)),
        latin_line_break: pr.attr("latinLnBrk").and_then(boolean),
        line_spacing: pr.child(MAIN_A, "lnSpc").and_then(spacing),
        space_before: pr.child(MAIN_A, "spcBef").and_then(spacing),
        space_after: pr.child(MAIN_A, "spcAft").and_then(spacing),
        ..Partial::default()
    };
    if let Some(tabs) = pr.child(MAIN_A, "tabLst") {
        level.tabs = Some(
            tabs.children_named(MAIN_A, "tab")
                .filter_map(|tab| number(tab.attr("pos"), 0, i64::from(i32::MAX)))
                .take(MAX_TABS)
                .collect(),
        );
    }
    for child in pr.children.iter().filter(|c| c.namespace == MAIN_A) {
        match child.name.as_str() {
            "buNone" | "buBlip" => level.bullet = Some(Bullet::None),
            "buChar" => {
                level.bullet = Some(Bullet::Char(
                    child.attr("char").unwrap_or_default().to_string(),
                ))
            }
            "buAutoNum" => {
                level.bullet = Some(Bullet::Number {
                    scheme: child.attr("type").unwrap_or("arabicPeriod").to_string(),
                    start: number(child.attr("startAt"), 1, 32_767).unwrap_or(1),
                })
            }
            "buFont" => level.bullet_font = Some(child.attr("typeface").map(str::to_string)),
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

/// A run's properties (`a:defRPr`, or a table style's `a:tcTxStyle`).
fn run(pr: &Element, level: &mut Partial) {
    level.size = number(pr.attr("sz"), 100, 400_000);
    level.bold = pr.attr("b").and_then(boolean);
    level.italic = pr.attr("i").and_then(boolean);
    level.caps = pr.attr("cap").and_then(|cap| match cap {
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
            .map(str::to_string)
    };
    level.latin = typeface("latin");
    level.east_asian = typeface("ea");
    level.complex = typeface("cs");
}

/// An integer attribute held to its schema range.
fn number(value: Option<&str>, min: i64, max: i64) -> Option<i64> {
    let value = value?.trim().parse::<i128>().ok()?;
    i64::try_from(value.clamp(i128::from(min), i128::from(max))).ok()
}

/// A percentage in thousandths of a percent: an integer (`90000`), or Strict's string (`90%`).
fn percent(value: Option<&str>) -> Option<i64> {
    let value = value?.trim();
    let Some(text) = value.strip_suffix('%') else {
        return number(Some(value), -1_000_000_000, 1_000_000_000);
    };
    let (whole, fraction) = text.split_once('.').unwrap_or((text, ""));
    let negative = whole.starts_with('-');
    let whole: i64 = whole.trim_start_matches('-').parse().ok()?;
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

fn boolean(value: &str) -> Option<bool> {
    match value {
        "1" | "true" | "on" => Some(true),
        "0" | "false" | "off" => Some(false),
        _ => None,
    }
}

/// A box's body properties as one `a:bodyPr` sets them.
#[derive(Debug, Clone, Default)]
pub(crate) struct PartialBody {
    left: Option<i64>,
    top: Option<i64>,
    right: Option<i64>,
    bottom: Option<i64>,
    wrap: Option<bool>,
    autofit: Option<Autofit>,
    columns: Option<i64>,
    vertical: Option<bool>,
}

pub(crate) fn body(pr: Option<&Element>) -> PartialBody {
    let Some(pr) = pr else {
        return PartialBody::default();
    };
    let inset = |name: &str| number(pr.attr(name), -51_206_400, 51_206_400);
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
        wrap: pr.attr("wrap").map(|wrap| wrap != "none"),
        autofit,
        columns: number(pr.attr("numCol"), 1, 16),
        vertical: pr.attr("vert").map(|vert| vert != "horz"),
    }
}

/// A box's body properties: the first that sets each, in order, else the defaults (§8.6).
pub(crate) fn resolve_body(chain: &[&PartialBody]) -> Body {
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
pub(crate) struct ThemeFonts {
    major: [String; 3],
    minor: [String; 3],
}

impl ThemeFonts {
    /// An `a:theme`'s font scheme; Office's (Calibri Light, Calibri) where it has no latin one.
    pub(crate) fn of(theme: Option<&Element>) -> ThemeFonts {
        let scheme =
            theme.and_then(|t| t.path(&[(MAIN_A, "themeElements"), (MAIN_A, "fontScheme")]));
        let fonts = |group: &str, latin: &str| {
            let face = |slot: &str| {
                scheme
                    .and_then(|s| s.path(&[(MAIN_A, group), (MAIN_A, slot)]))
                    .and_then(|font| font.attr("typeface"))
                    .unwrap_or_default()
                    .to_string()
            };
            let first = face("latin");
            [
                if first.is_empty() {
                    latin.to_string()
                } else {
                    first
                },
                face("ea"),
                face("cs"),
            ]
        };
        ThemeFonts {
            major: fonts("majorFont", "Calibri Light"),
            minor: fonts("minorFont", "Calibri"),
        }
    }

    /// A typeface as text names it, a theme reference resolved.
    fn resolve(&self, typeface: &str) -> String {
        let (group, slot) = match typeface {
            "+mj-lt" => (&self.major, 0),
            "+mj-ea" => (&self.major, 1),
            "+mj-cs" => (&self.major, 2),
            "+mn-lt" => (&self.minor, 0),
            "+mn-ea" => (&self.minor, 1),
            "+mn-cs" => (&self.minor, 2),
            _ => return typeface.to_string(),
        };
        group[slot].clone()
    }
}

/// A level's style: the first that sets each property, in order, else the defaults (§8.6), with
/// theme references resolved.
pub(crate) fn resolve_level(chain: &[&Partial], theme: &ThemeFonts) -> Level {
    let defaults = Level::default();
    macro_rules! first {
        ($field:ident) => {
            chain.iter().find_map(|p| p.$field.clone())
        };
    }
    let font =
        |name: Option<String>, default: &str| theme.resolve(name.as_deref().unwrap_or(default));
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
            latin: font(first!(latin), "+mn-lt"),
            east_asian: font(first!(east_asian), "+mn-ea"),
            complex: font(first!(complex), "+mn-cs"),
            bold: first!(bold).unwrap_or(defaults.run.bold),
            italic: first!(italic).unwrap_or(defaults.run.italic),
            caps: first!(caps).unwrap_or(defaults.run.caps),
            kern: first!(kern).unwrap_or(defaults.run.kern),
            spacing: first!(spacing).unwrap_or(defaults.run.spacing),
        },
    }
}

/// Each level resolved along `chain`.
pub(crate) fn resolve_levels(chain: &[&PartialLevels], theme: &ThemeFonts) -> Levels {
    std::array::from_fn(|i| {
        let levels: Vec<&Partial> = chain.iter().map(|levels| &levels[i]).collect();
        resolve_level(&levels, theme)
    })
}

/// A table style's text for the header row and for the others (§8.6): `a:tcTxStyle` of its first
/// row over its whole table's, and of its whole table's. Medium Style 2, which output falls back
/// to, sets the minor font for both and bold for the header.
pub(crate) fn table_text(styles: Option<&Element>, id: &str) -> (Partial, Partial) {
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
            None if id == MEDIUM_STYLE_2 => Partial {
                latin: Some("+mn-lt".into()),
                east_asian: Some("+mn-ea".into()),
                complex: Some("+mn-cs".into()),
                bold: (name == "firstRow").then_some(true),
                ..Partial::default()
            },
            None => Partial::default(),
        }
    };
    let whole = part("wholeTbl");
    let first = part("firstRow");
    let header = merge(&first, &whole);
    (header, whole)
}

/// A table style part's text properties: `b` and `i` (`on`, `off`, `def`), and its font or font
/// reference.
fn cell_text(style: &Element) -> Partial {
    let mut text = Partial {
        bold: style.attr("b").and_then(boolean),
        italic: style.attr("i").and_then(boolean),
        ..Partial::default()
    };
    if let Some(font) = style.child(MAIN_A, "font") {
        let typeface = |slot: &str| {
            font.child(MAIN_A, slot)
                .and_then(|f| f.attr("typeface"))
                .map(str::to_string)
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
            text.latin = Some(format!("+{group}-lt"));
            text.east_asian = Some(format!("+{group}-ea"));
            text.complex = Some(format!("+{group}-cs"));
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

/// A table cell's first level, from its table style's text, then the master's `p:otherStyle`, then
/// `p:defaultTextStyle`.
pub(crate) fn table_level(
    text: &Partial,
    other: &PartialLevels,
    default: &PartialLevels,
    theme: &ThemeFonts,
) -> Level {
    resolve_level(&[text, &other[0], &default[0]], theme)
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
            r#"<a:tabLst><a:tab pos="300"/><a:tab pos="x"/><a:tab pos="900"/></a:tabLst>"#,
            r#"<a:defRPr sz="1" b="1" i="0" cap="small" kern="1200" spc="-50"><a:latin typeface="+mj-lt"/><a:ea typeface="Brand Mincho"/></a:defRPr>"#,
            r#"</a:lvl1pPr><a:lvl2pPr><a:buFont typeface="+mn-lt"/><a:buSzPct val="80000"/><a:buChar char="-"/></a:lvl2pPr>"#,
        ));
        let theme = ThemeFonts {
            major: ["Major".into(), "Major EA".into(), String::new()],
            minor: ["Minor".into(), String::new(), String::new()],
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
            (
                run.latin.as_str(),
                run.east_asian.as_str(),
                run.complex.as_str()
            ),
            ("Major", "Brand Mincho", "")
        );
        let second = &levels[1];
        assert_eq!(second.bullet, Bullet::Char("-".into()));
        assert_eq!(second.bullet_font.as_deref(), Some("Minor"));
        assert_eq!(second.bullet_size, BulletSize::Percent(80_000));
        assert_eq!(second.run.latin, "Minor");
        // A level nothing sets is the defaults.
        assert_eq!(levels[2].line_spacing, Spacing::Percent(100_000));
        assert_eq!(levels[2].run.size, 1_800);
    }

    #[test]
    fn a_body_reads_wrap_columns_and_direction() {
        let body = |xml: &str| resolve_body(&[&super::body(Some(&element(xml).children[0]))]);
        assert!(!body(r#"<a:bodyPr wrap="none"/>"#).wrap);
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
        assert_eq!(number(Some("99999999999999999999999"), 0, 10), Some(10));
    }
}
