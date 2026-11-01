# Architecture

> **Status (2026-10-31): re-founded on a Rust core (D10).** The TypeScript packages under `packages/`
> are now the **reference prototype**. They proved the pipeline end to end and still act as the
> oracle for the conformance fixtures wherever they agree with the spec. They are retired once the
> Rust core and its hosts pass the same fixtures (see [roadmap.md](roadmap.md), Phase D). Sections
> below describe the target architecture. Where the prototype differs, that is called out
> explicitly.

## Context and Goals

`Compyla` converts markdown into four output formats (docx, pdf, pptx, xlsx). LLM-generated
markdown is the dominant input. Users reach the engine through **every** channel: agents over MCP,
the CLI, an HTTP API, a library embedded in Node or Python apps, a browser or editor (live preview,
VS Code extension, offline web app), and Compyla's hosted product. The engine is shipped
**open-core**.

Design goals, in priority order:

1. A base dialect of plain CommonMark must produce reasonable output in any format with zero
   configuration; per-format styling only refines it.
2. Output is **faithful to a supplied template**: the template *is* the output's skeleton (D12), not
   a source of a few font and color values.
3. Failures happen at validation time with actionable, line-referenced errors, never at render time.
   Nothing the engine cannot represent is dropped silently.
4. Output is **byte-reproducible**: identical input, template and fonts yield identical bytes.
5. **One core, every platform**: the same engine runs natively in Node, Python, the browser, the
   CLI, the MCP server and the HTTP server (D10).
6. Adding an output format must not require touching the parser.

## Spec-First Contracts

The source of truth is the **spec**, not any implementation (D16). Implementations, including the
prototype, conform to it, and every host must pass the same fixtures.

| Artifact | Path | Role |
|----------|------|------|
| Dialect spec | [markdown-spec.md](markdown-spec.md) | Syntax, how markdown maps to the IR, per-format profiles |
| IR schema | `spec/ir.schema.json` | The public, versioned document model (JSON Schema 2020-12) |
| Diagnostics registry | `spec/diagnostics.json` | Every diagnostic code: severity, stage, strict promotion, meaning |
| Conformance fixtures | `spec/conformance/` | The executable definition of correct behavior |

Diagnostic *message wording* is not normative. Code, severity and source position are. Changes to
any of these artifacts follow the versioning rules in markdown-spec.md §14.

## High-Level Design

```
                 ┌────────────────────────── compyla core (no I/O, D11) ──────────────────────────┐
  markdown ─────▶│  parse ──▶ IR ──▶ validate ──▶ compose ──▶ serialize (per-format feature)        │──▶ bytes
  or IR JSON ───▶│              ▲         ▲            ▲              ▲                             │    + diagnostics
                 └──────────────┼─────────┼────────────┼──────────────┼─────────────────────────────┘
                                │   template model   font metrics   template package · font bytes · images
                                └───────────────────── host-supplied resolvers ───────────────────────
                  hosts:  Node (napi-rs) · Python (PyO3) · browser (WASM) · CLI · MCP · HTTP server
```

### Stage 1 — Parse

- CommonMark + GFM (tables, task lists, strikethrough, autolinks), YAML frontmatter, and Compyla
  directives (`::: name args` fences and `<!-- name -->` markers), as specified in
  markdown-spec.md §1–§2.
- Every IR node carries its 1-based source line and column. Diagnostics depend on it.
- Produces **parse diagnostics** for anything the IR cannot represent (`unsupported_content`),
  malformed frontmatter, and unclosed directive fences. Nothing is dropped silently.
