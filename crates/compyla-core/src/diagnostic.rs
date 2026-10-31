//! Diagnostics (markdown-spec.md §11). Every code's default severity and strict promotion come from
//! the registry, `spec/diagnostics.json`, compiled in, so the registry stays the single source of
//! truth.

use std::collections::HashMap;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

const REGISTRY: &str = include_str!("../../../spec/diagnostics.json");

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
}

/// A relative JSON Pointer (`0/…`), as [`Diagnostic::within`] writes; an absolute one is empty or
/// starts with `/`.
pub(crate) fn is_relative(path: &str) -> bool {
    !path.is_empty() && !path.starts_with('/')
}

/// The diagnostic object (`spec/diagnostic.schema.json`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    pub code: String,
    pub severity: Severity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub struct CodeInfo {
    pub severity: Severity,
    pub strict_promotable: bool,
}

#[derive(Deserialize)]
struct Registry {
    diagnostics: Vec<RegistryEntry>,
}

#[derive(Deserialize)]
struct RegistryEntry {
    code: String,
    severity: Severity,
    #[serde(rename = "strictPromotable")]
    strict_promotable: bool,
}

fn registry() -> &'static HashMap<String, CodeInfo> {
    static REGISTRY_MAP: OnceLock<HashMap<String, CodeInfo>> = OnceLock::new();
    REGISTRY_MAP.get_or_init(|| {
        let registry: Registry =
            serde_json::from_str(REGISTRY).expect("spec/diagnostics.json is valid");
        registry
            .diagnostics
            .into_iter()
            .map(|e| {
                (
                    e.code,
                    CodeInfo {
                        severity: e.severity,
                        strict_promotable: e.strict_promotable,
                    },
                )
            })
            .collect()
    })
}

/// The registry entry for a code. Panics on an unregistered code: that is a bug, not an input
/// problem, and the conformance tests exercise every code the implementation emits.
pub fn code_info(code: &str) -> CodeInfo {
    *registry()
        .get(code)
        .unwrap_or_else(|| panic!("diagnostic code `{code}` is not in spec/diagnostics.json"))
}

impl Diagnostic {
    /// A diagnostic with the code's registered default severity.
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_string(),
            severity: code_info(code).severity,
            line: None,
            column: None,
            path: None,
            message: message.into(),
            hint: None,
        }
    }

    pub fn at(mut self, line: usize, column: Option<usize>) -> Self {
        self.line = Some(line);
        self.column = column;
        self
    }

    pub fn at_line(self, line: usize) -> Self {
        self.at(line, None)
    }

    /// The diagnostic concerns a part of the node at its line: `pointer` (JSON Pointer reference
    /// tokens, e.g. `3` or `header/3`) below that node. Kept as a relative JSON Pointer until a
    /// [`crate::locate::Locator`] resolves it against the node's path (§11.3).
    pub fn within(mut self, pointer: &str) -> Self {
        self.path = Some(format!("0/{pointer}"));
        self
    }

    pub fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }
}

/// Text that isn't the engine's (a template's names and targets, a parser's reason, a document's
/// layout name) as a message quotes it: at most `most` characters, with control and bidi
/// formatting characters escaped, so it can't write to a terminal or reorder the message's line
/// (markdown-spec.md §10.4).
pub fn quoted(text: &str, most: usize) -> String {
    let mut out = String::with_capacity(text.len().min(most.saturating_mul(4)));
    for (count, c) in text.chars().enumerate() {
        if count == most {
            out.push('…');
            break;
        }
        if c.is_control() || is_formatting(c) {
            out.extend(c.escape_unicode());
        } else {
            out.push(c);
        }
    }
    out
}

/// Characters that change how a line reads without showing: bidi controls, zero-width
/// characters, separators and the byte order mark.
pub fn is_formatting(c: char) -> bool {
    matches!(c, '\u{061C}' | '\u{200B}'..='\u{200F}' | '\u{2028}'..='\u{202E}' | '\u{2060}'..='\u{2064}' | '\u{2066}'..='\u{2069}' | '\u{FEFF}')
}

/// Sorts per markdown-spec.md §11.2: line (absent first), column (absent first), code, path.
/// How many diagnostics an operation reports before ending with `diagnostics_truncated` (§11.2).
pub const CAP: usize = 1000;

