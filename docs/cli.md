# CLI & SDK Reference

Compyla is a **local-first, hybrid** engine (see [architecture.md](architecture.md), D5). The primary
interface is the CLI and the `@compyla/sdk` TypeScript SDK — both run the full pipeline **locally and
offline**, and the CLI is itself a thin consumer of the SDK (D8: core packages → SDK → apps &
services). A self-hostable server (`@compyla/server`) and a tiny local lint UI (`@compyla/web`) — same
engine, both documented below — plus Compyla's managed hosting on top of the server are the other
surfaces.

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

## Server

`@compyla/server` is a thin HTTP wrapper over the same engine (D5) — **OSS, self-hostable**. It's raw
`node:http` plus `busboy` for `multipart/form-data`, no web framework — routing is 3 fixed routes.
Install and run:

```bash
npm i @compyla/server @compyla/docx   # plus whichever renderer packages you need
PORT=3000 npx compyla-server
# or embed it: import { createServer } from "@compyla/server"; createServer().listen(3000);
```

Sync execution only for now; async jobs/webhooks are added when workload demands. Anyone can run it on
their own infrastructure — it requires no Compyla account, and nothing it does is persisted server-side
(uploads are processed in memory per-request and discarded).

### `POST /convert`

`multipart/form-data` fields: `markdown` (required text), `format` (required text), `strict` (optional
text, `"true"`/`"1"`), `template` (optional file). On success, the response body **is the converted
file's raw bytes** — not a signed URL; a signed URL implies object storage, which is a managed-hosting
concern, not something the stateless self-hosted server has. `Content-Type` is set per format
(`application/vnd.openxmlformats-officedocument.wordprocessingml.document` for docx, `application/pdf`
for pdf, the presentationml/spreadsheetml equivalents for pptx/xlsx) and `Content-Disposition:
attachment; filename="output.<format>"` is set. Non-blocking warning diagnostics (if any) ride along in
an `X-Compyla-Diagnostics` response header (JSON array, only present when non-empty) rather than
breaking the binary body.

There is no asset-upload support yet: a document referencing an image always fails validation with
`image_unresolved` (see below) rather than the server guessing at a filesystem path that doesn't exist
on a stateless HTTP request.

### `POST /validate`

Same required `markdown`/`format` fields (and optional `strict`) as `/convert`, no `template` — mirrors
`compyla lint`, fully offline, no renderer ever loaded. Response body is JSON: `{ "ok": boolean,
"diagnostics": [...] }` (the diagnostics shape above).

### `POST /templates`

`multipart/form-data` field: `template` (required file; kind is inferred from the filename's extension,
`.docx` or `.pptx`). Response body is the same introspection report `compyla template inspect` prints —
stateless, nothing is persisted or referenced by an ID. A persisted, shareable template registry is
Phase 3's "Template manager UI," not this endpoint.

### Status codes

| Condition | Status | Body |
|---|---|---|
| Pre-render validation blocked | `422` | `{ "ok": false, "diagnostics": [...] }` |
| Unknown format/template kind (client's mistake) | `400` | `{ "error": "..." }` |
| Known format/kind, but this deployment doesn't have the opt-in renderer package installed | `501` | `{ "error": "..." }` |
| Malformed request (bad multipart, missing required field) | `400` | `{ "error": "..." }` |
| No matching route | `404` | `{ "error": "..." }` |

`400` vs `501` is deliberate: an unknown format is a bad input value from the client, while a
known-but-not-installed renderer is *this deployment's* capability gap (HTTP's own definition of 501 —
different self-hosted servers legitimately install different opt-in format subsets, D7).

## Web

`@compyla/web` is a tiny local UI for interactive lint feedback — paste or drop a markdown file, see
line-level diagnostics live as you type. Diagnostics only: it shows the same output as `compyla lint`,
never a rendered document preview (that's the Phase 3 live preview editor). Runs entirely client-side —
once the page loads, nothing leaves the browser. Install and run:

```bash
npm i @compyla/web
PORT=4300 npx compyla-web
# or embed it: import { createServer } from "@compyla/web"; createServer().listen(4300);
```

It's a single vanilla-JS bundle (no framework) calling `@compyla/sdk`'s `lint()` directly in the
browser, served by a fixed-route `node:http` static server — the same no-framework approach as
`@compyla/server`. Only `lint()` is reachable from the UI, so the renderer packages' opt-in dynamic
imports never execute and are excluded from the browser bundle.

## Managed hosting

Compyla's own **managed hosting** of `@compyla/server` — auth, usage metering/billing, and
governance/enterprise features on top — is the paid surface of the open-core model (Phase 2+). The
engine, renderers, CLI, and self-hosted server documented above remain OSS and fully usable without it.
