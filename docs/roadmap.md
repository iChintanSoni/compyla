# Roadmap

Strategy: **developers first, teams second.** Ship an open-core, local-first engine (CLI + SDK) that
developers adopt and validate; build the hosted/teams product on what their usage reveals.

## Phase 0 — Engine core + docx (done)

Goal: the pipeline works end-to-end for **docx** — the first native renderer — with real validation
and byte-reproducible output.

- [x] Monorepo scaffold (pnpm/turbo): `packages/{core,docx,cli}`, TS strict
- [x] Parser (`@compyla/core`): CommonMark + GFM + frontmatter + directive fences, with source positions
- [x] Token model: base tokens + per-format override tokens; builder
- [x] Profile framework: profile = mapping rules + validation rules
- [x] docx native renderer (`@compyla/docx`) against a supplied/default template
- [x] Font resolution: template families → Google Fonts (download + embed + subset) + fallback chain
- [x] Validator with line-level diagnostics + `--strict` fidelity level
- [x] Byte-reproducible output (pinned timestamps, sorted zip entries, deterministic IDs)
- [x] `compyla lint` / `compyla convert` / `compyla template inspect` as local CLI

Exit criterion (met): a nontrivial plain-markdown report converts to a clean, on-template docx **twice
with identical bytes**, and a deliberately broken document produces useful line-level diagnostics.

## Phase 1 — Developer MVP (remaining formats, current)

- [ ] pdf native renderer (`@compyla/pdf`) — native page layout, no conversion (D4)
- [x] pptx renderer (`@compyla/pptx`): slide segmentation (`groupSlides`, shared with the core
      validator), layout directives + layout inference v1, `<!-- col -->` columns, speaker notes,
      byte-reproducible output. Fonts are name-only (no embedding — pptxgenjs has no font-embedding
      API, unlike docx). Layout names bind to an uploaded template's own layouts (see alias mapping
      below); a template without a name/alias match for a given layout falls back to Compyla's
      house layout set.
- [x] xlsx renderer (`@compyla/xlsx`): H1 sections -> sheets (`groupSheets`, shared with the core
      validator), H2 range titles, note paragraphs, `columns`/`anchor`/`table` directives, per-column
      type inference, **parsed + allowlisted** formulas (function allowlist, not a denylist —
      `@compyla/core`'s `formula.ts`, offline so `compyla lint` needs no renderer), byte-reproducible
      output. No xlsx template introspection yet (house header style only; `--template` warns) —
      unlike pptx's alias mapping, this wasn't called out as in-scope for Phase 1.
- [x] Template theme introspection (docx, pptx) — shared DrawingML theme parser (`@compyla/ooxml`)
- [x] Template alias mapping: pptx layout names bind to an uploaded template's *own* real layouts
      by explicit alias, then case-insensitive name match, else the house layout set
      (`@compyla/ooxml` slide-layout parser + `@compyla/pptx`'s `bindLayouts`)
- [x] `@compyla/sdk` — **core packages → SDK → apps & services** layering (D8 in
      [architecture.md](architecture.md)): `convert()` / `lint()` / `inspectTemplate()`, opt-in
      renderer loading with helpful not-installed errors, `--strict` escalation, uniform diagnostics.
      `packages/cli` refactored onto it as the first consumer — it now holds only argv parsing, file
      I/O, and terminal formatting, no engine orchestration of its own. This is the prerequisite the
      two items below build on, so a web UI or server doesn't have to re-implement any of it.
- [ ] `@compyla/server` — thin, **self-hostable** HTTP server (OSS): `POST /convert`, `POST /validate`,
      `/templates`, same diagnostics shape as the CLI. Deployable to any Node host, no Compyla account
      required — distinct from Compyla's own managed hosting of it (Phase 2, see below).
- [ ] `@compyla/web` — tiny local web UI for interactive lint feedback: paste/drop markdown, see
      line-level diagnostics live, offline. Diagnostics only, no rendered-document preview (that's the
      Phase 3 live preview editor).
- [ ] Docs site: spec, quickstart (< 5 min to first file), and an honest "when to just use Pandoc" page
- [ ] Published OSS packages (core + opt-in renderers) with semver

Exit criterion: an external developer integrates the SDK/CLI without talking to us.

## Phase 2 — Developer growth

- [ ] Plugin platform GA: the four extension points (formats, directives/tokens, validation rules,
      template-binding adapters), local plugins first
- [ ] Chart directive for pptx/xlsx (`::: chart bar` over a table)
- [ ] Layout inference v2 (content-density-aware overflow, auto-splitting long slides)
- [ ] Compyla's **managed hosting** of `@compyla/server` (the paid open-core surface): auth, usage
      metering/billing, sync first with async/webhooks as needed. The server package itself is OSS
      and self-hostable from Phase 1 — this is Compyla *running* it for you.
- [ ] GitHub Action for docs-as-deliverables pipelines
- [ ] HTML renderer (cheap thanks to the token model) for previews

## Phase 3 — Teams

Built from observed usage (which templates, which formats, which failures):

- [ ] Template manager UI: upload, introspection report, alias mapping, sharing within an org
- [ ] Live preview editor (markdown left, rendered doc right)
- [ ] Org workspaces, roles, seat pricing alongside usage pricing
- [ ] Brand governance: locked templates, required frontmatter fields
- [ ] Untrusted plugin execution inside the hosted service (sandboxing)

## Explicit non-goals until further notice

- Office → markdown reverse conversion
- WYSIWYG editing of output files