/// One more than [`CAP`], so a collector still shows that the cap was exceeded.
const KEEP: usize = CAP + 1;

/// Collects diagnostics in bounded memory (§12.1). An operation reports at most [`CAP`], errors
/// first ([`cap`]), so once many are collected, only the first `CAP + 1` in §11.2 order of each class
/// are kept: errors, strict-promotable warnings (errors under `strict`) and advisory warnings. What
/// is reported is exactly what keeping everything would report. A located sink maps each diagnostic
/// to its final position as it arrives (`crate::locate`), since the order depends on it.
#[derive(Debug, Default)]
pub struct Sink {
    items: Vec<Diagnostic>,
    locator: Option<crate::locate::Locator>,
    blocking: Blocking,
}

/// Whether diagnostics block `convert` (§11.1), over every diagnostic a sink saw, kept or not: an
/// error, or under `strict`, a strict-promotable warning.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Blocking {
    pub error: bool,
    pub promotable: bool,
}

impl Blocking {
    pub fn blocks(self, strict: bool) -> bool {
        self.error || (strict && self.promotable)
    }

    pub fn see(&mut self, diagnostic: &Diagnostic) {
        match diagnostic.severity {
            Severity::Error => self.error = true,
            Severity::Warning => self.promotable |= code_info(&diagnostic.code).strict_promotable,
        }
    }

    pub fn merge(self, other: Blocking) -> Blocking {
        Blocking {
            error: self.error || other.error,
            promotable: self.promotable || other.promotable,
        }
    }
}

impl Sink {
    pub fn new() -> Self {
        Self::default()
    }

    /// A sink for diagnostics on a renumbered document.
    pub fn located(locator: crate::locate::Locator) -> Self {
        Self {
            locator: Some(locator),
            ..Self::default()
        }
    }

    pub fn push(&mut self, mut diagnostic: Diagnostic) {
        if let Some(locator) = &self.locator {
            locator.locate(std::slice::from_mut(&mut diagnostic));
        }
        // A path still relative to its node (`within`) has nothing to resolve against.
        if diagnostic.path.as_deref().is_some_and(is_relative) {
            diagnostic.path = None;
        }
        self.blocking.see(&diagnostic);
        self.items.push(diagnostic);
        if self.items.len() >= 4 * KEEP {
            // Stable, like the final sort, so ties keep the order they arrived in.
            sort(&mut self.items);
            let mut kept = [0usize; 3];
            self.items.retain(|d| {
                let class = match d.severity {
                    Severity::Error => 0,
                    Severity::Warning if code_info(&d.code).strict_promotable => 1,
                    Severity::Warning => 2,
                };
                kept[class] += 1;
                kept[class] <= KEEP
            });
        }
    }

    pub fn into_vec(self) -> Vec<Diagnostic> {
        self.items
    }

    /// The diagnostics kept, and whether any seen blocks `convert`.
    pub fn into_parts(self) -> (Vec<Diagnostic>, Blocking) {
        (self.items, self.blocking)
    }
}

impl Extend<Diagnostic> for Sink {
    fn extend<I: IntoIterator<Item = Diagnostic>>(&mut self, diagnostics: I) {
        for diagnostic in diagnostics {
            self.push(diagnostic);
        }
    }
}

impl std::ops::Deref for Sink {
    type Target = [Diagnostic];

    fn deref(&self) -> &[Diagnostic] {
        &self.items
    }
}

/// The cap (§11.2), on sorted diagnostics: at most [`CAP`], errors before warnings: the first `CAP`
/// errors, then as many of the first warnings as fit, all kept in order. So when an error blocks
/// `convert`, the list shows one. Returns whether any were dropped.
pub fn cap(diagnostics: &mut Vec<Diagnostic>) -> bool {
    if diagnostics.len() <= CAP {
        return false;
    }
    let errors = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .count();
    let (mut errors_left, mut warnings_left) = (CAP, CAP.saturating_sub(errors));
    diagnostics.retain(|d| {
        let left = match d.severity {
            Severity::Error => &mut errors_left,
            Severity::Warning => &mut warnings_left,
        };
        let keep = *left > 0;
        *left = left.saturating_sub(1);
        keep
    });
    true
}

