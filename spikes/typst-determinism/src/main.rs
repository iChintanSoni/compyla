//! Toolchain check 2 (docs/roadmap.md): Typst compiled in-process from Rust, inside the sandbox
//! markdown-spec.md §12.2 requires, gives byte-identical PDFs across runs.
//!
//! Usage: typst-determinism <out.pdf>
//! Prints the PDF's SHA-256. verify.sh runs it twice, in separate processes, and compares.
//!
//! In-process, it also checks the sandbox:
//! - document text reaches Typst only as data (`sys.inputs`) and is never interpreted as markup
//! - the World exposes no filesystem and no packages
//! - the World exposes no clock
//! - no creation date is written

use std::process::ExitCode;

use sha2::{Digest, Sha256};
use typst::diag::{FileError, FileResult, Severity};
use typst::foundations::{Bytes, Datetime, Dict, Duration, IntoValue, Smart};
use typst::syntax::{FileId, RootedPath, Source, VirtualPath, VirtualRoot};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Library, LibraryExt, World};
use typst_layout::PagedDocument;

/// Document text with markup a naive splice would execute. It must come out as literal text.
const HOSTILE: &str = "]#read(\"/etc/passwd\")#[ and #import \"@preview/cetz:0.3.0\": *";

const MAIN: &str = r#"
#set document(title: "Determinism check", author: "Compyla", date: none)
#set text(lang: "en", hyphenate: true)
#set page(paper: "a4", header: [Compyla], numbering: "1")
#outline()

= Heading One
#sys.inputs.body

#table(columns: 2, table.header[*Region*][*Revenue*], [APAC], [120000], [EMEA], [98000])

= Heading Two
Unicode text: café, naïve, Ελληνικά, Кириллица. A long paragraph to exercise line breaking and
hyphenation across the full width of the page so that the layout engine has real work to do.
"#;

/// A World that sees exactly one in-memory source and the embedded fonts: no filesystem, no
/// packages, no clock (markdown-spec.md §12.2).
struct SandboxWorld {
    library: LazyHash<Library>,
    book: LazyHash<FontBook>,
    fonts: Vec<Font>,
    main: Source,
}

impl SandboxWorld {
    fn new(main_text: &str, body: &str) -> Self {
        let mut inputs = Dict::new();
        inputs.insert("body".into(), body.into_value());
        let fonts: Vec<Font> = typst_assets::fonts()
            .flat_map(|data| Font::iter(Bytes::new(data)))
            .collect();
        let id = RootedPath::new(VirtualRoot::Project, VirtualPath::new("/main.typ").unwrap()).intern();
        Self {
            library: LazyHash::new(Library::builder().with_inputs(inputs).build()),
            book: LazyHash::new(FontBook::from_fonts(&fonts)),
            fonts,
            main: Source::new(id, main_text.to_string()),
        }
    }
}

impl World for SandboxWorld {
    fn library(&self) -> &LazyHash<Library> {
        &self.library
    }
    fn book(&self) -> &LazyHash<FontBook> {
        &self.book
    }
    fn main(&self) -> FileId {
        self.main.id()
    }
    fn source(&self, id: FileId) -> FileResult<Source> {
        if id == self.main.id() { Ok(self.main.clone()) } else { Err(FileError::AccessDenied) }
    }
    fn file(&self, id: FileId) -> FileResult<Bytes> {
        if id == self.main.id() { Ok(Bytes::from_string(self.main.clone())) } else { Err(FileError::AccessDenied) }
    }
    fn font(&self, index: usize) -> Option<Font> {
        self.fonts.get(index).cloned()
    }
    fn today(&self, _offset: Option<Duration>) -> Option<Datetime> {
        None
    }
}

