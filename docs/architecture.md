# Architecture

## Context and Goals

`Compyla` converts markdown into four output formats (docx, pdf, pptx, xlsx) through **native
renderers** sharing one engine. The primary consumers are developers integrating a local library or
CLI, with LLM-generated markdown as the dominant input source. The engine is **hybrid** — the same
TypeScript code runs locally (CLI, CI, offline) and powers an optional hosted API — and shipped
**open-core**.

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

Everything is a **pnpm/turbo monorepo**. `@compyla/core` (parser + tokens + validator) has **zero
dependency on any renderer**. Renderers are installed opt-in (see D7). The CLI runs the engine
locally; the hosted API is a thin server over the same engine.

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

### D5 — Hybrid engine, local-first, hosted optional (open-core)
The engine runs locally (CLI/CI/offline) and also powers an optional hosted API — same code, both
paths. Distribution is **open-core**: engine, renderers, and CLI are OSS; the hosted API and
governance/enterprise features are the paid surface; self-host follows for free. Sync execution is
sufficient for the initial use case; async jobs/webhooks are added when workload demands.

### D6 — Byte-reproducible output
Identical markdown + template → identical file bytes. Enables render caching, trivial CI diffing, and
golden-file tests. Requires deliberate discipline in every renderer (pin timestamps, sort zip entries,
deterministic IDs). Accepted — fits the validation-first, CI-friendly positioning.

### D7 — Core + opt-in renderer packages; plugin platform
Users install `@compyla/core` plus only the format packages they need. The CLI errors helpfully when a
requested format's package is absent. Renderer packages are the primary **plugin boundary**; the
platform (phased) supports four extension points: new output formats, custom directives/styling
tokens, custom validation rules, and template-binding adapters. Local plugins ship first; running
untrusted plugins inside the hosted service (a sandboxing problem) is deferred.

## The pixel-perfect tension

Pixel-perfect output (D3/D4), messy LLM markdown (D2), and a shared base dialect (D1) pull against
each other. Resolution: the **base dialect always renders acceptably** — not necessarily
pixel-perfect. **Pixel-perfect fidelity is reached by binding to a known template** plus normalization
and layout inference; the **fidelity validation level** (`--strict`) surfaces anything that would
drift (missing fonts, unresolved layouts, overflow) as diagnostics rather than silent degradation.

## Data Flow and Integration Points

1. Caller invokes the engine (CLI, library import, or hosted API) with markdown (+ optional template).
2. Engine runs Parse → Tokens → Validate → Render.
3. Validation errors surface the diagnostic list (CLI: non-zero exit; API: `422`).
4. Success writes the artifact (local path) or stores it and returns a signed URL (hosted).

Module boundaries mirror the pipeline: `packages/core` (`parser/`, `ir/`, `profiles/`, validator),
`packages/{docx,pptx,xlsx,pdf}` (renderers), `packages/cli`, and an optional `packages/server`. The
`core` package has no dependency on any renderer.

Two more packages exist purely to avoid duplicating logic that turned out to be needed by more
than one renderer, discovered empirically once a second OOXML renderer (pptx) was built alongside
docx: `packages/fonts` (the Google Fonts resolution chain — §"Template + Font Engine") and
`packages/ooxml` (the deterministic zip repackaging step from D6, and the DrawingML theme parser,
since docx and pptx templates share the identical `<a:theme>` schema). Both are internal
implementation-sharing packages, not renderers in their own right.

## Non-Goals (for now)

- WYSIWYG editing or live preview (teams-phase feature).
- HTML/ePub output (cheap later thanks to the token model, but not MVP).
- Two-way conversion (Office → markdown).
