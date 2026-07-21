# Roadmap

Strategy: **developers first, teams second.** Ship an open-core, local-first engine (CLI + library)
that developers adopt and validate; build the hosted/teams product on what their usage reveals.

## Phase 0 — Engine core + docx (current)

Goal: the pipeline works end-to-end for **docx** — the first native renderer — with real validation
and byte-reproducible output.

- [ ] Monorepo scaffold (pnpm/turbo): `packages/{core,docx,cli}`, TS strict
- [ ] Parser (`@compyla/core`): CommonMark + GFM + frontmatter + directive fences, with source positions
- [ ] Token model: base tokens + per-format override tokens; builder
- [ ] Profile framework: profile = mapping rules + validation rules
- [ ] docx native renderer (`@compyla/docx`) against a supplied/default template
- [ ] Font resolution: template families → Google Fonts (download + embed + subset) + fallback chain
- [ ] Validator with line-level diagnostics + `--strict` fidelity level
- [ ] Byte-reproducible output (pinned timestamps, sorted zip entries, deterministic IDs)
- [ ] `compyla lint` / `compyla convert` / `compyla template inspect` as local CLI

Exit criterion: a nontrivial plain-markdown report converts to a clean, on-template docx **twice with
identical bytes**, and a deliberately broken document produces useful line-level diagnostics.

## Phase 1 — Developer MVP (remaining formats)

- [ ] pdf native renderer (`@compyla/pdf`) — native page layout, no conversion (D4)
- [ ] pptx renderer: slide segmentation, layout directives, layout inference v1, speaker notes
- [ ] xlsx renderer: sheets, tables, **parsed + allowlisted** formulas, `columns` directive, type inference
- [ ] Template introspection + alias mapping (docx, pptx)
- [ ] Docs site: spec, quickstart (< 5 min to first file), and an honest "when to just use Pandoc" page
- [ ] Published OSS packages (core + opt-in renderers) with semver

Exit criterion: an external developer integrates the library/CLI without talking to us.

## Phase 2 — Developer growth

- [ ] Plugin platform GA: the four extension points (formats, directives/tokens, validation rules,
      template-binding adapters), local plugins first
- [ ] Chart directive for pptx/xlsx (`::: chart bar` over a table)
- [ ] Layout inference v2 (content-density-aware overflow, auto-splitting long slides)
- [ ] Hosted API (open-core): thin server over the engine; sync first, async/webhooks as needed
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
