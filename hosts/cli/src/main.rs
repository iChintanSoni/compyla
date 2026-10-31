//! The `compyla` command. A thin host (architecture.md D8): argument parsing, file I/O and terminal
//! output only; everything else is the `compyla` facade.
//!
//! Exit codes (markdown-spec.md §10.8): 0 success, 1 error diagnostics, 2 an operation or usage error.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use compyla::{Asset, Diagnostic, Format, Options, Severity};

const USAGE: &str = "\
usage: compyla parse <file.md>
       compyla lint <file.md> [--format docx|pdf|pptx|xlsx] [--strict] [--json]
       compyla lint --ir <file.json> [--format ...] [--strict] [--json]

  parse   print the document's IR and parse diagnostics as JSON
  lint    check the document for a format; the format defaults to the frontmatter's `format`

  --ir       the input is IR JSON (markdown-spec.md §10.7) instead of markdown
  --strict   escalate every non-advisory warning to an error
  --json     print diagnostics as JSON instead of text";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(code) => code,
        Err(message) => {
            eprintln!("compyla: {message}");
            ExitCode::from(2)
        }
    }
}

fn run(args: &[String]) -> Result<ExitCode, String> {
    let (command, rest) = args.split_first().ok_or(USAGE)?;
    match command.as_str() {
        "parse" => parse(rest),
        "lint" => lint(rest),
        "--help" | "-h" | "help" => {
            println!("{USAGE}");
            Ok(ExitCode::SUCCESS)
        }
        "--version" | "-V" => {
            println!("compyla {}", env!("CARGO_PKG_VERSION"));
            Ok(ExitCode::SUCCESS)
        }
        other => Err(format!("unknown command `{other}`\n\n{USAGE}")),
    }
}

fn parse(args: &[String]) -> Result<ExitCode, String> {
    let [file] = args else {
        return Err(USAGE.into());
    };
    let source = read(Path::new(file))?;
    let result = compyla::parse(&source);
    println!(
        "{}",
        serde_json::to_string_pretty(&result).expect("IR serializes")
    );
    Ok(exit_for(&result.diagnostics))
}

fn lint(args: &[String]) -> Result<ExitCode, String> {
    let mut file: Option<PathBuf> = None;
    let mut options = Options::default();
    let mut json = false;
    let mut ir = false;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--strict" => options.strict = true,
            "--json" => json = true,
            "--ir" => ir = true,
            "--format" => {
                let name = args.next().ok_or("--format needs a value")?;
                options.format = Some(Format::parse(name).ok_or_else(|| {
                    format!("format_unknown: `{name}` isn't docx, pdf, pptx or xlsx")
                })?);
            }
            flag if flag.starts_with('-') => {
                return Err(format!("unknown option `{flag}`\n\n{USAGE}"));
            }
            path if file.is_none() => file = Some(PathBuf::from(path)),
            _ => return Err(USAGE.into()),
        }
    }
    let file = file.ok_or(USAGE)?;
    let source = read(&file)?;
    let base = file.parent().map(Path::to_path_buf).unwrap_or_default();
    let assets = move |src: &str| resolve_asset(&base, src);
    let result = if ir {
        compyla::lint_ir(&source, &options, &assets)
    } else {
        compyla::lint(&source, &options, &assets)
    };
    let diagnostics = result.map_err(|e| format!("{}: {}", e.code, e.message))?;

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({ "diagnostics": diagnostics }))
                .expect("diagnostics serialize")
        );
    } else {
        for d in &diagnostics {
            print_diagnostic(&file, d);
        }
        let errors = diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .count();
        let warnings = diagnostics.len() - errors;
        eprintln!(
            "{}: {errors} error(s), {warnings} warning(s)",
            file.display()
        );
    }
    Ok(exit_for(&diagnostics))
}

/// Reads the input as UTF-8 (markdown-spec.md §1.1); invalid bytes are the `input_not_utf8` error.
fn read(path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("can't read {}: {e}", path.display()))?;
    String::from_utf8(bytes)
        .map_err(|_| format!("input_not_utf8: {} isn't valid UTF-8", path.display()))
}

/// The CLI's asset resolver (§10.3): relative references resolve against the input file's directory.
/// Remote URLs aren't fetched; for `lint` they count as existing.
fn resolve_asset(base: &Path, src: &str) -> Asset {
    if src.starts_with("http://") || src.starts_with("https://") {
        return Asset::Exists;
    }
    match std::fs::read(base.join(src)) {
        Ok(bytes) => Asset::Bytes(bytes),
        Err(_) => Asset::NotFound,
    }
}

fn print_diagnostic(file: &Path, d: &Diagnostic) {
    let location = match (d.line, d.column) {
        (Some(line), Some(column)) => format!("{}:{line}:{column}", file.display()),
        (Some(line), None) => format!("{}:{line}", file.display()),
        _ => file.display().to_string(),
    };
    let severity = match d.severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
    };
    println!("{location}: {severity}[{}]: {}", d.code, d.message);
    if let Some(hint) = &d.hint {
        println!("  hint: {hint}");
    }
}

fn exit_for(diagnostics: &[Diagnostic]) -> ExitCode {
    if diagnostics.iter().any(|d| d.severity == Severity::Error) {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}
