# When to just use Pandoc

[Pandoc](https://pandoc.org) is a mature, enormously capable, general-purpose document converter. For
a lot of markdown-to-office needs, it — or Pandoc piped through headless LibreOffice — is genuinely
the right tool, and reaching for Compyla instead would be the wrong call. This page is here so you
don't find that out the hard way. (Design principle 4 in [architecture.md](architecture.md): errors
are a feature, including this one.)

## Use Pandoc instead if:

- **You need more than markdown as input, or more than four formats as output.** Pandoc converts
  between dozens of formats — LaTeX, reStructuredText, HTML, EPUB, ODT, MediaWiki, man pages, and more
  in every direction. Compyla takes markdown in and produces docx/pdf/pptx/xlsx out. Nothing else, in
  either direction.
- **You need Office → markdown (or Office → anything) conversion.** Compyla is one-way by design (see
  Non-Goals in [architecture.md](architecture.md)); Pandoc does this well today.
- **You need citations and a bibliography.** Pandoc's citeproc integration (BibTeX, CSL styles, etc.)
  is mature and well-documented. Compyla has no citation system at all.
- **You need math notation.** Pandoc handles LaTeX/MathML math natively. Compyla's markdown dialect
  doesn't support math today.
- **You need LaTeX-quality PDF typesetting** — dense academic papers, footnotes, complex figure
  placement, professional-grade typography. Pandoc → LaTeX → PDF is a real typesetting engine with
  decades of refinement. Compyla's native PDF renderer is a from-scratch layout engine and won't match
  LaTeX's sophistication for a long time, if ever.
- **You don't care about binding to a specific brand template**, and "reasonable default styling" is
  good enough. Pandoc's `--reference-doc` gets you a fair distance for less setup.
- **You want one mature, battle-tested binary** with a huge community, extensive docs, and years of
  edge cases already sanded off, rather than a pre-1.0 project (Compyla is currently pre-MVP — see
  [roadmap.md](roadmap.md)).
- **LibreOffice is already a solved problem in your pipeline** — you're running it as a managed
  service, latency and reliability are acceptable, and you don't need byte-for-byte reproducible
  output.

## Compyla is worth it instead if:

- **Your input is LLM-generated markdown, in production, at volume.** Compyla is built around the
  assumption that input will sometimes be malformed and needs to render *something reasonable* rather
  than fail unhelpfully or drift silently (D2 in architecture.md) — and to be validated with
  line-level, actionable diagnostics *before* that happens, not discovered by opening the output file.
- **You need pixel-perfect binding to a real corporate `.docx`/`.pptx` template**, with automatic font
  resolution through Google Fonts and a template-introspection report telling you exactly what will and
  won't come through faithfully. Pandoc's reference-doc mechanism is more manual and doesn't validate
  fit against your content ahead of time.
- **You need byte-reproducible output** — for render caching, CI golden-file testing, or diffing two
  runs to prove nothing changed. Pandoc's LibreOffice-dependent paths aren't deterministic across
  environments or LibreOffice versions; Compyla's native renderers pin timestamps, sort zip entries,
  and use deterministic IDs specifically so this works.
- **You want to avoid a LibreOffice dependency entirely.** No headless office suite in the pipeline —
  relevant if you're deploying into containers/serverless where a stateful, heavyweight `soffice`
  process is awkward to manage.
- **You need real xlsx spreadsheets** — formulas, per-column types, formatting — generated from
  markdown tables, with formulas parsed and checked against a function allowlist rather than passed
  through unvalidated. Pandoc has no xlsx writer at all.
- **You need pptx with layout control**: directive-driven slide layouts that bind to a template's own
  real layouts by name, with overflow validation (does this content actually fit the chosen layout?)
  before you ever open PowerPoint. Pandoc's markdown→pptx path is far more limited here.
- **You want a typed SDK with structured diagnostics**, not a CLI you shell out to and scrape stderr
  from — see [cli.md](cli.md).

## The honest summary

If you're not sure, and your documents don't need brand-template fidelity or byte-reproducibility,
start with Pandoc — it's more mature and does more. Compyla earns its place specifically at the
intersection of *LLM-generated markdown*, *must match a brand template*, and *needs to be validated
and cacheable in an automated pipeline*. Outside that intersection, Pandoc is very likely the better
default today.
