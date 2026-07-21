# Compyla Markdown Specification

One dialect, four profiles. This document defines the **base syntax** accepted everywhere, the
**per-format override** mechanism, then the per-format profile that governs how structure maps to each
output.

**Guiding rule:** a document written in plain CommonMark + GFM with no Compyla extensions MUST produce
reasonable output in every profile. Extensions and overrides add control; they never gatekeep.

Spec version: `0.1-draft`

---

## 1. Base Syntax

- **CommonMark** core.
- **GFM extensions:** tables, task lists, strikethrough, autolinks.
- **YAML frontmatter** delimited by `---` at the very top of the file.
- **Directive blocks:**

  ```markdown
  ::: name arg1, arg2
  ...children (markdown)...
  :::
  ```

- **Inline markers** as HTML comments, used where a block form is awkward:

  ```markdown
  <!-- col -->        column separator inside a layout
  <!-- pagebreak -->  explicit page break (docx/pdf), ignored elsewhere
  ```

Unknown directives and markers emit a validator **warning** and render as if absent.

## 2. Frontmatter

```yaml
---
format: pptx            # optional; the engine/CLI may also set it (explicit call wins)
template: acme-brand-v2 # template ID or path
title: Q3 Review        # document / deck / workbook title
author: J. Doe
lang: en
---
```

Unknown keys are preserved in token `meta` and available to renderers (e.g. custom template
variables). **Fonts are never specified here or anywhere in the markdown** — see §3.

## 3. Base tokens, per-format overrides, and fonts

Compyla parses markdown into **base tokens** (format-agnostic intent) plus optional **per-format
override tokens** that refine a single target. This is how one file renders to all four formats while
still allowing format-specific styling.

Overrides are scoped by format, either as a directive block or a format-scoped frontmatter key:

```markdown
::: pptx
layout: Two Content
accent: theme.color2
:::
```

A renderer consumes only the overrides for its own format; others ignore them. Plain markdown with no
overrides always renders in every profile (graceful degradation).

**Fonts.** Markdown never names a font. The **template** declares font families; the engine resolves
each family through a fallback chain: (1) font embedded in the template → use it; (2) family name
matches **Google Fonts** → download + embed + subset; (3) user-supplied font file; (4) substitute +
**fidelity warning**. This keeps input LLM-friendly and avoids bundling licensed corporate fonts.

## 4. Thematic Break (`---`)

The `---` line (outside frontmatter) is a semantic **Break**. Its meaning is assigned by profile:

| Profile | Meaning of `---` |
|---------|------------------|
| docx    | Page break |
| pdf     | Page break |
| pptx    | **Slide break** |
| xlsx    | Ignored (warning) |

---

## 5. Profile: docx *(Phase-0 / first renderer)*

The natural-fit profile. Mapping:

| Markdown | Word output |
|----------|-------------|
| `#`…`######` | Styles `Heading 1`–`Heading 6` |
| Paragraph | `Normal` style |
| Bold / italic / code / links | Character formatting / `Hyperlink` style |
| `>` blockquote | `Quote` style |
| GFM table | Table with template's default table style; header row styled |
| Fenced code block | `Code` style (monospace block); language ignored |
| Image | Inline picture, sized to fit page width unless sized (see 5.1) |
| `---` | Page break |
| Task list | Checkbox glyphs (☐ / ☒) — not interactive controls |

### 5.1 Directives (docx)

```markdown
::: toc              → insert table of contents field
::: pagebreak        → explicit page break (same as ---)
::: image width=50%  → wraps following image, sizing hint
```

### 5.2 Validation (docx)

- Heading levels should not skip (H1 → H3): **warning**.
- Images must resolve: **error**.
- Font family unresolved after the fallback chain: **warning** (**error** under `--strict`).

## 6. Profile: pdf

Identical authoring surface to docx (§5), but **rendered by the native PDF renderer**
(`@compyla/pdf`) — not converted from docx. Additional frontmatter keys:

```yaml
page_size: A4 | Letter   # default A4
margins: normal | narrow
```

---

## 7. Profile: pptx *(Phase-1)*

### 7.1 Slide segmentation

- `---` starts a new slide.
- Content before the first `---` is the **title slide**: first H1 → deck title, following paragraph →
  subtitle.

### 7.2 Per-slide structure

- First heading on the slide (any level) → slide **title** placeholder.
- Remaining content → body placeholder(s).

### 7.3 Layout selection

