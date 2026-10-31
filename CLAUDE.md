# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

Compyla converts markdown (CommonMark + GFM + frontmatter + `:::` directives) into docx, pdf, pptx and
xlsx, each written natively. Output must be byte-reproducible.

The repository holds three implementations of differing status:

- **`crates/` + `hosts/`: the Rust implementation (architecture.md D10).** All new work goes here.
- **`packages/`: the TypeScript prototype.** It's frozen as the reference and the conformance
  *oracle*, and it gets retired in Phase D. Don't add features to it.
- **`spikes/`: the Phase A toolchain checks** (comrak positions, Typst determinism, napi/PyO3/WASM
  bindings). It's a separate Cargo workspace, excluded from the main one.

Current phase and open work: `docs/roadmap.md`. Every open item there is a GitHub issue.

## Spec first

The spec is the source of truth, not the code (D16). It consists of:

- `docs/markdown-spec.md`: the dialect, the IR mapping, the four format profiles, operations,
  diagnostics, limits and determinism. Sections are cited as `§n`. Its change log in §0.2 numbers
  each change `Cn`.
- `docs/architecture.md`: the decisions, numbered `Dn`.
- `spec/`: `ir.schema.json`, `diagnostics.json` (the registry of every diagnostic code), the other
  schemas, and `spec/conformance/` (fixtures that define correct behavior).

A behavior change goes in this order:

1. The spec text.
2. A C-numbered change-log entry.
3. The registry, if codes change.
4. Fixtures.
5. Code.

A behavior change without a fixture isn't done. The code cites `§`, `C` and `D` numbers in doc
comments throughout; keep doing so.

`compyla-core/src/diagnostic.rs` compiles in `spec/diagnostics.json` with `include_str!`. Emitting a
code that isn't in the registry panics, so register a new code before using it.

## Commands

Rust (run all of these before a PR; CI runs the same):

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace                     # the render suite shells out to `uv`; it must be on PATH
node spec/tools/check.mjs --schemas        # spec artifact consistency + JSON Schema validation (uses uvx)
node spec/tools/oracle.mjs                 # fixtures vs. the TS prototype; needs `pnpm install && pnpm build` first
```

Narrower runs:

```bash
cargo test -p compyla --test conformance lint_suite   # one suite: parse_suite, lint_suite, lint_ir_agrees_with_lint,
                                                      # compose_suite, render_suite, inspect_suite
cargo test -p compyla --test hostile                  # resource/limit regressions
cargo test -p compyla-xlsx --test render
cargo test -p compyla-core <test_name>                # unit tests live next to the code
cargo check --workspace --lib --bins                  # with the 1.89 toolchain: the MSRV check
cargo build --release -p compyla-wasm --target wasm32-unknown-unknown   # core-only WASM build
```

Each conformance suite is a single `#[test]` that loops over every fixture directory and reports all
failures by fixture name. You can't filter to one fixture, so read the failure list instead.

Using the CLI (`hosts/cli`):

```bash
cargo run -p compyla-cli -- lint examples/deck.md --format pptx [--strict] [--json]
cargo run -p compyla-cli -- convert examples/budget.md --format xlsx -o out.xlsx
cargo run -p compyla-cli -- template inspect some.potx
```

To print the projection the render suite compares for a rendered file:

```bash
uv run --no-project --with lxml==6.0.2 --with pypdf==6.1.1 python -I spec/tools/render_check.py project pptx out.pptx
```

TypeScript prototype (Node ≥ 22.13, pnpm 11): `pnpm install`, `pnpm build`, `pnpm test`,
`pnpm typecheck`.

## Architecture

The pipeline: **parse → IR → validate → compose → serialize**.

The layering is strict (D8): **core crates → the `compyla` facade → hosts**. Hosts (`hosts/cli`,
`hosts/wasm`) handle only argv, I/O and presentation. They call the facade and never
`compyla-core` directly.

- **`compyla-core`** holds the parser, the IR, validation and the per-format composed models. It does
  no I/O (D11). It never touches the filesystem or network. Image and template bytes come through
  resolvers the host supplies (`Asset`, the template resolver). That is what lets one core build for
  native, WASM, napi-rs and PyO3.
  - `prepass.rs` handles directive fences and markers, then comrak parses each segment. Directive
    bodies are segments, so no block crosses a directive.
  - `locate.rs` gives diagnostics IR paths.
  - `ir_input.rs` validates IR JSON submitted in place of markdown (§10.7).
  - **Compose lives in `validate/`** (D15). `validate::pptx::model` builds the `Deck` and
    `validate::xlsx` builds the `Workbook`. `lint` and `convert` share this one composed model, so
    they can't disagree. The renderer crates only serialize decisions already made there. Put
    placement, inference and fit logic in compose, not in a renderer.