- Format-agnostic: the parser knows nothing about output formats. Candidate crate: `comrak` (a
  port of GitHub's cmark-gfm, with full source positions). The directive grammar is a pre-pass,
  exactly as in the prototype.

### Stage 2 — Intermediate Representation (IR)

The IR is a format-agnostic document model of **base tokens** (D1), published as JSON and defined
by `spec/ir.schema.json`. Hosts may submit IR JSON directly instead of markdown. This is the path
for agents and programs that emit structured output.

```
Document { irVersion, meta, body }
└─ body: Node[]
   ├─ section    (heading level, title runs, children: the content it governs)
   ├─ paragraph  (inline runs: bold, italic, code, strike, link)
   ├─ table      (header cells, column alignment, rows; each cell has inline runs + raw source)
   ├─ list       (ordered, items with optional task-list checkbox state)
   ├─ media      (image src, alt)
   ├─ code       (language, content)
   ├─ quote      (children)
   ├─ break      (from ---; meaning assigned per profile: horizontal rule vs. slide)
   └─ directive  (name, args, children; resolved per profile)
```

Base tokens preserve *intent*, not layout. A `break` is not a page break or a slide break until a
profile interprets it.

### Stage 3 — Validate

Profile rules run over the IR, plus the **template model when one is supplied**. That lets pptx
layout names be checked against the template's real layouts, not just the built-in set. Examples:
rectangular xlsx tables, the formula allowlist, unique sheet names, heading structure, resolvable
images. Every diagnostic carries a registered code, a severity and a source position.

**Severity policy** (markdown-spec.md §11.1): an error means no safe or faithful output is possible
(an unresolvable brand template, a limit exceeded). Every other problem is a warning with a defined
fallback, so plain markdown always renders. Under `--strict`, every non-advisory warning becomes an
error, so strict means nothing was dropped, guessed or substituted. Validation runs fully offline and
is exposed as `lint`.

### Stage 4 — Compose (D15)

Each format gets its own composed model:

| Format | Composed model |
|--------|----------------|
| pptx | Slides bound to template layouts, content assigned to placeholders, text **measured** against placeholder boxes |
| xlsx | Sheets, placed ranges, typed cells, formulas with resolved addresses |
| docx / pdf | A flow of blocks bound to the template's named styles; page geometry from the template |

Composition-time diagnostics (measured overflow, font substitution, unbound layout) come from the
same code that places content, so validation and rendering can never disagree. Text measurement uses
the pinned font files (shaping via `rustybuzz`, D14). It is a diagnostic tool. Word and PowerPoint still do
the final layout of flowing text.

### Stage 5 — Serialize (one feature per format)

| Format | Approach |
|--------|----------|
| docx | **Template-first** (D12): the template's package is copied, its body replaced, and IR content written against its named styles |
| pptx | **Template-first** (D12): the template's sample slides are removed, and each composed slide is written against one of its real layouts |
| xlsx | Written by `compyla-xlsx` (its own writer, on the `compyla-ooxml` package model), with a house style plus theme tokens taken from a supplied docx/pptx template. Template-first xlsx, from an `.xltx`, comes later |
| pdf | **Typst** (D13): the composed flow becomes Typst markup with a theme generated from the template, compiled in-process |

No LibreOffice and no format-to-format conversion in the pipeline (D4).

## The Core Does No I/O (D11)

The core's API is pure: `(markdown | IR, options, resolvers) → (bytes?, diagnostics)`. Hosts supply
the resolvers:

| Resolver | Supplies |
|----------|----------|
| Asset | Image bytes for a `src` (also used by `lint` to check existence) |
| Template | Template package bytes |
| Font | Font bytes for a family/style, from the local font cache (D14) |

The core never touches the network or the filesystem. That is what lets one codebase compile to a
native binary, a napi-rs addon, a PyO3 extension and WebAssembly, and it is half of what makes
output reproducible.

## Operations

Every host exposes the same four operations, with the same names, inputs and outputs:

| Operation | Input | Output |
|-----------|-------|--------|
| `parse` | markdown | IR JSON + parse diagnostics |
| `lint` | markdown or IR, format, optional template, `strict` | diagnostics |
| `convert` | markdown or IR, format, optional template, `strict`, resolvers | bytes (absent on blocking error) + diagnostics |
| `inspect_template` | template bytes | template report: layouts, named styles, theme colors/fonts, referenced fonts |

All hosts share one diagnostics shape (see `spec/diagnostics.json`).

## Code Layout (planned)

```
crates/
  compyla-core     parser, IR, validator, profiles, formula allowlist (no renderer deps)
  compyla-ooxml    zip + XML, template package model, deterministic writer, template inspection
  compyla-fonts    font lockfile model, measurement, subsetting
  compyla-docx     template-first docx serializer
  compyla-pptx     template-first pptx serializer
  compyla-xlsx     xlsx serializer
  compyla-pdf      Typst-backed pdf serializer
  compyla          facade: the four operations, renderer dispatch, strict escalation (D8)
hosts/
  node             @compyla npm package (napi-rs native addon)
  wasm             browser builds: core-only (lint) and full (preview)
  python           compyla on PyPI (PyO3 + maturin)
  cli              the `compyla` binary
  mcp              MCP server (rmcp)
  server           HTTP server
spec/              the contracts above
packages/          TypeScript reference prototype (retired at the end of Phase D)
```

Layering is strict (D8): **core crates → `compyla` facade → hosts**. Hosts hold only their own I/O
and presentation: argv and terminal output, HTTP request handling, MCP tool plumbing, DOM. They never
reach past the facade.

## Template-First Rendering (D12)

The output package is the template package with generated content written into it:

- **docx**: the template's `styles.xml`, `numbering.xml`, theme, settings, headers, footers and final
  section properties (page size, margins, header/footer references) are kept. Only the body is
  replaced. IR nodes map to named styles by **style name** (`Heading 1`–`Heading 6`, `Normal`,
  `Quote`, a code style, the default table style), resolved to the template's own style IDs. A style
  the template lacks is added from the house template. `.dotx` input is written out as a `.docx`.
- **pptx**: the template's masters, layouts and theme are kept, and its sample slides are removed.
  Each composed slide is a new slide part referencing one of the template's layouts. Placeholders are
  written without position or formatting of their own, so they inherit geometry and style from the
  layout and master, which is how OOXML is designed. Logos, backgrounds, footers and fonts therefore
  carry over unchanged.
- **House templates are real bundled `.dotx` / `.potx` files.** With no template supplied, the same
  code path runs against the house file. There is one path, not "template layouts" vs. "house layouts
  defined in code".
- **Layout binding** (markdown-spec.md §8.5): explicit alias, then name match against the template's
  layouts, then a match by OOXML layout type (so localized templates bind), else the template's own
  Title and Content layout with a `layout_not_in_template` warning. A second master is never imported.
- **Sanitized** (markdown-spec.md §10.4): macro-enabled, encrypted or DTD-bearing templates are
  rejected. External relationships, ActiveX and embedded OLE objects are stripped, and document
  properties are written from the document, never copied from the template.

**Design tokens across formats.** From a docx or pptx template the engine reads theme fonts
(major/minor), theme colors and, for docx, page size and margins. These drive the pdf theme and the
xlsx header style, so one brand looks the same in all four outputs without a theming DSL (D3).

> *Prototype difference:* the TypeScript renderers generate fresh files with `docx` / `pptxgenjs` and
> take only two theme fonts, one accent color and (pptx) placeholder boxes from the template.
> Masters, backgrounds, logos, headers and footers are lost. D12 exists to fix this.

## Fonts (D14)

Fonts are never named in markdown (unchanged). Resolution moves **out of the render path**.
Installing a template (or running `compyla fonts sync`) resolves each family the template references
through the chain: (1) font embedded in the template, (2) user-supplied file, (3) **Google Fonts**
(OFL/Apache licensing permits redistribution). Each resolved file goes into a content-addressed local
cache and is recorded in a lockfile (`compyla.lock`: family, style, source, SHA-256). Renders read
fonts only through the font resolver, so they run **offline and pinned**. A family that cannot be
resolved produces `font_unresolved` (a warning, an error under `--strict`), and the viewer substitutes
a font. *Amended (C62):* a font a docx template embeds is read from the template itself rather than
pinned, `fonts sync` pins user files and Google Fonts, and whenever an operation runs the resolver
comes first.

Embedding: docx embeds fonts the way Word does, and pdf embeds subsets via Typst. pptx sets fonts by
name; pptx embedding is an open question.

*Amended 2026-11-01 (markdown-spec.md C62):* the design in detail.

- **House fonts.** The house templates name the fonts Office installs: Calibri, Calibri Light and
  Consolas. They look native wherever Office is, and nothing needs embedding. Office's fonts can't
  be redistributed, so the engine bundles an open-licence stand-in: **Carlito** (OFL),
  metric-compatible with Calibri, in its four styles, unmodified (its licence reserves the name, so
  a subset or a copy without outlines would need renaming). Carlito measures Calibri exactly and
  Calibri Light about 1–2% wide. Consolas is monospaced, so it is measured exactly by its fixed
  advance, with no font at all. Carlito also draws pdf text for Calibri and Calibri Light; docx,
  pptx and xlsx keep the Office names. The alternative, house templates in bundled OFL fonts, would
  measure exactly but make every house deck substitute fonts on machines without them until pptx
  embedding is solved.
- **Same everywhere.** Measurement is part of `lint` (D15), and `lint` must agree in every host, so
  Carlito is in every build, WASM included: 2.77 MB, and about 0.8 MB more for the shaper and
  Unicode tables (3.5 MB in all, 1.4 MB gzipped; measured in #73). Every distribution carries
  Carlito's copyright notice and licence, as the OFL requires.
- **One font stack.** `ttf-parser` parses and `rustybuzz` shapes, at the versions the pinned `typst`
  uses (0.25 and 0.20 with typst 0.15), so a face parses and shapes alike in measurement and in pdf.
  Both are pure Rust, build for every target at the MSRV, and never read installed fonts.
  Measurement serves the compose stage's diagnostics; a pdf is laid out by Typst (D13) with the
  faces it draws, so its line breaks may differ from what measurement assumed.
- **Measurement.** A run's width is its shaped advances in font units, scaled to the size in EMU
  with integer arithmetic. A character is measured with the face of its class (latin, East Asian,
  complex script), falling back to Carlito and then to its East Asian Width, so text in a script the
  face lacks isn't measured at half its width. Shaping uses the features the viewers apply (kerning
  where PowerPoint kerns, no ligatures). The result depends only on the text, the font bytes and the
  implementation version. Line breaks fall at UAX #14 opportunities; how lines fill a box, and what
  overflows, is the compose stage's (D15).
- **Resolution order.** The font resolver first, so a project can pin a full font where Word
  embedded a subset, or the real Calibri; then a font a docx template embeds; then a bundled
  stand-in. A family none of them resolves is `font_unresolved`, always at the validate stage so
  `lint` and `convert` agree, and is measured with Carlito. A family a pptx template embeds
  (Embedded OpenType, which the engine doesn't read) isn't reported, since PowerPoint shows it.
- **Lockfile and cache.** `compyla.lock` is JSON (`spec/lockfile.schema.json`): family, style,
  source and SHA-256, sorted. `fonts sync` may pin a metric-compatible open font under a template's
  family name (Arimo for Arial), and a single font file is used whatever its names say. The cache
  names files by SHA-256 and lives in the user's cache directory. Hosts read both; the core only
  ever sees bytes from the resolver (D11). A cached file whose digest doesn't match is not found, so
  a lockfile pins bytes, not names.
- **Hostile fonts.** A font file is bounded (§12.1), and one that doesn't parse counts as not found.
  `ttf-parser` is written not to panic on malformed input; `rustybuzz` makes no such promise and
  bounds its own work, so the measurement crate is fuzzed (#26) before untrusted fonts, such as a
  docx template's embedded ones, reach it in a release.

> *Prototype difference:* the prototype fetches Google Fonts on every docx/pptx render with no cache
> and does not subset, despite earlier docs claiming subsetting. Its output therefore depends on the
> network and on Google's file never changing.

## PDF via Typst (D13)

The composed flow model is emitted as Typst markup together with a Typst theme generated from the
template's tokens, then compiled in-process with the `typst` crate. Fonts come from the font
resolver. This provides font embedding and subsetting, full Unicode (including CJK and right-to-left
text), a table of contents with real page numbers, headers and footers, table headers repeated across
pages, and hyphenation. Determinism is enforced by a fixed document date and fixed inputs, and is
verified by a toolchain check before Phase C (roadmap).

Typst runs **sandboxed** (markdown-spec.md §12.2). Document text reaches it only as data, never
spliced into markup. Its file access exposes only the generated document, resolved assets and fonts,
and package downloads are disabled.

> *Prototype difference:* `@compyla/pdf` hand-lays pages on `pdfkit` with the 14 standard PDF fonts
> (WinAnsi only; hence ASCII task-list checkboxes), with no template binding and no font embedding.

## Key Decisions and Trade-offs

Decisions keep their numbers for life. Superseded or amended decisions say so in place.

### D1 — Shared base tokens + per-format override tokens
**Retained.** One parser and N renderers keeps the dialect coherent and makes new formats cheap. Base
tokens carry format-agnostic intent. Override tokens add per-format control without forking the
dialect. *Amended by markdown-spec.md 0.2:* the override block syntax is reserved, not yet specified
(§3). The prototype never implemented it.

### D2 — Directives refine, never gatekeep
**Retained and strengthened.** Plain CommonMark always renders, and directives only add control. The
rationale holds: LLM output is the primary input and cannot reliably emit custom syntax. Added in
0.2: anything the engine cannot represent produces a diagnostic, and only problems with no safe
fallback block output (the severity policy, markdown-spec.md §11.1). Graceful degradation never means
silent loss.

### D3 — Real Office files as templates; no theming DSL
**Retained, deepened by D12.** Brand assets already exist as `.docx` / `.pptx`, and introspecting
them meets users where they are. A portable YAML/JSON template package was considered and rejected:
it asks users to rebuild their brand in a new format. Its main benefit, one brand across every
format, is delivered instead by deriving design tokens from the Office template. Font resolution is
amended by D14.

### D4 — Native renderers, no LibreOffice in the pipeline
**Retained; PDF approach amended by D13.** No heavyweight, stateful office suite at render time.
LibreOffice (or real Office) may be used **in CI only**, to verify that outputs open and render, never
to produce them.

### D5 — Hybrid engine, local-first, open-core
**Retained.** The engine runs locally and powers a server, with the same code on every path. The
self-hosted server is OSS. The paid surface is Compyla's managed deployment plus governance features
(auth, metering, multi-tenancy, org template registry and locked templates). Whether the org template
registry ships before or with Phase F is open (see roadmap).

### D6 — Byte-reproducible output
**Retained; extended by D14.** Pinned zip entry order and timestamps, deterministic relationship IDs,
fixed PDF dates, and fonts from the lockfile. Enables render caching, trivial CI diffing and golden
tests.

### D7 — Opt-in renderers; plugin platform
**Amended by D10.** Renderers are compile-time features of the facade crate. Native hosts (npm,
PyPI, CLI) ship all four renderers in one binary, since size is not a concern there. The browser
build is split: **core-only** for lint, **full** for preview. The four plugin extension points (output
formats, directives/tokens, validation rules, template-binding adapters) stand. The *mechanism* is
reopened: portable plugins across native and WASM hosts most likely means WASM components. Decided in
Phase E.

### D8 — Core → facade → hosts (strict layering)
**Retained in new form.** The `compyla` facade crate is the old SDK's role: it owns the
`parse → validate → compose → serialize` sequence, renderer dispatch, strict escalation and the
uniform diagnostics shape. Hosts depend only on the facade, never on a renderer crate or core
internals.

### D9 — Docs site: hand-rolled static generator dogfooding the engine
**Retained for now.** `packages/docs-site` renders `docs/*.md` through the prototype's `parse()`.
Revisited in Phase D: either it dogfoods the Node host or it moves to an off-the-shelf generator. It
must not become a reason to keep the TypeScript prototype alive.

### D10 — Rust core with native hosts *(new, 2026-10-31)*
**Chosen.** Requirements: users reach the engine through every channel. Native packages on **both**
npm and PyPI, from one core. The full engine must run **in-process in browsers and editors**. Optimize
for one core on every platform, accepting slower development.

| | TypeScript | Python | Rust |
|---|---|---|---|
| Native npm **and** PyPI | ✗ (Python gets a wrapper) | ✗ (Node gets a wrapper) | ✓ napi-rs + PyO3 |
| Full engine in browser/editor | ✓ | ✗ | ✓ WASM |
| One core, single CLI binary | ✗ | ✗ | ✓ |
| Typst for PDF | via binding | via binding | native library |

Only Rust meets every requirement. Costs: slower iteration; a release matrix (macOS, Linux, Windows ×
x64, arm64, plus WASM); no Office libraries to lean on (D12 requires owning the XML anyway); the
TypeScript prototype is retired, though its spec, fixtures and diagnostics design carry over.
**Revisit if** the Phase A toolchain check fails: one crate must build cleanly for napi-rs, PyO3 and
`wasm32` in CI. *Passed (2026-10-31):* the core, with comrak inside, builds unchanged for all three and
returns identical results ([spikes/README.md](../spikes/README.md), check 3).

### D11 — The core does no I/O *(new)*
**Chosen.** All file, network and font access goes through host-supplied resolvers. Required for
WASM and the bindings. It also makes renders reproducible and keeps the core trivially sandboxable.
Cost: every host implements resolvers. Accepted; they are small.

### D12 — Template-first rendering *(new)*
**Chosen.** The output is the template package with content written in, not a fresh file styled to
resemble the template. This is the only way to keep masters, logos, backgrounds, headers, footers,
page setup and the full style sheet. Cost: we write and own the docx/pptx XML ourselves (roughly
1–1.5k lines per format for text, lists, tables, images and notes), and native charts later need
their own XML. Accepted. Owning the XML also makes D6 simpler.

### D13 — PDF via Typst *(new)*
**Chosen over** hand layout (the prototype's approach, which would mean building a typesetting
engine) and over Office-to-PDF conversion (rejected by D4). Typst is a real typesetting engine,
Apache-2.0 licensed, and a native Rust library. Cost: a large dependency, which only the full WASM
build carries (D7), plus generating Typst markup from the composed model. **Revisit if** the
determinism check fails. *Passed (2026-10-31):* byte-identical PDFs across separate processes, with the
sandbox holding ([spikes/README.md](../spikes/README.md), check 2).

### D14 — Fonts resolved ahead of render, pinned in a lockfile *(new)*
**Chosen.** Renders must work offline and give identical bytes, and the prototype's per-render
network fetch broke both. Cost: a `fonts sync` step and a cache directory per user or project.
*Amended 2026-11-01 (C62):* house templates keep Office's font names, with a bundled open-licence
stand-in (Carlito) for measurement and pdf, in every build so `lint` agrees across hosts; the
resolver before a template's embedded fonts; one font stack shared with Typst; measurement in
integer font units with the viewers' features; a JSON lockfile pinning SHA-256 digests. See "Fonts
(D14)".

### D15 — Explicit compose stage; diagnostics from the composed model *(new)*
**Chosen.** The prototype's validator and renderers each reasoned about layout separately (block
counts in core vs. placeholder filling in pptx), with a shared helper as the only guard against drift.
A compose stage that both validation and serialization consume makes "validate what you render"
exact, and is where measured overflow lives.

**Measured overflow** *(amended, markdown-spec.md C63)*. pptx text is measured at the validate stage,
so `lint` and `convert` agree: the facade hands compose the template's text model (each box, its
insets and autofit, and the text styles it inherits) and a measurer over the resolved faces, compose
fills lines and decides what fits, and the renderer writes those decisions. Lines follow
PowerPoint's own rules, each checked in PowerPoint for Mac rather than read from the standard: a line
is 1.2 × its size × its spacing whatever the font, trailing spaces hang, tab stops count from the
inset. Measurement stays a diagnostic. PowerPoint does the final layout, and a stand-in that is a
little wide (Carlito for Calibri Light) can break a line a word sooner.

PowerPoint applies a box's "shrink text on overflow" (`normAutofit`) only when the text is edited,
not when it opens a file, so a deck written without a scale opens with its text running out of the
box. Where a template asks for shrinking, the engine therefore picks PowerPoint's own scale steps and
writes the first that fits, and reports overflow only below the smallest (25%): the deck opens as the
template intends. The house template asks for none (decided 2026-11-01), so house decks keep one type
scale and report overflow, whose hint says to split the slide.

### D16 — Spec-first contracts *(new)*
**Chosen.** The dialect spec, IR schema, diagnostics registry and conformance fixtures are the
source of truth. Every implementation and host must pass the fixtures. This makes the language choice
(D10) recoverable rather than permanent, and it is how the move from TypeScript to Rust is verified.
Cost: spec changes need fixture changes. Accepted. That is the point.

### D17 — MCP is a first-class host *(new)*
**Chosen.** Agents are a primary channel, and the lint → fix → re-lint loop over registered
diagnostic codes with hints is how an agent corrects its own markdown. The MCP server exposes the four
operations as tools, and the dialect spec, diagnostics registry and template reports as resources. It
ships in the first release (Phase D), not later.

## The Pixel-Perfect Tension

Template fidelity (D3/D12), messy LLM markdown (D2) and a shared base dialect (D1) pull against each
other. Resolution: the **base dialect always renders acceptably**, not necessarily pixel-perfect.
**Pixel-perfect fidelity is reached by writing into a known template** (D12) plus layout inference and
measurement (D15). `--strict` turns every non-advisory warning (missing fonts, unbound layouts,
overflow, dropped content) into an error, rather than allowing silent degradation.

## Data Flow

1. A host receives markdown or IR JSON, plus an optional template and options.
2. The host calls the facade with its resolvers (D11).
3. The facade runs parse → validate → compose → serialize (D8).
4. Blocking errors return diagnostics without bytes (CLI: non-zero exit; HTTP: `422`; MCP: a tool
   result carrying the diagnostics).
5. On success the host writes or returns the bytes, together with the non-blocking diagnostics.

## Reference Prototype (TypeScript)

`packages/` contains the prototype: `core` (parser, tokens, validator), `docx`, `pdf`, `pptx`, `xlsx`
renderers, `fonts`, `ooxml`, `sdk`, `cli`, `server`, `web` and `docs-site`. It stays buildable and
tested until Phase D. Its roles until then:

- **Oracle:** `spec/tools/oracle.mjs` regenerates conformance expectations from it, except for
  fixtures marked as diverging from the prototype, where the spec deliberately corrects it.
- **Reference:** a readable record of decisions already made (placement rules, inference rules, the
  formula allowlist), all of which are now written into the spec.

It is not published to npm. The first public release ships from the Rust core.

## Non-Goals (for now)

- WYSIWYG editing of output files. A live *preview* editor is planned (Phase F).
- HTML/ePub output (cheap later thanks to the IR, but not in the first release).
- Two-way conversion (Office → markdown).