Priority order:

1. Explicit directive: `::: layout <name>` as the first block of the slide.
2. **Inference** from content shape:

| Content shape | Inferred layout |
|---------------|-----------------|
| Title only | Section header |
| Title + one content block | Title and Content |
| Content split by `<!-- col -->` | Two Content |
| Table as sole body content | Title and Content (table fills body) |
| Image as sole body content | Picture with Caption |
| Blockquote as sole body content | Quote layout if template has one, else Title and Content |

Layout names bind to template layouts by (a) explicit alias map provided at template upload, then
(b) case-insensitive name match against the template's layout names.

### 7.4 Directives (pptx)

```markdown
::: layout Two Content      → pick layout
::: notes                   → children become speaker notes (any markdown)
::: image fit=contain       → image sizing within its placeholder
```

Column separation inside multi-content layouts uses `<!-- col -->`.

### 7.5 Validation (pptx)

- Layout directive names must resolve against the template: **error**, message lists available layouts.
- More content than the layout has placeholders: **warning**, overflow flows into the last placeholder
  (**error** under `--strict`).
- H4+ headings inside a slide body: **warning** (likely a docx-oriented document).

---

## 8. Profile: xlsx

The novel profile. A workbook is authored as sheets of tables.

### 8.1 Structure mapping

| Markdown | xlsx output |
|----------|-------------|
| `# Heading` (H1) | **Sheet** named after the heading |
| GFM table | Data range placed at the current anchor (default `A1`, subsequent tables stacked with one blank row) |
| Table header row | Styled header row (bold, fill from template preset) |
| `## Heading` (H2) | Range title: a styled label cell above the next table |
| Paragraph | A note cell above the next table (single merged cell) |
| Everything else (quotes, code, images) | **Ignored with warning** |

Content before the first H1 belongs to a default sheet named `Sheet1` (or `meta.title`).

### 8.2 Formulas — parsed and allowlisted

A cell whose content starts with `=` is a **formula cell**. Formulas are **parsed and checked against a
function allowlist** before being written:

```markdown
| Region | Revenue     |
|--------|-------------|
| APAC   | 120000      |
| EMEA   | 98000       |
| Total  | =SUM(B2:B3) |
```

- **Allowed:** pure spreadsheet functions (SUM, AVERAGE, IF, VLOOKUP, INDEX/MATCH, arithmetic, …).
- **Blocked (error):** network/command/DDE vectors — `WEBSERVICE`, `RTD`, `HYPERLINK` to external
  targets, `DDE`, and any cell that would be interpreted as a command (leading `=`,`+`,`-`,`@` used to
  smuggle payloads). This closes the formula/CSV-injection hole for whoever opens the sheet.
- Cell references are workbook coordinates after placement. The validator checks references fall inside
  the sheet's populated range (out-of-range → **warning**; cross-sheet `SheetName!A1` allowed).

To render a literal string beginning with `=`, escape it: `\=`.

### 8.3 Directives (xlsx)

```markdown
::: columns B:currency, C:percent, D:date(iso)
    → number formats + sensible widths for the NEXT table

::: anchor C5
    → place the next table at C5 instead of stacking

::: table name=sales_q3, style=striped, autofilter
    → register the next table as a named Excel table
```

### 8.4 Type inference

Without a `columns` directive, cell types are inferred per column: all-numeric → number; ISO dates →
date; `true/false` → boolean; else text. A column with mixed inferred types becomes text (**warning**).

### 8.5 Validation (xlsx)

- Non-rectangular table (ragged rows): **error** with row/line reference.
- Duplicate or >31-char sheet names: **error** with suggested truncation.
- Formula parse failure or disallowed function: **error** pointing at the cell.

---

## 9. Versioning

Documents may pin a spec version in frontmatter (`spec: 0.1`). The engine defaults to the latest and
warns when behavior differs across pinned versions. Breaking changes to profile mappings bump the
minor version pre-1.0, major after.

## 10. Open Questions

- pptx/xlsx: chart generation from tables (`::: chart bar` over a table) — high value, deferred past
  MVP.
- xlsx: multi-table relationships / named ranges referenced across sheets.
- Shared: footnotes and how each profile should treat them (docx footnotes are natural; pptx/xlsx have
  no equivalent).
- Override syntax: `::: pptx` blocks vs. attribute syntax (`{.two-col}`) — fenced form chosen for
  LLM-friendliness, revisit on user feedback.
