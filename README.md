# Compyla

Convert Markdown into professional documents — **docx, pdf, pptx, and xlsx** — with one **native**
engine. Local-first, open-core.

Markdown is the native output of LLMs and the native writing format of developers. `Compyla` turns that
markdown into real, on-brand deliverables: reports, slide decks, spreadsheets, and PDFs — each rendered
**natively** (no LibreOffice, no conversion), byte-for-byte reproducibly.

```
Markdown → AST → base + per-format tokens → Validator → native renderer → .docx / .pdf / .pptx / .xlsx
```

## Why Compyla

- **One dialect, four outputs.** Write standard CommonMark + GFM. The same base document renders to any
  format; optional per-format override tokens add control — never required.
- **LLM-native.** Plain markdown with zero directives always produces a reasonable output. Fonts are
  never named in markdown; the brand template decides. Model-generated content works out of the box.
- **Native renderers.** Each format is generated natively by its own package — docx, pptx, xlsx, and a
  native PDF layout. No headless office suite in the pipeline.
- **Template-driven, pixel-perfect.** Upload a real `.docx`/`.pptx` as a brand template; fonts resolve
  through Google Fonts automatically. No custom theming language to learn.
- **xlsx done right.** Markdown tables become real sheets with formulas, column types, and formatting —
  with formulas **parsed and allowlisted** so generated sheets can't carry injection payloads.
- **Byte-reproducible.** Identical input yields identical bytes — cacheable, diffable, testable.
- **Validation first.** A linter validates documents against the target format profile *before*
  rendering, with actionable line-level errors. Runs offline. CI-friendly.

## Quick Start

Local-first — install the CLI plus the formats you need, then convert:

```bash
npm i @compyla/cli @compyla/docx

# Convert a markdown file to docx
compyla convert report.md --format docx --template brand.docx --out report.docx

# Lint before converting (offline; exits non-zero on error)
compyla lint report.md --format pptx
```

## Minimal Examples

**docx / pdf** — plain markdown just works:

```markdown
# Quarterly Report
## Revenue
Revenue grew 14% quarter over quarter...
```

**pptx** — `---` splits slides, directives add control:

```markdown
---
format: pptx
template: acme-brand-v2
---

# Q3 Review

---

## Revenue vs. Target

::: layout Two Content
Left column content.
<!-- col -->
Right column content.
:::

::: notes
Mention the APAC numbers first.
:::
```

**xlsx** — H1 = sheet, tables = data ranges, allowlisted formulas pass through:

```markdown
# Sales

::: columns B:currency, C:percent

| Region | Revenue | Growth      |
|--------|---------|-------------|
| APAC   | 120000  | 0.14        |
| EMEA   | 98000   | 0.09        |
| Total  | =SUM(B2:B3) | =AVERAGE(C2:C3) |
```

## Documentation

Browse as a searchable, navigable site: **https://ichintansoni.github.io/compyla/** — or read the
source files directly:

| Doc | Contents |
|-----|----------|
| [docs/quickstart.md](docs/quickstart.md) | Zero to a real `.docx` in under 5 minutes |
| [docs/architecture.md](docs/architecture.md) | Hybrid engine, native renderers, the token model, key decisions and trade-offs |
| [docs/markdown-spec.md](docs/markdown-spec.md) | The Compyla dialect: base syntax, frontmatter, per-format overrides, fonts, and the four format profiles |
| [docs/cli.md](docs/cli.md) | CLI + SDK reference (local, offline), template introspection, the self-hostable server, the local web lint UI, and Compyla's managed hosting |
| [docs/when-to-use-pandoc.md](docs/when-to-use-pandoc.md) | An honest comparison — including when Pandoc is the better choice |
| [docs/roadmap.md](docs/roadmap.md) | Phased plan (developers → teams), starting with the docx renderer |

## Project Status

**Phase 0** (engine core + docx) and the docx/pdf/pptx/xlsx renderers, SDK, CLI, self-hostable server,
and web lint UI of **Phase 1** are done. See the [roadmap](docs/roadmap.md) for what's left.

## Design Principles

1. **Graceful degradation.** Plain CommonMark must render acceptably in every format. Overrides refine;
   they never gatekeep.
2. **One parser, many native renderers.** All format logic lives in renderer packages consuming shared
   tokens. New formats are new packages, not new parsers.
3. **Meet users where their files are.** Templates are real Office files; fonts resolve via Google
   Fonts — no bespoke theme language, no bundling licensed fonts.
4. **Errors are a feature.** Validation messages must be specific, line-referenced, and suggest the fix.
5. **Byte-reproducible output.** Same input, same bytes — always.
6. **Open-core, local-first.** The engine, renderers, CLI, a self-hostable server, and the web lint UI
   are OSS and run offline or on your own infrastructure; Compyla's managed hosting and governance
   features are the paid surface.

## Packaging

**Core packages → SDK → apps & services** — install only what you need:

```bash
npm i @compyla/core        # parser + tokens + validator (no renderer dependency)
npm i @compyla/docx        # + docx renderer
npm i @compyla/pptx @compyla/xlsx @compyla/pdf   # add as needed
npm i @compyla/sdk         # convert/lint/inspectTemplate — the programmatic interface (pulls in core)
npm i @compyla/cli         # the compyla command (pulls in the SDK)
```

## Contributing

Not yet open for external contributions. Internal setup:

```bash
git clone <repo>
cd compyla
# pnpm/turbo monorepo — see docs/architecture.md for the package layout
```
