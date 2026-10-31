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

    pub fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }
}

/// Sorts per markdown-spec.md §11.2: line (absent first), column (absent first), code, path.
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
