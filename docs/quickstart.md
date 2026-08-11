# Quickstart

From nothing to a real `.docx` in under 5 minutes.

## 1. Install

```bash
npm i -g @compyla/cli @compyla/docx
```

(Or add them to a project with `npm i @compyla/cli @compyla/docx` and run via `npx compyla`.)

## 2. Write some markdown

Plain CommonMark + GFM — no Compyla-specific syntax required (see D2 in
[architecture.md](architecture.md)):

```markdown
# Q3 Business Review

This quarter delivered **strong growth** across every region, with particular strength in
*enterprise* accounts.

## Revenue

| Region | Revenue | Growth |
|--------|---------|--------|
| APAC   | 120000  | 14%    |
| EMEA   | 98000   | 9%     |
| AMER   | 210000  | 18%    |
```

Save that as `report.md`.

## 3. Lint it (optional, but this is the CI-friendly step)

```bash
compyla lint report.md --format docx
```

```
report.md: OK (docx)
```

`lint` runs the parser + validator only — no renderer needed, fully offline, exits non-zero on error.
Try it against a deliberately broken document to see what a real diagnostic looks like:

```bash
compyla lint examples/broken.md --format docx
```

```
examples/broken.md:3: warning: Heading level jumps from H1 to H3. [heading_skip]
  hint: Insert an H2 before this heading, or promote it to H2.
examples/broken.md:7: error: Image "does-not-exist.png" could not be resolved. [image_unresolved]
  hint: Check the path is correct, or pass an assets directory so relative paths resolve.
examples/broken.md:9: warning: Unknown directive "made-up-directive" for the docx profile; it renders as if absent. [unknown_directive]
  hint: Known docx directives: toc, pagebreak, image.
```

Every diagnostic is line-referenced with a suggested fix — that's true in the CLI, the SDK, and (once
built) the server, since they all share one diagnostics shape (see [cli.md](cli.md)).

## 4. Convert it

```bash
compyla convert report.md --format docx --out report.docx
```

```
Wrote report.docx
```

No template supplied, so this rendered against Compyla's house docx template. Run it again and diff
the two files — they're byte-identical (D6): same input always produces the same bytes, which is what
makes render output safe to cache and to assert in tests.

## 5. Bring your own brand template (optional)

```bash
compyla template inspect brand.docx      # see what Compyla detected: styles, theme, fonts
compyla convert report.md --format docx --template brand.docx --out report.docx
```

Fonts are never named in markdown — the template supplies them, and Compyla resolves each family
through Google Fonts automatically (see "Template + Font Engine" in [architecture.md](architecture.md)).

## Next steps

- [markdown-spec.md](markdown-spec.md) — the full dialect: frontmatter, directives, per-format
  overrides, and the pptx/xlsx profiles (slide segmentation, sheet/table directives, formula rules).
- [cli.md](cli.md) — the complete CLI + SDK reference, including embedding `@compyla/sdk` directly in
  your own TypeScript instead of shelling out to the CLI.
- [when-to-use-pandoc.md](when-to-use-pandoc.md) — an honest comparison, so you don't reach for
  Compyla where Pandoc is genuinely the better fit.
- [architecture.md](architecture.md) — how the pieces fit together and why, if you're evaluating this
  for production use or planning to contribute a renderer/plugin.
