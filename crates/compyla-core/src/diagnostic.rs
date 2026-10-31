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
        (a.line, a.column, &a.code, &a.path).cmp(&(b.line, b.column, &b.code, &b.path))
    });
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
