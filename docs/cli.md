# CLI & SDK Reference

Compyla is a **local-first, hybrid** engine (see [architecture.md](architecture.md), D5). The primary
interface is the CLI and the `@compyla/sdk` TypeScript SDK — both run the full pipeline **locally and
offline**, and the CLI is itself a thin consumer of the SDK (D8: core packages → SDK → apps &
services). A self-hostable server and Compyla's managed hosting mirror the same engine and are
described at the end as later, planned surfaces.

## Installation

Install the SDK (or the CLI, which depends on it) plus only the format renderers you need (opt-in, D7):

```bash
npm i @compyla/sdk @compyla/docx     # embed the SDK directly in your own code; docx only
npm i @compyla/cli @compyla/docx     # or use the CLI — it depends on @compyla/sdk internally
npm i @compyla/pptx @compyla/xlsx @compyla/pdf   # add formats as needed
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

## SDK

The same engine as a TypeScript API — this is what the CLI itself calls (D8):

```ts
import { convert, lint } from "@compyla/sdk";

const { diagnostics, ok } = lint(markdown, { format: "docx" });
if (!ok) throw new Error("validation failed");

const { buffer } = await convert(markdown, {
  format: "docx",
  template: brandDocxBuffer,   // optional Buffer; omit for the house template
});
```

`convert`'s output is **byte-reproducible** (D6): identical markdown + template → identical bytes,
which makes it safe to cache and to assert with golden-file tests. `lint` runs parse + validate only
(no renderer required, same as `compyla lint`); `inspectTemplate(buffer, kind)` (`kind`: `"docx"` |
`"pptx"`) is the programmatic form of `compyla template inspect`.

`@compyla/core` and the individual renderer packages (`parse`, `validate`, `renderDocx`, …) remain
directly importable for advanced or custom orchestration — the SDK wraps them for the common case, it
doesn't hide them.

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

Every diagnostic — CLI output, SDK return value, or server response — is uniform:

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

## Server & managed hosting (planned)

`@compyla/server` is a thin HTTP wrapper over the same engine (D5) — **OSS, self-hostable, planned for
Phase 1** (not part of the Phase-0 MVP). It exposes the same pipeline over HTTP — `POST /convert`,
`POST /validate`, `/templates` — returning the diagnostics shape above, with sync execution first and
async jobs/webhooks added when workload demands. Anyone can run it on their own infrastructure; it
requires no Compyla account.

Compyla's own **managed hosting** of that same server — auth, usage metering/billing, and
governance/enterprise features on top — is the paid surface of the open-core model (Phase 2+). The
engine, renderers, CLI, and self-hosted server documented above remain OSS and fully usable without
it. Detailed HTTP reference will be written when `@compyla/server` is built.