/// Characters laid out with glyph id 0 (`.notdef`): what no available font could draw. Typst
/// itself doesn't warn about these, so `glyph_missing` (markdown-spec.md §7) is computed here.
fn missing_glyphs(document: &PagedDocument) -> Vec<String> {
    fn walk(frame: &typst::layout::Frame, out: &mut Vec<String>) {
        for (_, item) in frame.items() {
            match item {
                typst::layout::FrameItem::Group(group) => walk(&group.frame, out),
                typst::layout::FrameItem::Text(text) => {
                    for glyph in text.glyphs.iter().filter(|g| g.id == 0) {
                        let range = glyph.range.start as usize..glyph.range.end as usize;
                        out.push(text.text[range].to_string());
                    }
                }
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    for page in document.pages() {
        walk(&page.frame, &mut out);
    }
    out
}

fn render(world: &SandboxWorld) -> Result<(Vec<u8>, Vec<String>), String> {
    let warned = typst::compile::<PagedDocument>(world);
    let mut warnings: Vec<String> = warned.warnings.iter().map(|w| w.message.to_string()).collect();
    let document = match warned.output {
        Ok(document) => document,
        Err(errors) => return Err(join(&errors)),
    };
    let missing = missing_glyphs(&document);
    if !missing.is_empty() {
        warnings.push(format!("glyph_missing: {}", missing.join(" ")));
    }
    let options = typst_pdf::PdfOptions { ident: Smart::Custom("compyla-determinism-check".into()), ..Default::default() };
    let pdf = match typst_pdf::pdf(&document, &options) {
        Ok(pdf) => pdf,
        Err(errors) => return Err(join(&errors)),
    };
    Ok((pdf, warnings))
}

fn join(errors: &[typst::diag::SourceDiagnostic]) -> String {
    errors.iter().filter(|e| e.severity == Severity::Error).map(|e| e.message.to_string()).collect::<Vec<_>>().join("; ")
}

/// A document that tries to escape the sandbox must fail, and fail *because of the sandbox*: the
/// error must be the World's own denial, not an unrelated failure (such as the file not existing on
/// this host), so a sandbox regression can't pass.
fn expect_blocked(label: &str, main: &str, denial: &str) -> bool {
    match render(&SandboxWorld::new(main, "")) {
        Ok(_) => {
            eprintln!("FAIL sandbox: {label} compiled");
            false
        }
        Err(error) if error.contains(denial) => {
            eprintln!("ok   sandbox: {label} blocked ({error})");
            true
        }
        Err(error) => {
            eprintln!("FAIL sandbox: {label} failed, but not through the sandbox ({error})");
            false
        }
    }
}

fn main() -> ExitCode {
    let Some(out) = std::env::args().nth(1) else {
        eprintln!("usage: typst-determinism <out.pdf>");
        return ExitCode::from(2);
    };

    let world = SandboxWorld::new(MAIN, HOSTILE);
    let (first, warnings) = match render(&world) {
        Ok(result) => result,
        Err(error) => {
            eprintln!("FAIL compile: {error}");
            return ExitCode::FAILURE;
        }
    };
    for warning in &warnings {
        eprintln!("     typst warning: {warning}");
    }

    let mut ok = true;
    // Same process, fresh World: identical bytes.
    let (second, _) = render(&SandboxWorld::new(MAIN, HOSTILE)).expect("second render");
    if first != second {
        eprintln!("FAIL in-process renders differ");
        ok = false;
    } else {
        eprintln!("ok   in-process renders identical ({} bytes)", first.len());
    }
    if contains(&first, b"/CreationDate") || contains(&first, b"/ModDate") {
        eprintln!("FAIL PDF carries a creation or modification date");
        ok = false;
    } else {
        eprintln!("ok   no creation or modification date");
    }

    ok &= expect_blocked("#read of a host file", "#read(\"/etc/passwd\")", "access denied");
    ok &= expect_blocked("#image of a host file", "#image(\"/etc/hosts\")", "access denied");
    ok &= expect_blocked("#import of a package", "#import \"@preview/cetz:0.3.0\": *", "access denied");
    ok &= expect_blocked("datetime.today()", "#datetime.today().display()", "unable to get the current date");

    // markdown-spec.md §7 glyph_missing: Typst must report text no available font can draw.
    match render(&SandboxWorld::new("東京 😀", "")) {
        Ok((_, warnings)) if warnings.iter().any(|w| w.starts_with("glyph_missing: ") && w.contains('東') && w.contains('😀')) => {
            eprintln!("ok   missing glyphs detected from the layout (Typst itself doesn't warn): {}", warnings.join("; "));
        }
        Ok((_, warnings)) => {
            eprintln!("FAIL missing glyphs not reported (warnings: {warnings:?})");
            ok = false;
        }
        Err(error) => {
            eprintln!("FAIL glyph probe failed to compile: {error}");
            ok = false;
        }
    }

    std::fs::write(&out, &first).expect("write pdf");
    let hash: String = Sha256::digest(&first).iter().map(|b| format!("{b:02x}")).collect();
    println!("{hash}");
    if ok { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}