- **`compyla-ooxml`** provides:
  - the deterministic zip writer (fixed timestamps and order, pure-Rust DEFLATE);
  - an XML writer that text can't escape from;
  - the package model;
  - the hostile-input template reader, which enforces §12.1 limits on the bytes and refuses macros,
    encryption, DTDs and unsafe paths;
  - theme extraction and `inspect_template`.
- **`compyla-pptx`** renders template-first (D12). The output *is* the template package with its
  sample slides replaced, and placeholders inherit layout geometry. With no template supplied, it
  uses the bundled `house/house.potx` (via `include_bytes!`).
- **`compyla-xlsx`** writes SpreadsheetML from the composed `Workbook`. It takes theme tokens from a
  supplied docx or pptx template (§9.2).
- **`compyla`** (the facade) provides `parse`, `lint`, `convert` and `inspect_template`. It also
  handles format resolution, strict escalation, diagnostic ordering and the cap (§11), and blocking
  decisions. Renderers are Cargo features (`pptx` and `xlsx`, both on by default). A format without
  a renderer returns `renderer_unavailable`. docx (#16) and pdf via Typst (#17) aren't built yet, so
  `render_suite` reports their fixtures as pending rather than failing.

## Conformance fixtures

Layout and rules: `spec/conformance/README.md`. Each fixture is `<suite>/<NNN-name>/` with
`input.md`, `case.json` and `expected.json`. Suites: parse, lint, compose, render and inspect.

- **Numbering:** `0xx` fixtures agree with the prototype (`"oracle": "prototype"`). `1xx` fixtures
  encode a spec correction (`"corrected"` or `"none"`, with a hand-written `expected.json`).
- **`node spec/tools/oracle.mjs --write`** regenerates parse, lint and compose expectations from the
  prototype. Review every rewritten file against the spec. Render expectations are always
  hand-written.
- **Diagnostics** are compared as `(code, severity, line, column, path)` in §11.2 order. `message` and
  `hint` are never compared.
- **Render fixtures** are rendered twice and must be byte-identical. The output is then checked
  through `spec/tools/render_check.py`, which projects the file into styles, slides, cells, text and
  XPath assertions and compares that.
- **Never hand-make binaries:**
  - The template packages for `inspect`, and for any other suite that needs one, come from
    `python3 -I spec/tools/make_templates.py`.
  - `house.potx` comes from `python3 -I crates/compyla-pptx/house/make_house.py`.

  Edit the script, regenerate, and commit both.

## Engineering rules

- **Determinism (§13):** the same input, template and fonts give the same bytes on every platform.
  Assign IDs in document order, write parts in a fixed order, and don't iterate over unordered maps
  when writing output.
- **Hostile input (§12):** every reader enforces its limits on the actual bytes, for example by
  stopping inflation at the limit. Panics, quadratic paths and unbounded memory are bugs; add
  regressions to `crates/compyla/tests/hostile.rs`.
- **Nothing is dropped silently.** Content that can't be represented produces a registered
  diagnostic. Errors mean no safe output is possible; everything else is a warning with a defined
  fallback, and `--strict` promotes those.
- **Toolchain:** `unsafe_code` is forbidden workspace-wide. MSRV is 1.89 (comrak's minimum), checked
  in CI. Edition is 2024.
- **Real viewers:** when output changes, open it in PowerPoint, Excel or Word. It must open with no
  repair prompt and pass a clean accessibility check.

## Workflow

- Every change starts from a GitHub issue, tracked on project #9 with milestones as phases.
- Branch from `main`. The PR closes the issue and fills in `.github/pull_request_template.md`.
- `main` is protected: changes arrive only through PRs with green checks, using rebase merges for
  linear history.
- Every review finding is either fixed in the PR, as "Fix the review of …" commits, or filed as an
  issue. Nothing moves to the next milestone while one is still open.
- Commit subjects are imperative and at most 72 characters. The body says what changed and why.
