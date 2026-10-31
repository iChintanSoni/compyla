# Roadmap

Strategy: **developers first, teams second**, built **core first and spec-driven**. Contracts come
before the core, the core before renderers, renderers before hosts. Each phase has an exit criterion,
and nothing downstream starts until the phase it depends on meets its exit.

```
Phase 0  TypeScript prototype            done; now the reference/oracle
Phase A  Foundation: spec & contracts    done
Phase B  Rust core (parse + lint)        next
Phase C  Template-first renderers
Phase D  Hosts & first public release
Phase E  Developer growth
Phase F  Teams
```

## Phase 0 — TypeScript prototype (done)

Proved the pipeline end to end: markdown → tokens → validator → four native renderers, with
line-level diagnostics and byte-reproducible output. Retained as the reference implementation and
conformance oracle (architecture.md, "Reference Prototype").

- [x] Parser (CommonMark + GFM + frontmatter + directive fences) with source positions; base token model
- [x] Profile framework and validator with `--strict` escalation; formula allowlist
- [x] docx, pdf (`pdfkit`), pptx (`pptxgenjs`) and xlsx (`exceljs`) renderers, byte-reproducible
- [x] Template theme introspection and pptx layout alias mapping (`@compyla/ooxml`)
- [x] SDK layering (core → SDK → apps), CLI, self-hostable HTTP server, web lint UI
- [x] Quickstart, Pandoc comparison, docs site on GitHub Pages
- Not published to npm. Superseded: the first public release ships from the Rust core (Phase D).

What it taught us, now addressed by architecture decisions D10–D17 and markdown-spec 0.2:

- Templates contributed only two fonts, one color and placeholder boxes; masters, logos, headers and
  footers were lost → template-first rendering (D12).
- Fonts were fetched from the network on every render, unpinned and unsubsetted → lockfile (D14).
- Content was dropped silently in several places, two of them corrupting: `=B2*C2*D2` was written as
  `=B2C2D2`, and a `---` after a `###` subsection didn't split slides → corrected in spec 0.2 (§0.2).

## Phase A — Foundation: spec & contracts (done)

Goal: the contracts are complete enough that the Rust core can be written from them alone, without
reading the prototype.

- [x] Architecture re-founded on a Rust core: D10–D17 ([architecture.md](architecture.md))
- [x] Markdown spec 0.2 ([markdown-spec.md](markdown-spec.md)), covering:
      - the input rules and the precise directive grammar
      - the IR mapping with exact positions
      - the four profiles, with xlsx placement, formula grammar and pptx slot rules
      - operations, resolvers and templates
      - the severity policy, limits, security and determinism

      42 recorded changes (C1–C42) correct prototype defects and close the gaps found by two
      independent reviews.
- [x] IR schema 0.1 (`spec/ir.schema.json`)
- [x] Diagnostics registry, with 50 codes (each with severity, stage, strict promotion and
      position) and 6 operation errors (`spec/diagnostics.json`, plus its own schema)
- [x] Diagnostic object and template report schemas (`spec/diagnostic.schema.json`,
      `spec/template-report.schema.json`)
- [x] Conformance fixtures: parse, lint and compose suites (`spec/conformance/`, 83 fixtures).
      Expectations are regenerated from the prototype wherever it agrees with the spec, and
      hand-written where 0.2 corrects it.
- [x] Spec tooling: the prototype oracle (`spec/tools/oracle.mjs`) and the artifact checker,
      including schema validation (`spec/tools/check.mjs`)
- [x] Toolchain check 1: `comrak` + the directive pre-pass reproduce the parse suite's positions
      exactly. All 39 parse fixtures match: 120 node positions, 34 table cells, 12 link destinations,
      19 diagnostics. One spec change resulted (C43) ([spikes/README.md](../spikes/README.md))
- [x] Toolchain check 2: Typst compiled in-process from Rust gives byte-identical PDFs across separate
      runs, inside the §12.2 sandbox (hostile text stays literal; file, package and clock access are
      denied). Typst doesn't report missing glyphs, so `glyph_missing` is detected from the layout
- [x] Toolchain check 3: one core crate (with comrak) builds unchanged for napi-rs, PyO3 (one abi3 wheel)
      and `wasm32` (425 KB), and returns identical results from Node, Python and WASM. Verified on
      macOS arm64 and Linux arm64. CI runs it on Linux x64 and arm64, macOS and Windows
      (`.github/workflows/toolchain-checks.yml`). D10's revisit trigger did not fire
