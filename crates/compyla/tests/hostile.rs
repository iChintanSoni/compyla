//! Hostile input (markdown-spec.md §12.1): inputs that once took quadratic time. Each runs in well
//! under a second; the bound is generous so that only a return to quadratic behaviour fails.

use std::time::{Duration, Instant};

const BOUND: Duration = Duration::from_secs(10);

fn lint_within_bound(
    name: &str,
    source: &str,
    format: compyla::Format,
) -> Vec<compyla::Diagnostic> {
    let options = compyla::Options {
        format: Some(format),
        strict: false,
    };
    let start = Instant::now();
    let diagnostics = compyla::lint(source, &options, &compyla::no_assets).unwrap();
    let elapsed = start.elapsed();
    assert!(elapsed < BOUND, "{name} took {elapsed:?}");
    diagnostics
}

#[test]
fn repeated_headings_take_linear_time() {
    // Each duplicate's slug once searched its suffixes from -1.
    let source = "# a\n\n".repeat(40_000);
    lint_within_bound("duplicate headings", &source, compyla::Format::Docx);
}

#[test]
fn many_directive_options_take_linear_time() {
    let keys: Vec<String> = (0..80_000).map(|i| format!("k{i}")).collect();
    let source = format!("::: table {}\n| A |\n|---|\n| 1 |\n:::\n", keys.join(" "));
    lint_within_bound("many options", &source, compyla::Format::Xlsx);
}

#[test]
fn a_long_line_of_inline_nodes_takes_linear_time() {
    // Character columns were once recomputed from the start of the line for every node.
    let line: Vec<String> = (0..12_000).map(|i| format!("*é{i}*")).collect();
    let source = format!("{}\n", line.join(" "));
    lint_within_bound("long line", &source, compyla::Format::Docx);
}

#[test]
fn leading_notes_take_linear_time() {
    let source = format!(
        "# T\n\n{}{}",
        "::: notes\n:::\n".repeat(40_000),
        "::: layout Blank\n:::\n".repeat(40_000)
    );
    lint_within_bound("leading notes", &source, compyla::Format::Pptx);
}

#[test]
fn diagnostics_are_capped() {
    let source = format!(
        "::: columns {}\n| A |\n|---|\n| 1 |\n:::\n",
        "X,".repeat(100_000)
    );
    let diagnostics = lint_within_bound("many diagnostics", &source, compyla::Format::Xlsx);
    assert_eq!(diagnostics.len(), 1001);
    assert_eq!(diagnostics.last().unwrap().code, "diagnostics_truncated");
}

#[test]
fn a_huge_ir_position_doesnt_overflow() {
    let ir = serde_json::json!({
        "irVersion": "0.1",
        "meta": {},
        "body": [{
            "type": "table",
            "position": { "line": 4_294_967_295u64, "column": 1 },
            "header": [{ "runs": [{ "text": "A" }], "raw": "A" }],
            "align": [null],
            "rows": [[{ "runs": [{ "text": "=SUM(" }], "raw": "=SUM(" }]]
        }]
    });
    let options = compyla::Options {
        format: Some(compyla::Format::Xlsx),
        strict: false,
    };
    let diagnostics = compyla::lint_ir(&ir.to_string(), &options, &compyla::no_assets).unwrap();
    assert!(diagnostics.iter().any(|d| d.code == "formula_error"));
}
