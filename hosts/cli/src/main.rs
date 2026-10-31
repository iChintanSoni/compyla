//! The `compyla` command. A thin host (architecture.md D8): argument parsing, file I/O and terminal
//! output only; everything else is the `compyla` facade.
//!
//! Exit codes (markdown-spec.md §10.8): 0 success, 1 error diagnostics, 2 an operation or usage error.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use compyla::{Asset, Diagnostic, Format, Options, Severity};

const USAGE: &str = "\
usage: compyla parse <file.md>
       compyla lint <file> [--ir] [--format docx|pdf|pptx|xlsx] [--strict] [--json]
       compyla convert <file> [--ir] [--format ...] [--strict] [--json] [-o <output>]
       compyla template inspect <template>

  parse     print the document's IR and parse diagnostics as JSON
  lint      check the document for a format; the format defaults to the frontmatter's `format`
  convert   write the document in a format (pptx or xlsx in this build), next to the input by default
  template inspect   print a template's report as JSON: kind, theme, fonts, styles, layouts

  --ir       the input is IR JSON (markdown-spec.md §10.7) instead of markdown
  --strict   escalate every non-advisory warning to an error
  --json     print diagnostics as JSON instead of text
  -o         where convert writes its output";

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
        "convert" => convert(rest),
        "template" => match rest {
            [sub, file] if sub == "inspect" => inspect(Path::new(file)),
            _ => Err(USAGE.into()),
        },
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

/// The arguments `lint` and `convert` share.
struct Args {
    file: PathBuf,
    options: Options,
    json: bool,
    ir: bool,
    output: Option<PathBuf>,
}

fn arguments(args: &[String], output_allowed: bool) -> Result<Args, String> {
    let mut file: Option<PathBuf> = None;
    let mut options = Options::default();
    let (mut json, mut ir, mut output) = (false, false, None);
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
            "-o" | "--output" if output_allowed => {
                output = Some(PathBuf::from(args.next().ok_or("-o needs a path")?));
            }
            flag if flag.starts_with('-') => {
                return Err(format!("unknown option `{flag}`\n\n{USAGE}"));
            }
            path if file.is_none() => file = Some(PathBuf::from(path)),
            _ => return Err(USAGE.into()),
        }
    }
    Ok(Args {
        file: file.ok_or(USAGE)?,
        options,
        json,
        ir,
        output,
    })
}

fn lint(args: &[String]) -> Result<ExitCode, String> {
    let args = arguments(args, false)?;
    let source = read(&args.file)?;
    let base = args
        .file
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default();
    let assets = move |src: &str| resolve_asset(&base, src);
    let result = if args.ir {
        compyla::lint_ir(&source, &args.options, &assets)
    } else {
        compyla::lint(&source, &args.options, &assets)
    };
    let diagnostics = result.map_err(|e| format!("{}: {}", e.code, e.message))?;
    report(&args, &diagnostics);
    Ok(exit_for(&diagnostics))
}

fn convert(args: &[String]) -> Result<ExitCode, String> {
    let args = arguments(args, true)?;
    let source = read(&args.file)?;
    let base = args
        .file
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default();
    let assets = move |src: &str| resolve_asset(&base, src);
    let result = if args.ir {
        compyla::convert_ir(&source, &args.options, &assets)
    } else {
        compyla::convert(&source, &args.options, &assets)
    };
    let result = result.map_err(|e| format!("{}: {}", e.code, e.message))?;
    report(&args, &result.diagnostics);
    let Some(bytes) = result.bytes else {
        eprintln!(
            "{}: not converted: an error blocks the output",
            args.file.display()
        );
        return Ok(ExitCode::from(1));
    };
    let output = match (&args.output, result.format) {
        (Some(path), _) => path.clone(),
        (None, Some(format)) => args.file.with_extension(format.name()),
        (None, None) => unreachable!("bytes are rendered only once the format is resolved"),
    };
    std::fs::write(&output, bytes).map_err(|e| format!("can't write {}: {e}", output.display()))?;
    if !args.json {
        eprintln!("wrote {}", output.display());
    }
    Ok(ExitCode::SUCCESS)
}

fn report(args: &Args, diagnostics: &[Diagnostic]) {
    let file = &args.file;
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({ "diagnostics": diagnostics }))
                .expect("diagnostics serialize")
        );
    } else {
        for d in diagnostics {
            print_diagnostic(file, d);
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
}

fn inspect(file: &Path) -> Result<ExitCode, String> {
    // Read no more than one byte past the size limit (§12.1): the report refuses larger packages,
    // and a huge file shouldn't be read into memory to learn that.
    use std::io::Read;
    let limit = compyla::TEMPLATE_SIZE_LIMIT as u64 + 1;
    let mut bytes = Vec::new();
    std::fs::File::open(file)
        .and_then(|f| f.take(limit).read_to_end(&mut bytes))
        .map_err(|e| format!("can't read {}: {e}", file.display()))?;
    let report = compyla::inspect_template(&bytes);
    println!(
        "{}",
        serde_json::to_string_pretty(&report).expect("the report serializes")
    );
    let invalid = report["kind"].is_null();
    Ok(if invalid {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    })
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
