# Architecture

## Context and Goals

`Compyla` converts markdown into four output formats (docx, pdf, pptx, xlsx) through **native
renderers** sharing one engine. The primary consumers are developers integrating a local library, CLI,
web UI, or server, with LLM-generated markdown as the dominant input source. The engine is **hybrid**
— the same TypeScript code runs locally (CLI, CI, offline, web) and powers a server, self-hosted or
Compyla-managed — and shipped **open-core**.

Design goals, in priority order:

1. A base dialect of plain CommonMark must produce reasonable output in any format with zero
   configuration; per-format styling only refines it.
2. Adding an output format is adding a renderer package — it must not require touching the parser.
3. Output is **pixel-perfect to a supplied template**, produced by binding to real Office files.
4. Failures happen at validation time with actionable, line-referenced errors — never at render time.
5. Output is **byte-reproducible**: identical input yields identical bytes.

## High-Level Design

```
markdown ─▶ @compyla/core ─────────────────────────────────────────────────┐
            ┌────────────┐   ┌──────────────────┐   ┌────────────────┐      │
            │   Parser   │──▶│  Token/IR Builder │──▶│   Validator    │      │
            │ CommonMark │   │  base tokens +    │   │ per-format     │      │
            │ +GFM +front│   │  per-format       │   │ profile rules  │      │
            │ +directives│   │  override tokens  │   │ (line-level)   │      │
            └────────────┘   └──────────────────┘   └───────┬────────┘      │
                                                            │ valid tokens  │
     ┌──────────────┬──────────────┬──────────────┬─────────┴────┐          │
     ▼              ▼              ▼              ▼               │          │
┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐  opt-in native       │
│@compyla/ │  │@compyla/ │  │@compyla/ │  │@compyla/ │  renderer packages    │
│  docx    │  │  pptx    │  │  xlsx    │  │  pdf     │  (also the plugin      │
└────┬─────┘  └────┬─────┘  └────┬─────┘  └────┬─────┘   boundary)            │
     └─────────────┴──────┬───────┴────────────┘                             │
                          ▼                                                  │
                 ┌─────────────────────┐                                     │
                 │ Template + Font Engine│ ◀───────────────────────────────────┘
                 │ (Office introspection │
                 │  + Google Fonts)      │
                 └─────────────────────┘
                          ▼
                  byte-reproducible artifact
```

Everything is a **pnpm/turbo monorepo**, and it layers strictly: **core packages → SDK → apps &
services** (see D8). `@compyla/core` (parser + tokens + validator) has **zero dependency on any
renderer**; renderers are installed opt-in (see D7). Neither layer knows anything above it — apps
depend on the SDK, never the reverse, and nothing outside `@compyla/core` reaches back into the parser.

### Interface Surfaces

Every app or service is a thin consumer of `@compyla/sdk` — none hold engine orchestration of their
own, and none import `@compyla/core` or a renderer package directly:

| Surface | Package | Notes |
|---------|---------|-------|
| CLI | `@compyla/cli` | `compyla convert` / `compyla lint` / `compyla template inspect`. Local, offline. Owns argv parsing, file I/O, and terminal-formatted diagnostics only. |
| Web | `@compyla/web` | Tiny local UI for interactive lint feedback — paste/drop markdown, see line-level diagnostics live. Diagnostics only, no rendered-document preview (that's the Phase 3 live preview editor — see Non-Goals). |
| Server | `@compyla/server` | Thin, **self-hostable** HTTP wrapper (`POST /convert`, `POST /validate`, `/templates`) — OSS, deploy to any Node host, no Compyla account required. Distinct from Compyla's own managed hosting of it (D5). |

The SDK is also the **programmatic interface** in its own right — embed `@compyla/sdk` directly in
any Node app or CI script (`convert()` / `lint()` / `inspectTemplate()`) without going through a CLI
subprocess or an HTTP hop. All surfaces share one diagnostics shape (see [cli.md](cli.md)).

### Stage 1 — Parser (`@compyla/core`)

- CommonMark + GFM (tables, strikethrough, task lists, autolinks).
- YAML frontmatter extraction.
- Directive extension: fenced `::: name args` blocks and `<!-- name -->` inline markers.
- Output: a standard markdown AST annotated with source line/column positions (required for validator
  error messages).
- The parser knows nothing about output formats. Candidate stack: `remark` + `remark-gfm` +
  `remark-directive` + a frontmatter step.

### Stage 2 — Token/IR Builder

Transforms the AST into the **Semantic IR** — a format-agnostic document model of **base tokens**,
with optional **per-format override tokens** layered on top. This is how "one file → four outputs"
coexists with per-format styling: the base always renders everywhere; overrides refine a specific
target.

```
Document
├─ meta            (frontmatter: title, template, format, author…)
└─ body: Node[]
   ├─ Section      (heading level, title, children)
   ├─ Paragraph    (rich-text inline runs: bold, italic, code, links)
   ├─ TableBlock   (rows, header row, column hints, cell content incl. formula cells)
   ├─ ListBlock    (ordered/unordered/task, nesting)
   ├─ MediaBlock   (image ref, alt, sizing hints)
   ├─ CodeBlock    (language, content)
   ├─ QuoteBlock
   ├─ Break        (from ---; meaning assigned per profile: page vs slide)
   └─ DirectiveBlock (name, args, children — resolved per profile)

+ overrides: per-format styling tokens scoped to a target (e.g. a `::: pptx` block or
  format-scoped frontmatter keys) that a single renderer consumes and others ignore.
```

Key property: base tokens preserve *intent*, not layout. "Break" is not "page break" or "slide break"
until a profile interprets it. Override tokens are the escape hatch for format-specific control.

### Stage 3 — Validator

Validates tokens against the target **format profile** (see [markdown-spec.md](markdown-spec.md)).
Examples:

- `xlsx`: every table must be rectangular; **formulas are parsed and checked against a function
  allowlist** (see D-formula); sheet names (H1s) unique and ≤ 31 chars.
- `pptx`: content between breaks must fit a known layout, or a layout directive must resolve.
- All profiles: image references must resolve; directive names must exist in the profile.

Every diagnostic carries severity (`error` | `warning`), source position, and a suggested fix. A
**fidelity level** (`--strict`) escalates "won't be pixel-perfect" situations — missing font,
unresolved layout, overflow — from silent drift to warnings/errors. The validator is exposed
standalone as `compyla lint` and runs fully **offline**.

### Stage 4 — Renderers (native, per-format packages)

One renderer package per format, each consuming the same tokens and emitting the format **natively —
no LibreOffice, no format-to-format conversion**:

| Package | Approach |
|---------|----------|
| `@compyla/docx` | Map tokens to Word named styles in the template (`Heading 1–6`, `Normal`, `Quote`, table styles). Candidate lib: `docx`. |
| `@compyla/pdf`  | **Native page layout** (candidate: `pdf-lib`/`pdfkit` or a layout engine). No docx→PDF conversion. |
| `@compyla/pptx` | Break-delimited slide groups → template layouts. Layout by directive, else inferred from content shape. |
| `@compyla/xlsx` | H1 sections → sheets; TableBlocks → ranges; **allowlisted formulas** passed through; column-type directives → number formats and widths. Candidate lib: `exceljs`. |

All renderers produce **byte-reproducible** output: pinned timestamps, sorted zip entries,
deterministic relationship IDs.

### Template + Font Engine

- Users supply a real `.docx`/`.pptx` as a template. Compyla introspects it: named styles, slide
  layouts and placeholder shapes, theme colors/fonts.
- Renderers bind tokens to template styles/placeholders by convention (IR `Section level=1` → Word
  style `Heading 1`; pptx layout `two-col` → template layout `Two Content`).
- **Fonts are never named in markdown.** The template declares font families; the engine resolves each
  family through a fallback chain: (1) font embedded in the template → use it; (2) name matches
  **Google Fonts** → download, embed, and subset (OFL/Apache licensing permits redistribution);
  (3) user-supplied font file; (4) substitute + **fidelity warning**.
- A default house template ships for every format so templates are optional.

## Key Decisions and Trade-offs

### D1 — Shared base tokens + per-format override tokens
**Chosen.** One parser and N renderers keeps the dialect coherent and makes new formats cheap. Base
tokens carry format-agnostic intent; override tokens add per-format control without forking the
dialect. Cost: the token model must express both layers, and profile-specific meaning (what `---`
means) is deferred to interpretation. Accepted.

### D2 — Directives refine, never gatekeep
Plain CommonMark always renders. Directives and override tokens only add control. Rationale: LLM
output is the primary input and cannot reliably emit custom syntax. Consequence: every profile needs
sensible inference defaults, which is real engineering effort. Accepted — this is core differentiation.

### D3 — Real Office files as templates + Google Fonts resolution
No bespoke theming DSL. Brand assets already exist as `.docx`/`.pptx`; introspecting them meets users
where they are. Fonts resolve from the template via Google Fonts, sidestepping the licensing trap of
bundling corporate fonts. Cost: template introspection is fiddly and font resolution fails for
proprietary faces not on Google Fonts. Mitigation: `compyla template inspect` and an explicit fidelity
warning.

### D4 — Native renderers, including native PDF (no LibreOffice)
**Chosen: native generation for all four formats, each its own TS package.** PDF is laid out natively
rather than converted from docx. Pros: no heavyweight/stateful LibreOffice or Office dependency (pure
JS workers scale cleanly), full pixel control, one distribution story. Cost: native PDF layout is the
most work of the four (effectively a layout engine), and each renderer owns its fidelity. Accepted.

### D5 — Hybrid engine, local-first; self-hosted server is OSS, managed hosting is paid (open-core)
The engine runs locally (CLI/CI/offline/web) and also powers a server — same code, every path.
Self-hosting is a first-class free path, not a fallback: `@compyla/server` (the thin HTTP wrapper
over the engine, see "Interface Surfaces") is OSS like everything else — anyone can run it on their
own infrastructure with no Compyla account. The **paid** surface is Compyla's own *managed* deployment
of that same server, plus governance/enterprise features (auth, usage metering/billing, multi-tenancy,
org template sharing — see Phase 3) that don't make sense to self-host. Sync execution is sufficient
for the initial use case; async jobs/webhooks are added when workload demands.

### D6 — Byte-reproducible output
Identical markdown + template → identical file bytes. Enables render caching, trivial CI diffing, and
golden-file tests. Requires deliberate discipline in every renderer (pin timestamps, sort zip entries,
deterministic IDs). Accepted — fits the validation-first, CI-friendly positioning.

### D7 — Core + opt-in renderer packages; plugin platform
Users install `@compyla/core` plus only the format packages they need. Requesting a format whose
renderer package is absent errors helpfully (see D8 — this check lives in the SDK, not each app).
Renderer packages are the primary **plugin boundary**; the platform (phased) supports four extension
points: new output formats, custom directives/styling tokens, custom validation rules, and
template-binding adapters. Local plugins ship first; running untrusted plugins inside the hosted
service (a sandboxing problem) is deferred.

### D8 — Core packages → SDK → apps & services (strict layering)
**Chosen.** `@compyla/sdk` sits between the engine and every consumer. It owns everything an app would
otherwise have to duplicate: the `parse → validate → render` sequence, opt-in renderer loading with a
helpful "package not installed" error, `--strict` fidelity escalation, template-kind introspection
dispatch, and the uniform diagnostics shape. Apps (`@compyla/cli`, `@compyla/web`, and
`@compyla/server`) depend only on the SDK — never on `@compyla/core` or a renderer package directly —
and hold nothing but their own I/O and presentation: argv parsing and terminal output for the CLI,
HTTP request/response handling for the server, DOM rendering for the web UI. Rationale: without this
layer, adding a second app (the original motivation — see "Interface Surfaces") means either
duplicating orchestration logic per app or reaching past a thin app into the renderer packages
directly, both of which defeat the point of one engine, many surfaces. Cost: one more package to
version and keep in sync with core/renderer releases. Accepted — the CLI was refactored onto this
layer as the first consumer, proving it by removing all engine logic that previously lived in
`packages/cli` (renderer dynamic-loading, convert/lint orchestration, template-kind dispatch).

## The pixel-perfect tension

Pixel-perfect output (D3/D4), messy LLM markdown (D2), and a shared base dialect (D1) pull against
each other. Resolution: the **base dialect always renders acceptably** — not necessarily
pixel-perfect. **Pixel-perfect fidelity is reached by binding to a known template** plus normalization
and layout inference; the **fidelity validation level** (`--strict`) surfaces anything that would
drift (missing fonts, unresolved layouts, overflow) as diagnostics rather than silent degradation.

## Data Flow and Integration Points

1. Caller invokes an app or the SDK directly (CLI, embedded library use, web UI, self-hosted server,
   or Compyla's managed hosting) with markdown (+ optional template).
2. `@compyla/sdk` runs the engine: Parse → Tokens → Validate → Render (D8).
3. Validation errors surface the diagnostic list (CLI: non-zero exit; server: `422`).
4. Success writes the artifact (local path) or stores it and returns a signed URL (server).

Module boundaries mirror **core packages → SDK → apps & services** (D8): `packages/core` (`parser/`,
`ir/`, `profiles/`, validator) and `packages/{docx,pptx,xlsx,pdf}` (renderers) are the core layer —
`core` has no dependency on any renderer, and renderers depend only on `core` (+ `fonts`/`ooxml`
below), never on each other. `packages/sdk` is the sole layer between them and every app; it depends
on `core` and, opt-in, on the renderer packages (D7). `packages/cli` and the other two
consumption-surface packages — `packages/server` (self-hostable HTTP API, OSS) and `packages/web`
(lint UI, OSS) — depend only on `packages/sdk`, never reaching past it.

Two more packages exist purely to avoid duplicating logic that turned out to be needed by more
than one renderer, discovered empirically once a second OOXML renderer (pptx) was built alongside
docx: `packages/fonts` (the Google Fonts resolution chain — §"Template + Font Engine") and
`packages/ooxml` (the deterministic zip repackaging step from D6, and the DrawingML theme parser,
since docx and pptx templates share the identical `<a:theme>` schema). Both are internal
implementation-sharing packages, not renderers in their own right.

## Non-Goals (for now)

- WYSIWYG editing or live *document* preview (teams-phase feature) — distinct from the Phase 1 web
  lint UI, which shows diagnostics only, never a rendered preview.
- HTML/ePub output (cheap later thanks to the token model, but not MVP).
- Two-way conversion (Office → markdown).