pub fn sort(diagnostics: &mut [Diagnostic]) {
    diagnostics.sort_by(|a, b| {
        (a.line, a.column, &a.code)
            .cmp(&(b.line, b.column, &b.code))
            .then_with(|| match (&a.path, &b.path) {
                (Some(a), Some(b)) => compare_paths(a, b),
                (a, b) => a.is_some().cmp(&b.is_some()),
            })
    });
}

/// JSON Pointers in document order (§11.2): token by token, array indices numerically and other
/// tokens as strings; a pointer comes before the longer ones it is a prefix of.
fn compare_paths(a: &str, b: &str) -> std::cmp::Ordering {
    let index = |token: &str| {
        (!token.is_empty() && token.bytes().all(|b| b.is_ascii_digit()))
            .then(|| token.trim_start_matches('0').len())
    };
    let (mut a, mut b) = (a.split('/'), b.split('/'));
    loop {
        match (a.next(), b.next()) {
            (None, None) => return std::cmp::Ordering::Equal,
            (None, Some(_)) => return std::cmp::Ordering::Less,
            (Some(_), None) => return std::cmp::Ordering::Greater,
            (Some(x), Some(y)) => {
                let order = match (index(x), index(y)) {
                    // Digits only, so comparing lengths (leading zeros aside), then text, is
                    // numeric comparison without overflow.
                    (Some(lx), Some(ly)) => lx
                        .cmp(&ly)
                        .then_with(|| x.trim_start_matches('0').cmp(y.trim_start_matches('0'))),
                    _ => x.cmp(y),
                };
                if order.is_ne() {
                    return order;
                }
            }
        }
    }
}

/// Applies `strict` (markdown-spec.md §11.1): strict-promotable warnings become errors.
pub fn escalate(diagnostics: &mut [Diagnostic], strict: bool) {
    if !strict {
        return;
    }
    for d in diagnostics {
        if d.severity == Severity::Warning && code_info(&d.code).strict_promotable {
            d.severity = Severity::Error;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cmp::Ordering::{Greater, Less};

    #[test]
    fn a_path_left_relative_is_dropped() {
        let mut sink = Sink::new();
        sink.push(Diagnostic::new("formula_error", "x").at_line(3).within("2"));
        assert_eq!(sink.into_vec()[0].path, None);
    }

    #[test]
    fn a_sink_keeps_what_sorting_everything_would() {
        // Lines in a scrambled order, with ties, well past the compaction bound.
        let all: Vec<Diagnostic> = (0..10_000usize)
            .map(|i| {
                let code = if i % 3 == 0 {
                    "heading_skip"
                } else {
                    "unknown_directive"
                };
                Diagnostic::new(code, format!("{i}")).at_line((i * 7919) % 2_500 + 1)
            })
            .collect();
        let mut sink = Sink::new();
        sink.extend(all.clone());
        let mut kept = sink.into_vec();
        sort(&mut kept);
        let mut expected = all;
        sort(&mut expected);
        // Between compactions a sink holds up to four times the bound; the first `KEEP` are exact.
        assert!((KEEP..4 * KEEP).contains(&kept.len()), "{}", kept.len());
        assert_eq!(kept[..KEEP], expected[..KEEP]);
    }

    #[test]
    fn the_cap_keeps_errors_first() {
        let mut diagnostics: Vec<Diagnostic> = (1..=1_500)
            .map(|line| Diagnostic::new("heading_skip", "w").at_line(line))
            .collect();
        diagnostics.push(Diagnostic::new("limit_exceeded", "e").at_line(2_000));
        assert!(cap(&mut diagnostics));
        assert_eq!(diagnostics.len(), CAP);
        assert_eq!(diagnostics.last().unwrap().code, "limit_exceeded");
        assert_eq!(diagnostics[CAP - 2].line, Some(999));
    }

    #[test]
    fn paths_compare_in_document_order() {
        assert_eq!(compare_paths("/body/2", "/body/10"), Less);
        assert_eq!(compare_paths("/body/2", "/body/2/children/0"), Less);
        assert_eq!(compare_paths("/body/0/rows/9/1", "/body/0/rows/10/0"), Less);
        assert_eq!(compare_paths("/meta/title", "/body/0"), Greater);
        assert_eq!(
            compare_paths(
                "/body/99999999999999999999999",
                "/body/100000000000000000000000"
            ),
            Less
        );
    }
}