- [x] Render conformance format: semantic projections per format (styles by name, slides, cells,
      PDF text and outline), package assertions (required and forbidden parts, XPath), and a
      twice-rendered byte check within one implementation (`spec/conformance/README.md`,
      `render.schema.json`, `spec/tools/render_check.py`). 7 render fixtures, 3 of them catching
      prototype defects
- [x] CI guards the spec: `ci.yml` runs the artifact checker and the prototype oracle on every push

Exit criterion (met): all Phase A items checked. No toolchain check failed, so D10 and D13 stand.

## Phase B — Rust core (next)

Goal: `parse` and `lint` in Rust, passing the conformance suites.

- [ ] Cargo workspace (`crates/`), CI (fmt, clippy, test), MSRV policy
- [ ] `compyla-core` parser → IR; parse suite passes
- [ ] Validator, profiles and formula allowlist; lint suite passes
- [ ] IR JSON in/out; a test asserts the schema generated from the Rust types equals `spec/ir.schema.json`
- [ ] `compyla` facade: `parse` and `lint` (D8)
- [ ] First host: `compyla lint` CLI, plus the core-only WASM build

Exit criterion: both suites pass in Rust; the prototype is no longer needed as the oracle.

## Phase C — Template-first renderers

Goal: output that is faithful to a real corporate template, and identical bytes on every run.

- [ ] `compyla-ooxml`: template package model, deterministic writer, `inspect_template`
- [ ] `compyla-fonts`: lockfile, cache, measurement, subsetting (D14)
- [ ] Compose stage with measured overflow diagnostics (D15)
- [ ] pptx template-first, with a bundled house `.potx` (D12)
- [ ] docx template-first, with a bundled house `.dotx`; headers, footers and page numbers come from the template
- [ ] xlsx per markdown-spec.md §9: placement, typed values, parsed and allowlisted formulas (with
      `_xlfn.` prefixes), Excel tables, sheet-name repairs, and theme tokens from a supplied template
- [ ] pdf via Typst with a theme derived from the template (D13)
- [ ] Render conformance suite passing for all four formats
- [ ] Real-viewer verification in CI (test only, D4): OOXML schema validation + LibreOffice → PNG visual diffs

Exit criterion: a real corporate `.potx` and `.dotx` round-trip with masters, logos and headers/footers
intact, rendered twice with identical bytes.

## Phase D — Hosts & first public release

Goal: one engine, every channel (D10).

- [ ] Node host (napi-rs) and the full browser WASM build
- [ ] Python host (PyO3 + maturin)
- [ ] CLI: `convert`, `lint`, `template inspect`, `fonts sync`
- [ ] MCP server (D17): the four operations as tools; dialect spec, diagnostics registry and template
      reports as resources
- [ ] HTTP server with request-size, upload and decompression limits (template uploads are zip files)
- [ ] Move the web lint UI and docs site onto the Node/WASM host (D9), then retire `packages/`
- [ ] Docs rewrite: quickstart, per-host reference, MCP guide
- [ ] Publish 0.1 to npm, PyPI and crates.io, plus CLI binaries on GitHub Releases, with semver

Exit criterion: an external developer integrates through any host without talking to us.

## Phase E — Developer growth

- [ ] Plugin platform: the four extension points, with the mechanism decided first (likely WASM components, D7)
- [ ] Chart directive for pptx/xlsx (`::: chart bar` over a table), as native editable charts
- [ ] Layout inference v2: automatically splitting slides that measure as overflowing
- [ ] Spec 0.3 candidates (markdown-spec.md §15): GitHub alerts as callouts, math, footnotes, override blocks, heading IDs
- [ ] Compyla's **managed hosting** of the server (the paid open-core surface): auth, usage metering and billing
- [ ] GitHub Action for docs-as-deliverables pipelines
- [ ] HTML renderer (cheap thanks to the IR)

## Phase F — Teams

Built from observed usage (which templates, which formats, which failures):

- [ ] Template manager: upload, introspection report, alias mapping, sharing within an org. Open
      question: pull forward into Phase E as the first paid feature (agents across an org producing
      on-brand output)?
- [ ] Live preview editor (markdown on the left, rendered document on the right), on the full WASM build
- [ ] Org workspaces, roles, seat pricing alongside usage pricing
- [ ] Brand governance: locked templates, required frontmatter fields
- [ ] Untrusted plugin execution inside the hosted service (sandboxing)

## Explicit non-goals until further notice

- Office → markdown reverse conversion
- WYSIWYG editing of output files
