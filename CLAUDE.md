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
  bindings). It's a separate Cargo workspace, excluded from the main one. It still matters: any
  change under `spec/` triggers the Toolchain checks workflow. Its check 1 reproduces every parse
  fixture's positions, raw text, links and positional diagnostics independently.

Current phase and open work: `docs/roadmap.md`. Every open item there is a GitHub issue.

`README.md`, `docs/cli.md` and `docs/quickstart.md` still describe the prototype (their rewrite is
#28). For the Rust CLI, use `cargo run -p compyla-cli -- --help`.

## Spec first

The spec is the source of truth, not the code (D16). It consists of:

- `docs/markdown-spec.md`: the dialect, the IR mapping, the four format profiles, operations,
  diagnostics, limits and determinism. Sections are cited as `§n`. While 0.2 is in draft, each change
  is a numbered row (`Cn`) in its change table, §0.2.
- `docs/architecture.md`: the decisions, numbered `Dn`.
- `spec/`: `ir.schema.json`, `diagnostics.json` (the registry of every diagnostic code), the other
  schemas, and `spec/conformance/` (fixtures that define correct behavior).

A behavior change goes in this order:

1. The spec text.
2. A C-numbered row in the change table. Its Fixtures column must cite existing `suite/NNN`
   fixtures.
3. The registry and schemas, where affected. A new code must also appear as `` `code` `` in
   markdown-spec.md.
4. Fixtures. Each `case.json`'s `spec` list must name existing sections or C-numbers.
5. Code.

`node spec/tools/check.mjs` enforces the cross-references in steps 2–4. A behavior change without a
fixture isn't done. The code cites `§`, `C` and `D` numbers in doc comments throughout; keep doing so.

`crates/compyla-core/src/diagnostic.rs` compiles in `spec/diagnostics.json` with `include_str!`.
Emitting a code that isn't in the registry panics, so register a new code before using it.

## Commands

Rust (run all of these before a PR; CI runs the same, with `cargo fmt --all --check`):

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings   # dbg! and todo! are lint warnings, so they fail
cargo test --workspace                     # the render suite shells out to `uv`; it must be on PATH
node spec/tools/check.mjs --schemas        # spec artifact consistency + JSON Schema validation (uses uvx)
node spec/tools/oracle.mjs                 # fixtures vs. the TS prototype; needs `pnpm install && pnpm build`, and uv
```

Narrower runs:

```bash
cargo test -p compyla --test conformance lint_suite   # one suite: parse_suite, lint_suite, lint_ir_agrees_with_lint,
                                                      # compose_suite, render_suite, inspect_suite
cargo test -p compyla --test hostile                  # resource/limit regressions
cargo test -p compyla-xlsx --test render
cargo test -p compyla-core <test_name>                # unit tests live next to the code
cargo +1.89 check --workspace --lib --bins            # the MSRV check (rustup toolchain install 1.89)
cargo build --release -p compyla-wasm --target wasm32-unknown-unknown   # core-only WASM build
(cd spikes && cargo run --release -p comrak-positions)   # after adding or changing a parse fixture
```

Each conformance suite is a single `#[test]` that loops over every fixture directory and reports all
failures by fixture name, so `cargo test` can't select one fixture. `render_suite` prints its pending
fixtures only with `-- --nocapture`. To reproduce one fixture by hand, run its `input.md` through the
CLI. The CLI resolves assets from the filesystem, not from the fixture's `assets` list.

Using the CLI (`hosts/cli`). Write output outside the repo: without `-o`, `convert` writes next to
its input.

```bash
cargo run -p compyla-cli -- parse spec/conformance/parse/001-sections/input.md
cargo run -p compyla-cli -- lint examples/deck.md --format pptx [--strict] [--json]
cargo run -p compyla-cli -- convert examples/budget.md --format xlsx -o /tmp/out.xlsx
cargo run -p compyla-cli -- template inspect some.potx
```

To print the projection the render suite compares for a rendered file, or to check it against a
fixture:

```bash
uv run --no-project --with lxml==6.0.2 --with pypdf==6.1.1 python -I spec/tools/render_check.py project pptx /tmp/out.pptx
uv run --no-project --with lxml==6.0.2 --with pypdf==6.1.1 python -I spec/tools/render_check.py check pptx <expected.json> /tmp/out.pptx
```

TypeScript prototype (Node ≥ 22.13, pnpm 11): `pnpm install`, `pnpm build`, `pnpm test`,
`pnpm typecheck`.

## Architecture

The pipeline: **parse → IR → validate → compose → serialize**.

The layering is strict (D8): **core crates → the `compyla` facade → hosts**. Hosts (`hosts/cli`,
`hosts/wasm`) handle only argv, I/O and presentation. They call the facade and never
`compyla-core` directly.

- **`compyla-core`** holds the parser, the IR, validation and the per-format composed models. It does
  no I/O (D11) and never touches the filesystem or network. Image bytes come through the host's
  asset resolver (`Fn(&str) -> Asset`). Of a template, core sees only its layouts
  (`validate::pptx::layout::TemplateLayouts`). That is what lets one core build for native, WASM,
  napi-rs and PyO3.
  - `prepass.rs` handles directive fences and markers, then comrak parses each segment. Directive
    bodies are segments, so no block crosses a directive.
  - `locate.rs` gives diagnostics IR paths.
  - `ir_input.rs` validates IR JSON submitted in place of markdown (§10.7).
  - **Compose lives in `validate/`** (D15). `validate::pptx::model` builds the `Deck` and
    `validate::xlsx` builds the `Workbook`. `lint` and `convert` share this one composed model, so
    they can't disagree. Put placement, inference and fit logic in compose, not in a renderer. The
    renderers serialize what compose decided. One exception today: `compyla-pptx` binds layouts
    itself when it renders into the house template.
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
  handles:
  - template resolution, reading and acceptance (§10.2–§10.4);
  - format resolution;
  - strict escalation, diagnostic ordering and the cap (§11);
  - blocking decisions.

  Renderers are Cargo features (`pptx` and `xlsx`). They're on by default in the crate, but the
  workspace dependency turns default features off, so each host enables the ones it needs, as
  `hosts/cli` does. `convert` to a format the build can't render fails with the operation error
  `renderer_unavailable`; `lint` works for every format. docx (#16) and pdf via Typst (#17) aren't
  built yet, so `render_suite` reports their fixtures as pending rather than failing.

## Conformance fixtures

Layout and rules: `spec/conformance/README.md`. Each fixture is `<suite>/<NNN-name>/` with
`input.md`, `case.json`, `expected.json`, and any asset or template files it uses. Suites: parse,
lint, compose, render and inspect. An `inspect` fixture has no `input.md`; its `case.json` names its
`template`.

- **Numbering:** `0xx` fixtures agree with the prototype (`"oracle": "prototype"`). `1xx` fixtures
  encode a spec correction, with a hand-written `expected.json`. They come in two kinds:
  - `"corrected"`: the oracle checks that the prototype really gives something different, so CI
    fails if it doesn't;
  - `"none"`: the prototype can't produce it, and the oracle skips it.

  The prototype emits no IR paths, so a fixture whose only correction is a `path` is `none`.
- **`node spec/tools/oracle.mjs --write`** rewrites `expected.json` for every `prototype` parse, lint
  and compose fixture at once. It never writes `corrected`, `none` or render fixtures. Review the
  diff against the spec, and revert rewrites unrelated to your change.
- **Diagnostics** are compared in §11.2 order:
  - `code`, `severity` and `line` always (an absent line must match an absent line);
  - `column` and `path` only where the expected diagnostic has them;
  - `message` and `hint` never;
  - an inspect fixture's `issues` only as `(code, severity)`.

  Put a `column` in an expectation only where the code's registry `position` has one.
- **Render fixtures** are rendered twice and must be byte-identical.
  - `spec/tools/render_check.py` then projects the file into styles, slides, cells, text and XPath
    assertions.
  - Only the keys a fixture lists under `output` are checked.
  - Paragraphs and texts match as in-order subsequences.
- **Never hand-make template packages.** Edit the generating script, regenerate, and commit both:
  - the template packages for `inspect`, and for any other suite that needs one, come from
    `python3 -I spec/tools/make_templates.py`, which regenerates every one of them at once;
  - `house.potx` comes from `python3 -I crates/compyla-pptx/house/make_house.py`.

## Engineering rules

- **Determinism (§13):** the same input, template and fonts give the same bytes on every platform.
  Assign IDs in document order, write parts in a fixed order, and don't iterate over unordered maps
  when writing output.
- **Hostile input (§12):** every reader enforces its limits on the actual bytes, for example by
  stopping inflation at the limit. Panics, quadratic paths and unbounded memory are bugs; add
  regressions to `crates/compyla/tests/hostile.rs`.
- **Nothing is dropped silently.** Content that can't be represented produces a registered
  diagnostic. Errors mean no safe output is possible. Everything else is a warning with a defined
  fallback. Register a warning as advisory (`strictPromotable: false`) only if the output stays
  faithful to the input. Every other warning is strict-promotable, and `--strict` makes it an error
  (§11.1).
- **Toolchain:** `unsafe_code` is forbidden workspace-wide. MSRV is 1.89 (comrak's minimum), checked
  in CI. Edition is 2024.
- **Real viewers:** output that changes must open in PowerPoint, Excel or Word with no repair prompt
  and a clean accessibility check. An agent can't confirm that. Say in the PR what output changed,
  and leave the PR checklist's real-viewer item for the user.

## Workflow

- Every change starts from a GitHub issue, tracked on project #9 with milestones as phases.
- Branch from `main`. The PR closes the issue and fills in `.github/pull_request_template.md`.
- Never push to `main`. Merge only through a PR whose checks are all green, with a rebase merge (the
  only method enabled), so history stays linear. Confirm CI yourself before merging; don't count on
  branch protection to stop a red merge.
- Every review finding is either fixed in the PR, as "Fix the review of …" commits, or filed as an
  issue. Nothing moves to the next milestone while one is still open.
- Commit subjects are imperative and at most 72 characters. The body says what changed and why.
