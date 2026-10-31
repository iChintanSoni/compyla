# Phase A Toolchain Checks

Three checks that had to pass before Phase B starts (docs/roadmap.md). Each one guards an architecture
decision; a failure would have reopened it. All three pass. Each check is a small crate in this
workspace, plus a script that builds it and verifies the result. CI runs them on every change
(`.github/workflows/toolchain-checks.yml`).

```bash
cd spikes
cargo run --release -p comrak-positions   # check 1
bash typst-determinism/verify.sh          # check 2
bash bindings/verify.sh                   # check 3
```

Requirements: Rust stable with the `wasm32-unknown-unknown` target, Node, and `uv` (for maturin, pypdf,
and the Python host).

## Results

| Platform | Check 1 | Check 2 | Check 3 |
|----------|---------|---------|---------|
| macOS arm64 (Rust 1.97.1, Node 24) | ✅ 39/39 fixtures | ✅ | ✅ Node, Python, WASM |
| Linux arm64, Debian 13 (Rust 1.99.1, Node 20) | ✅ 39/39 fixtures | ✅ | ✅ Node, Python, WASM |
| Linux x64, Linux arm64, macOS, Windows | CI (`toolchain-checks.yml`) | CI | CI |

## Check 1: comrak reproduces the parse suite's positions (D10, markdown-spec.md §1.3, §4)

`comrak-positions` implements the directive pre-pass (§1.3) and the structural part of the IR mapping
on top of `comrak` 0.59. Against every fixture in `spec/conformance/parse/`, it compares:
- the position of every IR block node (120 nodes)
- every table cell's `raw` text (34 cells)
- every link destination, including references resolved across directive bodies (12)
- every position-bearing parse diagnostic (19)

All 39 fixtures match.

Findings that shape Phase B:
- **Columns.** With `parse.sourcepos_chars`, comrak reports columns in Unicode scalar values, which is
  the spec's unit, so no conversion is needed.
- **Directive bodies keep absolute positions.** Each container (the document, or a directive body) is
  parsed from a text of the same length as the source: its own lines kept, everything else blank. Each
  child directive's opener becomes a one-line HTML comment placeholder. An HTML comment interrupts a
  paragraph and ends on its own line, which is exactly §1.3 rule 7, so no offset arithmetic is needed.
- **Document-wide link references** (§4.5) work through comrak's `broken_link_callback`, in two passes:
  collect the labels each container can't resolve, then resolve them against the whole document. That
  reuses comrak's own definition parsing, and the first definition wins.
- **Table `raw`** is re-split from the source rows (§4.6), because a parser's table tree normalizes
  cell counts.
- **One spec change:** comrak places an indented code block at its first content character, while
  remark placed it at column 1. Since an indented block has no opening delimiter, the spec adopted the
  content position (C43, fixture parse/125).
- `comrak::try_parse_document` supports cancellation, a candidate mechanism for the §12.1 limits.

## Check 2: Typst gives byte-identical, sandboxed PDFs (D13, markdown-spec.md §7, §12.2, §13)

`typst-determinism` compiles a document with Typst 0.15.1 in-process, through a `World` that exposes one
in-memory source and the embedded fonts, and nothing else. The document uses headings, an outline, a
table, page headers, numbering, hyphenation and Unicode text. The document text arrives through
`sys.inputs` as data. `verify.sh` runs the binary in two separate processes.

| Property | Result |
|----------|--------|
| Same process, fresh World | Identical bytes |
| Separate processes | Identical bytes (SHA-256 compared) |
| Separate operating systems | Identical bytes on macOS arm64 and Linux arm64 (`fcddec94…`): stronger than §13 requires |
| Creation or modification date | None written |
| Hostile text (`]#read("/etc/passwd")#[ and #import …`) passed as data | Rendered literally, never executed |
| `#read`, `#image` of a host file; `#import` of a package; `datetime.today()` | All fail: the World denies them |
| Outline and metadata | Both headings in the outline; title and author set |

Finding that shapes Phase C: **Typst does not report characters no font can draw.** CJK or emoji
without a covering font silently become empty boxes. The check therefore detects them itself, by
finding glyphs laid out as `.notdef` (glyph id 0) in the laid-out document, and it found `東`, `京` and
`😀`. markdown-spec.md §7 now specifies this for `glyph_missing`.

## Check 3: one core builds and runs in Node, Python and WASM (D10)

`bindings/core` stands in for `compyla-core`. It is pure and has no I/O, but it depends on the real
parser (comrak). Three thin hosts compile it unchanged:

| Host | Toolchain | Artifact |
|------|-----------|----------|
| Node | napi-rs 3 (`napi4`) | Native addon, loaded with `require` |
| Python | PyO3 0.30 + maturin 1.16, `abi3-py39` | One wheel for every CPython ≥ 3.9, installed into a throwaway environment and imported |
| Browser / WASM | wasm-bindgen 0.2.130 via wasm-pack 0.13.1 | 425 KB module (core + comrak, release with LTO), loaded through its generated JS |

`verify.sh` calls `summarize` through each host and requires byte-identical results.

Finding that shapes CI and release builds: **wasm-pack's prebuilt `wasm-bindgen` needs glibc 2.38 or
later.** It fails on Debian 12 (glibc 2.36) and works on Debian 13 and Ubuntu 24.04. CI therefore pins
Ubuntu 24.04 or later. Older build images would need `wasm-bindgen-cli` built from source.
