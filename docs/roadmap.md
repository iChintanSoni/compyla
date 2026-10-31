# Roadmap

Strategy: **developers first, teams second**, built **core first and spec-driven**. Contracts come
before the core, the core before renderers, renderers before hosts. Each phase has an exit criterion,
and nothing downstream starts until the phase it depends on meets its exit.

```
Phase 0  TypeScript prototype            done; now the reference/oracle
Phase A  Foundation: spec & contracts    done
Phase B  Rust core (parse + lint)        done
Phase C  Template-first renderers        current
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

      44 recorded changes (C1–C44) correct prototype defects and close the gaps found by two
      independent reviews of the spec and a code review of its tooling.
- [x] IR schema 0.1 (`spec/ir.schema.json`)
- [x] Diagnostics registry, with 50 codes (each with severity, stage, strict promotion and
      position) and 6 operation errors (`spec/diagnostics.json`, plus its own schema)
- [x] Diagnostic object and template report schemas (`spec/diagnostic.schema.json`,
      `spec/template-report.schema.json`)
- [x] Conformance fixtures: parse, lint, compose and render suites (`spec/conformance/`).
      Expectations are regenerated from the prototype wherever it agrees with the spec, and
      hand-written where 0.2 corrects it.
- [x] Spec tooling: the prototype oracle (`spec/tools/oracle.mjs`) and the artifact checker,
      including schema validation (`spec/tools/check.mjs`)
- [x] Toolchain check 1: `comrak` + the directive pre-pass reproduce the parse suite's positions
      exactly. All 44 parse fixtures match: 128 node positions, 34 table cells, 13 link destinations,
      21 diagnostics. Directive bodies are parsed as segments, so no block crosses a directive. One
      spec change resulted (C43) ([spikes/README.md](../spikes/README.md))
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

## Phase B — Rust core (done)

Goal: `parse` and `lint` in Rust, passing the parse, lint and compose suites. The compose suite is
included because validation runs on the composed model (D15): pptx slide fit and xlsx placement are
part of linting.

- [x] Cargo workspace (`crates/`, `hosts/`) with an MSRV policy (Rust 1.89, comrak's own minimum),
      and CI running fmt, clippy (`-D warnings`), the conformance suites on Linux x64/arm64, macOS
      and Windows, the MSRV check, and the core-only WASM build (`.github/workflows/rust.yml`)
- [x] `compyla-core` parser → IR; the parse suite passes (44/44). Uses the toolchain-check design:
      directive segments, a document-wide link map, and frontmatter parsed from `saphyr-parser`
      events (chosen because `serde_yaml` is deprecated and `serde_yml` is unmaintained)
- [x] Validator and profiles for docx/pdf, pptx and xlsx; the lint and compose suites pass. lint and
      the compose summary share one composed model per format, so they can't disagree, and Phase C
      renders from the same model. The spec ambiguities found while implementing are settled in
      markdown-spec.md C45 (pptx) and C46 (xlsx)
- [x] IR out, with the Rust types proven to agree with the spec on the whole fixture corpus: every
      expected IR round-trips through the Rust types unchanged, and every produced IR and diagnostic
      validates against `spec/ir.schema.json` / `spec/diagnostic.schema.json`. (This replaces
      "schemars output equals the spec schema", which would be brittle without proving more.)
- [x] IR input for `lint` (§10.7): schema and invariant checks (`ir_invalid`,
      `ir_version_unsupported`), the text rules, and diagnostics that carry the IR `path` (§11.3).
      Paths come from `compyla_core::locate`, which renumbers the document so that each diagnostic's
      position names its node; markdown diagnostics get paths too. Every lint fixture's IR, with
      positions stripped, produces the same validate-stage diagnostics as its markdown, and every
      path resolves
- [x] `compyla` facade: `parse` and `lint` (D8), with format resolution, strict escalation,
      ordering and the diagnostics cap (§10.2, §11)
- [x] First hosts: the `compyla` CLI (`parse`, `lint`; exit codes per §10.8) and the core-only WASM
      build (`hosts/wasm`, 1.0 MB)

Exit criterion: the parse, lint and compose suites pass in Rust; the prototype is no longer needed as
the oracle for them.

## Phase C — Template-first renderers (current)

Goal: output that is faithful to a real corporate template, and identical bytes on every run.

- [x] `compyla-ooxml` writing: a deterministic zip writer (fixed timestamps and entry order,
      pure-Rust DEFLATE), XML that text can't escape from, the package model, and document
      properties from `meta` only (C44, §13)
- [x] `compyla-ooxml` templates: a package reader that enforces §12.1's limits on the bytes
      themselves (inflation stops at the limit), refuses what §10.4 refuses (macros, encryption,
      DTDs, unsafe paths), a namespace-aware XML reader, and `inspect_template` (C50) in the facade
      and CLI (`compyla template inspect`). The `inspect` suite has synthetic templates from
      `spec/tools/make_templates.py`, valid and hostile
- [ ] Template sanitization on output (§10.4: external relationships, ActiveX and OLE removed with
      `template_content_removed`), with the pptx and docx renderers
- [ ] `compyla-fonts`: lockfile, cache, measurement, subsetting (D14)
- [ ] Compose stage with measured overflow diagnostics (D15)
- [x] pptx with the bundled house `.potx` (D12; `crates/compyla-pptx`, template generated by
      `house/make_house.py`): slides written into the template package, placeholders inheriting
      their layout's geometry, text, tables, images (`contain`/`cover`, upright by EXIF
      orientation), links to slides, speaker notes, content and titles a layout has no placeholder
      for, and the template's unreachable parts and show settings removed (§8.8, C51–C53)
- [ ] pptx with a supplied template: the `template` option and resolver (§10.2, §10.3), layout
      binding by alias, name and type with `layout_not_in_template` (§8.5), and a notes master for
      templates without one
- [ ] docx template-first, with a bundled house `.dotx`; headers, footers and page numbers come from the template
- [x] xlsx per markdown-spec.md §9 (`compyla-xlsx`): placement, typed values, parsed and allowlisted
      formulas (with `_xlfn.` prefixes), Excel tables, sheet-name repairs, links and the house style
      (C48). The renderer only serializes the workbook the compose stage resolved, cell by cell
- [ ] xlsx theme tokens (fonts, header color) from a supplied docx or pptx template
- [x] `convert` in the facade and the CLI (`compyla convert`), with blocking (§11.1) decided over
      every diagnostic, including any the cap drops; formats without a renderer yet are
      `renderer_unavailable`
- [ ] pdf via Typst with a theme derived from the template (D13)
- [ ] Render conformance suite passing for all four formats. The Rust runner (`render_suite`) renders
      each fixture twice for byte identity and checks it with `render_check.py`; xlsx and pptx pass
      (render/002, 003, 103–116), docx and pdf are pending
- [ ] `convert` locates render-stage diagnostics through the same `locate` renumbering as `lint`
- [ ] Real-viewer verification in CI (test only, D4): OOXML schema validation + LibreOffice → PNG visual diffs

Exit criterion: a real corporate `.potx` and `.dotx` round-trip with masters, logos and headers/footers
intact, rendered twice with identical bytes.

## Phase D — Hosts & first public release

Goal: one engine, every channel (D10).

- [ ] Node host (napi-rs) and the full browser WASM build
- [ ] Python host (PyO3 + maturin)
- [ ] CLI: `fonts sync` (`lint`, `convert` and `template inspect` arrived in Phases B and C)
- [ ] MCP server (D17): the four operations as tools; dialect spec, diagnostics registry and template
      reports as resources
- [ ] HTTP server with request-size, upload and decompression limits (template uploads are zip files)
- [ ] Resource hardening before the server is public: fuzz every operation for panics, hangs and
      memory growth. Phase B's review found a panic, four quadratic paths and unbounded diagnostic
      memory by hand; all are fixed, with regression tests in `crates/compyla/tests/hostile.rs`.
      Also set a memory budget at the 10 MiB input limit: peak memory is about 0.7 GB today, mostly
      comrak's AST during parsing, which is too much for a shared server
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
