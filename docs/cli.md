# CLI & Library Reference

Compyla is a **local-first, hybrid** engine (see [architecture.md](architecture.md), D5). The primary
interface is the CLI and the TypeScript library — both run the full pipeline **locally and offline**.
A hosted API mirrors the same engine and is described at the end as a later, optional surface.

## Installation

Install `@compyla/core` plus only the format renderers you need (core + opt-in, D7):

```bash
npm i @compyla/core @compyla/docx @compyla/cli    # docx only
npm i @compyla/pptx @compyla/xlsx @compyla/pdf     # add formats as needed
```

Requesting a format whose package isn't installed produces an actionable error:

```
error: format "pptx" requires @compyla/pptx. Install it with:
       npm i @compyla/pptx
```

## CLI

```bash
compyla convert report.md --format docx --template brand.docx --out report.docx
compyla lint    report.md --format xlsx            # validate only; offline; exit 1 on error
compyla lint    report.md --format pptx --strict   # escalate fidelity warnings to errors
compyla template inspect brand.pptx                # report detected styles/layouts/fonts
```

Common flags:

| Flag | Purpose |
|------|---------|
| `--format` | `docx` \| `pdf` \| `pptx` \| `xlsx`. Overrides frontmatter `format`. |
| `--template` | Template file path or ID. Overrides frontmatter `template`. |
| `--out` | Output path (default: input name with the format extension). |
| `--assets <dir>` | Directory resolving images referenced by relative path. |
| `--strict` | Fidelity level: unresolved fonts/layouts/overflow become errors. |

`compyla lint` is the CI entry point: it runs the parser + validator only, needs no renderer, and
exits non-zero when any `error` diagnostic is present. Diagnostics are line-referenced with a
suggested fix.

## Library

The same engine as a TypeScript API:

```ts
import { parse, validate } from "@compyla/core";
import { renderDocx } from "@compyla/docx";

const doc = parse(markdown);                         // AST + base/override tokens (with source pos)
const diagnostics = validate(doc, { format: "docx" });
if (diagnostics.some(d => d.severity === "error")) throw new Error("validation failed");

const bytes = await renderDocx(doc, { template: "brand.docx" });   // byte-reproducible Buffer
```

`renderDocx` output is **byte-reproducible** (D6): identical `doc` + template → identical bytes, which
makes it safe to cache and to assert with golden-file tests.

## Template introspection

`compyla template inspect <file>` reports what Compyla found in a `.docx`/`.pptx` — the binding surface
renderers use, plus any issues that will affect fidelity:

```json
{
  "kind": "pptx",
  "layouts": [
    { "name": "Title Slide", "placeholders": ["title", "subtitle"] },
    { "name": "Two Content", "placeholders": ["title", "body", "body"] }
  ],
  "theme": { "fonts": ["Inter", "Inter Light"], "colors": ["#1A1A2E", "#0F79D7"] },
  "fonts": { "Inter": "google-fonts", "Corporate Sans": "unresolved" },
  "issues": [
    { "code": "no_quote_layout", "message": "No layout for quotes; blockquotes use 'Title and Content'." },
    { "code": "font_unresolved", "message": "'Corporate Sans' not embedded and not on Google Fonts." }
  ]
}
```

## Diagnostics shape

Every diagnostic — CLI output, library return value, or hosted API response — is uniform:

```json
{
  "severity": "error",
  "code": "table_not_rectangular",
  "line": 42,
  "message": "Table row 3 has 4 cells; header defines 5 columns.",
  "hint": "xlsx sheets require rectangular tables. Add the missing cell or remove the extra column."
}
```

---

## Hosted API (open-core, planned)

The hosted service is a thin server over the same engine (D5) and is **not part of the Phase-0 MVP**.
When it ships it will expose the same pipeline over HTTP — `POST /convert`, `POST /validate`,
`/templates` — returning the diagnostics shape above, with sync execution first and async
jobs/webhooks added when workload demands. It is the paid surface of the open-core model, alongside
governance/enterprise features; the engine, renderers, and CLI documented above remain OSS and fully
usable without it. Detailed HTTP reference will be written against the hosted service when it is built.
