# Compyla Markdown Specification

One dialect, four profiles. This document defines the **input** Compyla accepts, how it maps to the
**IR** (the format-agnostic document model), the **per-format profiles** that govern how that
structure maps to each output, and the **operations** every host exposes.

**Guiding rule:** a document written in plain CommonMark + GFM with no Compyla extensions MUST produce
reasonable output in every profile. Extensions add control; they never gatekeep. When the engine
can't do exactly what the input says, it applies a **defined fallback** and reports a diagnostic.
Nothing is dropped or changed silently, and only problems with no safe fallback block output (§11.1).

Spec version: `0.2` · IR version: `0.1` · Status: draft (pre-1.0)

---

## 0. Status and Conformance

### 0.1 Conformance

The keywords MUST, SHOULD and MAY are used as in RFC 2119.

An implementation conforms to spec `0.2` when it passes every fixture in `spec/conformance/`
(format described in `spec/conformance/README.md`) and follows every MUST in this document. The
fixtures are normative. Where this document and a fixture disagree, the fixture wins until the
conflict is resolved in the next revision.

The companion artifacts are part of the spec:

| Artifact | Defines |
|----------|---------|
| `spec/ir.schema.json` | The IR (JSON Schema 2020-12) |
| `spec/diagnostic.schema.json` | The diagnostic object every operation returns |
| `spec/diagnostics.json` | Every diagnostic code and operation error: severity, stage, strict promotion, position, meaning |
| `spec/template-report.schema.json` | The output of `inspect_template` |
| `spec/conformance/` | Executable expectations for parse, lint, compose and render |

**Positions.** Lines are 1-based. Columns are 1-based and count Unicode scalar values from the start
of the physical line; a tab counts as one. Positions are absolute in the input, including frontmatter
lines, and are counted after a leading byte-order mark is removed (§1.1). Hosts that talk to editors
convert columns to the editor's unit (UTF-16 for LSP).

**Diagnostics.** A diagnostic's `code`, `severity`, `line` and `column` are normative. Each code's
registry entry says where it points (§11.3). Message and hint text are not normative, but SHOULD name
the offending item and suggest a fix.

### 0.2 Changes from 0.1

0.1 described the dialect loosely, and the TypeScript prototype filled the gaps. Building the
conformance suite against the prototype, and two independent reviews of the spec, found behavior that
dropped content silently, gave wrong or unsafe results, or was simply undefined. 0.2 corrects or
defines each case. Fixtures numbered `1xx` encode a correction; `0xx` fixtures agree with the
prototype.

| # | 0.2 behavior | Before (0.1 text or prototype) | Fixtures |
|---|--------------|--------------------------------|----------|
| C1 | Formula cells are detected and read from the cell's **raw source text** (§4.6, §9.3) | Read from rendered text: `=B2*C2*D2` lost its `*`s to emphasis and became `=B2C2D2`; the documented `\=` escape never worked | parse/101, lint/102 |
| C2 | A directive opener or closer always starts a new block, and the pre-pass tracks code fences per CommonMark (§1.3) | An opener right after a paragraph line was lost, a longer code fence containing a shorter one broke, and an internal placeholder leaked into the text | parse/108, parse/116, parse/128 |
| C3 | `:::name` (no space) opens a directive, like `::: name` (§1.3) | Literal text | parse/104 |
| C4 | An unclosed opener becomes a leaf directive, with `directive_unclosed` (§1.3) | Literal text | parse/105 |
| C5 | Reference-style links resolve to links; a soft line break is a space; a hard break or `<br>` is `"\n"` (§4.4, §4.5) | Reference links dropped with their text; soft breaks kept as `"\n"`; `<br>` dropped (`x<br>y` → `xy`) | parse/102, parse/103 |
| C6 | Content the IR can't represent produces `unsupported_content` (§4.8) | Dropped silently (inline images, footnotes, raw HTML) | parse/106 |
| C7 | Invalid frontmatter produces `frontmatter_invalid`; an unsupported `spec` produces `spec_version_unsupported` (§2) | The parser threw; `spec` was ignored | parse/107, lint/109 |
| C8 | Annotation directives apply to the blocks they wrap, or, when empty, to what follows (§1.5) | xlsx dropped wrapped tables; a leaf `layout` emptied its slide | lint/110, compose/102 |
| C9 | pptx slides split at every `---` in document order (§8.1) | A `---` after a subsection heading didn't split the slide | compose/101 |
| C10 | pptx fit is checked against the resolved layout, explicit or inferred; demoted headings join text blocks (§8.6) | Explicit layouts only; a demoted heading counted as its own block | lint/108, lint/127 |
| C11 | `lint` given a template validates layout names against the template's layouts (§8.5) | Built-in names only | lint/139 |
| C12 | Unqualified formula references are **table-relative** and translated to placed addresses (§9.3) | Absolute: a range title, note or stacked table silently shifted every reference (including in `examples/budget.md`) | compose/103, lint/104 |
| C13 | xlsx paragraphs and range titles that never attach to a table produce `xlsx_ignored_content` (§9.4) | Dropped silently | lint/103 |
| C14 | xlsx sheet names, table names, table headers and table styles are checked and repaired (§9.6, §9.7) | Unchecked: invalid names produced broken workbooks; unknown styles were replaced silently | lint/105, lint/122 |
| C15 | Diagnostics about a table row or cell report the row's own source line (§9.7) | The table's first line, and cell addresses assumed placement at A1 | lint/101 |
| C16 | `::: image width=N%` requires an integer 1–100 (§1.6) | `NaN` passed through to the renderer | lint/107 |
| C17 | Per-format override blocks are **reserved**, not yet specified (§3) | Described in 0.1, never implemented | — |
| C18 | pptx checks image references like docx and pdf (§8.7) | Unchecked; a missing image failed at render time | lint/111 |
| C19 | In docx and pdf, `---` is a **horizontal rule**; page breaks are explicit (`::: pagebreak`) (§5) | A page break. LLM-written reports use `---` as a section divider, which produced a page per section | render/101, render/102 |
| C20 | Dialect directives a profile doesn't use are handled per the directive registry, without `unknown_directive`; `notes` is omitted outside pptx (§1.5) | `unknown_directive` warnings, and speaker notes rendered as body text in docx/pdf | lint/112 |
| C21 | Directive names are case-insensitive; fences may use 3 or more colons; `::: name args :::` is a one-line directive; markers are complete on their opener line; a fence-like line that isn't one produces `directive_malformed` (§1.3) | Case-sensitive; exactly `:::`; one-line form unsupported; `::: toc` swallowed content up to the next closer; malformed fences silently became text | parse/109, parse/110, parse/121, parse/122 |
| C22 | Inline runs are canonical: adjacent runs with identical marks merge, empty runs are dropped (§4.4) | Implementation-defined run boundaries | parse/102 |
| C23 | Link reference definitions apply document-wide, including inside directive bodies (§4.5) | References across a directive boundary didn't resolve | parse/111, parse/129 |
| C24 | Link destinations are checked against an allowlist; `#fragment` links resolve to headings (§4.5) | Any destination, including `javascript:` and UNC paths, written into output; fragments unresolved | parse/112, parse/127, lint/113 |
| C25 | Characters not allowed in XML are replaced, with `invalid_character` (§1.1) | Passed through, producing corrupt Office files | parse/113 |
| C26 | pptx placeholder slots define what fits; empty slides are omitted; Two Content distributes blocks without `col` markers (§8.1, §8.6) | Block counts only; empty trailing slides rendered; Two Content without markers put everything left | compose/104, lint/114 |
| C27 | xlsx value integrity: >15 significant digits, leading zeros and impossible dates stay text; Excel limits, overlapping ranges and empty workbooks are diagnosed (§9.2, §9.5, §9.7) | Silent precision loss, dropped zeros, rolled-over dates, overwritten cells, invalid empty workbooks | compose/105, lint/115, lint/116, lint/119 |
| C28 | Frontmatter is a JSON-compatible YAML subset; known keys are type-checked; key diagnostics point at the key's line (§2) | Any YAML (aliases expanded); types unchecked; diagnostics at line 1 | parse/114, parse/126, lint/117 |
| C29 | A paragraph line directly above `---` (which CommonMark makes a heading) produces `ambiguous_break` (§4.9) | Silently a heading instead of a break | parse/115 |
| C30 | Image bytes are checked by content: PNG, JPEG and GIF are supported; `data:` URIs are decoded by the core (§4.7) | Unsupported formats failed at render time | lint/118 |
| C31 | Diagnostics may carry an IR `path`; IR input may omit positions; operation outputs are deterministically ordered (§10, §11) | Undefined | — |
| C32 | **Severity policy:** an error means no safe or faithful output is possible; everything else is a warning with a defined fallback, and `strict` promotes every non-advisory warning (§11.1) | Many plain-markdown problems blocked output (`# Revenue: 2024`, a ragged table, a missing image); `strict` didn't cover dropped content | lint/120–lint/123, lint/101 |
| C33 | Formulas must parse under a grammar; a name followed by `(` is always a function call; sheet qualifiers are checked; newer functions are stored with `_xlfn.` (§9.3) | `=LOG10(100)` was read as a cell reference, bypassing the allowlist; `=SUM(` and `=` were accepted; `IFS` etc. showed `#NAME?` in Excel | lint/124 |
| C34 | An image-only paragraph yields one `media` per image, a linked image keeps its link, and empty paragraphs are omitted; empty link text shows the destination (§4.3, §4.4, §4.7) | Two images in a paragraph, or a linked image, were dropped, leaving an empty paragraph | parse/118, parse/124 |
| C35 | Ordered lists record their start number (§4.3) | Lost; numbering restarted at 1 after a code block between steps | parse/117 |
| C36 | Inline tags that aren't HTML elements (`Promise<void>`) stay literal text (§4.8) | Dropped as HTML, deleting the word | parse/119 |
| C37 | `table_malformed` and `document_in_code_fence` flag common LLM table and wrapping mistakes (§4.9) | Silently a paragraph of pipes, or one big code block | parse/120, parse/123 |
| C38 | pptx inference only picks layouts the content fits; the title-slide rule applies only when the content fits it; layouts bind by name, then by OOXML layout type; the house set has the standard Office layouts (§8.3, §8.5) | Inferred layouts its own check rejected; titles slides held tables; localized templates never matched | compose/107, compose/108, compose/110, lint/121 |
| C39 | xlsx: tables under level-3+ headings are kept; `columns` keys may be header names; percent values may end in `%`; typed values that don't parse warn; number formats and recalculation are defined (§9) | Tables under `###` dropped; formats undefined; silent text fallback | compose/106, compose/109, lint/125 |
| C40 | `layout_not_in_template` and `font_unresolved` are validate-stage when their inputs are available, so `lint --strict` agrees with `convert --strict` (§11) | Render-stage only | lint/139 (`layout_not_in_template`) |
| C41 | Hosts' resolvers are confined (§10.3); templates are stripped of macros, ActiveX, OLE, external references and orphaned parts; template XML may not contain a DTD; Typst runs sandboxed (§10.4, §12.2) | Unspecified | render suite (Phase C) |
| C42 | `heading_skip` compares headings within one heading tree (§6.3) | A heading in a directive body was compared with the outer section | lint/126 |
| C43 | An indented code block's position is the first character of its content (§4.1) | Column 1 of its line | parse/125 |
| C44 | Document properties come only from `meta`: a title or author it lacks is not written (§5) | The rendering library's defaults (`PptxGenJS Presentation`, `Unknown`) | render/103 |
| C45 | pptx: a misplaced `layout` is ignored and entered like any annotation; an unresolved or missing layout name is selected as if absent, title-slide rule included; Quote holds only a quote or text (§8.1, §8.3, §8.6) | A misplaced `layout` was an opaque block; Quote accepted any one block | compose/111, compose/002, lint/128 |
| C46 | xlsx: a column whose cells match different tests, or that has no literal values, is `general`: each cell keeps its own type. Percent formats default to just enough decimals. Currency codes are any three letters. A sheet qualifies both cells of a range. Booleans in formulas are written in uppercase. An overridden empty annotation is misplaced. Column parameters follow §1.6's unknown/invalid split. A misplaced directive's arguments aren't checked (§1.5, §9.3–§9.6) | Numbers mixed with percents became percents (`42` → 4200%); mixed and formula-only columns were text; `9.5%` showed as 10%; an overridden annotation vanished silently | compose/103, compose/105, compose/112, lint/129, lint/130 |
| C47 | Limits (§12.1) hold for every input: IR input is counted as markdown is, and cells per row and slides are enforced. A limit stops processing, so no validate-stage diagnostic follows one. IR positions are at most 2³²−1, and IR text rules cover every string. Nested annotations of the same name: the innermost applies (§9.4, §10.7, §12.1) | Unenforced for IR input, rows and slides: hostile input crashed, hung or placed cells past XFD | lint/132, lint/133, lint/134, lint/135 |
| C48 | xlsx rendering: the header row is bold on a light fill unless an Excel table styles it; an Excel table with no data rows gets an empty one; hyperlinked text is underlined in the link color and line breaks wrap (§9.2, §9.6) | Header style unspecified; an Excel table with no data rows was invalid | render/104, render/105, render/106, render/107 |
| C49 | xlsx output, from review: the diagnostics cap keeps errors first, so a blocked `convert` always reports why (§11.2); text that looks like a SpreadsheetML escape reads back as written; Excel table headers keep links and rich text; emphasis on a number becomes the cell's font; an erroring formula's text is its `raw` (§9.3, §9.5, §9.6) | A blocked `convert` could report no error; `_x0041_` read back as "A"; header links dropped silently | render/108 |
| C50 | `inspect_template` is specified end to end: an invalid template's report has `kind: null` and its reason; layouts report their OOXML type; Word's page defaults where a template is silent (§10.4, §10.5) | Undefined for invalid packages | inspect/101–110 |
| C51 | pptx rendering is specified: placeholders by slot, text (bullets, numbering, code, quotes), tables, images (`contain` by default, `cover` cropped), shared slots, links to slides, notes, and the package (§8.8) | Undefined beyond the projection | render/109–render/114 |
| C52 | From review of the pptx renderer: images over §12.1's limits are `limit_exceeded`, and unreadable ones `image_unsupported`; nested images and tables are `pptx_ignored_content`; numbering outside DrawingML's range is text; notes are plain; `lang` reaches the output (§5, §8.8, §12.1) | Unchecked, silent, or schema-invalid output | render/115, lint/136 |
| C53 | Nothing a slide holds is dropped: content on a layout without slots goes into its first content placeholder or over the master's body area, and a title on Blank over the master's title area; EXIF orientation is honored, with the tag set to 1 in the image written; show and view settings naming removed slides are dropped (§8.6, §8.8) | Dropped content, sideways photos, a show of missing slides | render/116 |
| C54 | xlsx leftovers: a cell's diagnostics carry its path; table names `TRUE`, `FALSE` and anything shaped like a cell reference are invalid; more than 65,530 hyperlinks on a sheet or 200 custom number formats in a workbook are `xlsx_limit_exceeded`; conformance compares `path` where the expectation has one; an empty Excel table stacked onto the last row is reported at the table (§9.6, §9.7, §11.3) | Row-level paths only; names and files Excel may reject or repair; a position no node has | lint/137, lint/138 |
| C55 | From opening the output in PowerPoint and Excel: columns are as wide as what they display, so numbers and dates don't show as `####`; a link in an Excel table keeps the table's text color (contrast); text that reads as a number isn't flagged as a number stored as text; slide shapes are written in reading order; a title on Blank is a title placeholder (§8.8, §9.2, §9.5) | Unreadable numbers; accessibility checks failing on contrast, reading order and slide titles; error flags on intended text | render/104, render/116–118 |
| C56 | Supplied pptx templates: the `template` option and `meta.template` through the resolver, with `template_unresolved`, `template_invalid`, `no_theme` and `xlsx_template_unsupported`; layout binding by alias, name and type, with `layout_not_in_template` once per layout; a template's own layouts; `layoutAliases` checked (`option_invalid`); slots past a bound layout's placeholders share its last (§8.5, §8.6, §8.8, §10.2, §10.6) | Templates not used; aliases unchecked; content drawn over a placeholder | lint/139–143, render/119 |
| C57 | A workbook takes a docx or pptx template's theme tokens: its colors and heading and body fonts as the workbook's theme, cells and titles in its fonts, the header fill and links in its colors, and a striped Excel table's dark header where white wouldn't read on its accent (§9.2, §9.6, §10.4) | Templates not used for xlsx | render/120 |
| C58 | Active and external content is removed from a supplied template's kept parts: external relationships other than hyperlinks (with `TargetMode` and targets that leave the package read fail-closed), hyperlinks §4.5 doesn't allow and actions that run something, OLE objects, ActiveX controls and their legacy drawings, Office add-ins, with what refers to them (a blip loses only the attribute; anything else goes with its alternates, extension, graphic frame, animations or transition), what only they used, and the parts only they reached; one `template_content_removed` per object, action or other removed relationship (§4.5, §10.4) | Which elements go with a removed relationship, what counts as one removal, and which hyperlinks a template may keep were unspecified | render/121 |
| C59 | A link destination is checked as a browser reads it: leading and trailing spaces and controls, and tabs and line breaks inside it, don't hide its scheme, and other control characters, and a leading non-ASCII space, are refused; before any query or fragment, it must pass percent-decoded too (§4.5) | ` file:///…` and `\tms-msdt:…` in IR input or a template's hyperlinks, and `%5C%5Chost` anywhere, passed the allowlist as relative references | parse/130, render/121 |
| C60 | A kept template part's relationships to the template's own slides (of the slide type, or to a part whose content type is a slide's or whose name one of the deck's slides or notes pages takes) are removed with what refers to them, as active content is: a master's or layout's jump to a template slide loses its link and keeps its shape, a slide only such a relationship reached isn't kept, and a jump that names no slide stays. Nothing is reported unless §10.4 removes the relationship whatever refers to it (§8.8, §10.4) | A jump kept a sample slide the presentation no longer lists, or jumped to whichever new slide took the sample's part name | render/122 |
| C61 | A deck with notes in a template without a notes master gets the house template's notes master and its theme, under part names the template doesn't use, with its slide image fitted to the slide size, and the presentation lists it; a deck without notes is written without one. A part the output writes anew never takes a relationships part the template has for its name, and new presentation relationships take ids the template's don't use (§8.8) | Every slide's notes were dropped silently; a stray relationships part could attach to new media; a template relationship id could be repeated | render/119, render/123–125 |
| C62 | Fonts are specified: the families checked; families compared without ASCII case and resolved per style through the font resolver, a docx template's embedded fonts and bundled stand-ins (Carlito for Calibri and Calibri Light, Consolas's fixed advance); `font_unresolved` always validate-stage, and not for fonts a pptx template embeds; how a face is chosen in a collection and per character class; measurement as shaped advances with the viewers' kerning, in integer arithmetic, the same in every host; the CLI's JSON lockfile pinning SHA-256 digests in a content-addressed cache; font files limited to 64 MiB, 256 faces, 512 tables and 64 character-map records (§3, §10.3, §11.2, §12.1, §13) | The chain, lockfile, measurement and house fonts were named but not specified; `font_unresolved` was render-stage without a font resolver, so `lint` and `convert` could disagree | `crates/compyla-fonts` tests: committed widths, run natively and on wasm32; lint/139, lint/144–146, render/119, render/121–125 |
| C63 | Text in pptx is measured against its box as PowerPoint lays it out: insets, styles inherited along placeholder, layout, master and presentation, UAX #14 lines whose trailing spaces hang, a pitch of 1.2 × the size × the spacing (the last line reaching into the face's descent where spacing is tight), paragraph spacing, tab stops from the inset, capitals, and table rows that grow with their text. `content_overflow` is validate-stage, once for each box. A box that shrinks text (`normAutofit`) gets PowerPoint's scale written, and reports only when 25% doesn't fit; `spAutoFit` is measured as fixed (§3, §8.6, §8.7) | `content_overflow` was render-stage and how text was measured wasn't specified; PowerPoint doesn't autofit a file it opens, so a shrinking template's text overflowed until someone edited it | lint and render fixtures (#82) |

0.2 also corrects 0.1's *text* in one place where the prototype was right: `---` in xlsx is ignored
without a diagnostic (§5).

---

## 1. Input and Base Syntax

### 1.1 Input text *(C25)*

- Input is Unicode text. Hosts decode input bytes as UTF-8; bytes that aren't valid UTF-8 are the
  operation error `input_not_utf8` (§10.6). Hosts that receive strings rather than bytes (JavaScript,
  Python) MUST replace unpaired surrogates with U+FFFD, and each produces `invalid_character`.
- A leading U+FEFF (byte-order mark) is removed before anything else.
- LF, CRLF and CR each end a line. Every string value in the IR uses LF.
- Characters XML 1.0 forbids (U+0000–U+0008, U+000B, U+000C, U+000E–U+001F, U+FFFE, U+FFFF) are
  replaced with U+FFFD before parsing. Each replacement produces `invalid_character` (warning) at its
  position. Without this, they would corrupt docx/pptx/xlsx output.
- Every pattern in this spec is ASCII-only: `[0-9]` means ASCII digits and `[A-Za-z]` ASCII letters.

### 1.2 Markdown

- **CommonMark** 0.31.2, with these **GFM** extensions as specified in GFM 0.29-gfm: tables, task list
  items, strikethrough (`~text~` and `~~text~~`) and autolinks, including extended autolinks. A
  `www.` autolink's destination gets an `http://` prefix and an email autolink's gets `mailto:`.
- GFM footnote syntax is recognized but not supported (§4.8). There is no smart punctuation.
- **YAML frontmatter** (§2) and **Compyla directives**: fences (§1.3) and markers (§1.4).

Wherever this spec says "per CommonMark", the CommonMark 0.31.2 specification is normative.

### 1.3 Directive fences *(C2, C3, C4, C21)*

```markdown
::: name args
...children (markdown)...
:::
```

Fences are recognized line by line, before markdown parsing, over every line except frontmatter
(§2):

```
opener = colons [ws] name [ws args] [[ws] colons] ; the line starts at column 1
closer = colons [ws]                              ; the line starts at column 1
colons = 3*":"
name   = ALPHA *( ALPHA / DIGIT / "_" / "-" )      ; ASCII, case-insensitive
args   = the rest of the line, with surrounding whitespace trimmed
ws     = 1*( SP / HTAB )
```

1. Openers and closers are recognized **only at column 1** and **only outside fenced code blocks**.
   Fenced code blocks are tracked per CommonMark: an opening run of three or more backticks or tildes,
   indented at most three spaces, closed by a run of the same character at least as long, indented at
   most three spaces, with nothing else on the line. A code fence still open at the end of the
   document runs to the end, so any `:::` lines after it are literal.
2. **Names are case-insensitive.** The IR records them in lowercase.
3. Fences nest. A closer closes the most recently opened fence; colon counts don't need to match.
4. An opener ending with a run of three or more colons (`::: toc :::`, `:::pagebreak:::`) is a
   **one-line directive**: complete on its own, with no children and no closer.
5. An opener whose name is a **marker** (`toc`, `pagebreak`, `col`; §1.5) is complete on its line. If
   the very next line is a closer, that closer belongs to it. So `::: toc` and `::: toc` + `:::` mean
   the same.
6. The lines between an opener and its closer are the directive's **children**. They are parsed as
   markdown on their own, keeping their absolute source positions. Sections inside a directive form
   their own heading tree (§4.2). Link reference definitions are document-wide (§4.5).
7. An opener or closer **always starts a new block and ends the previous one**: no paragraph
   continuation, setext underline or table row crosses it, even directly after a paragraph line.
8. An opener still open at the end of the document is **unclosed**. It becomes a directive with no
   children, a `directive_unclosed` warning is reported at the opener, and the lines after it are
   parsed as ordinary content of the enclosing context.
9. These lines produce `directive_malformed` (warning, at the line's first colon):
   - a column-1 line starting with three or more colons that matches neither the opener nor the
     closer grammar (Pandoc's `::: {.note}`, `:::note{.x}`), or a closer with no open fence. The line
     stays literal text, except that a closer with no open fence is dropped (it still ends the
     previous block).
   - a line that would be an opener or closer but is indented by 1–3 spaces, or follows blockquote
     markers (`>`), e.g. inside a list item or a quote. It stays literal text.

   Lines inside fenced code blocks never produce this, including fenced code inside blockquotes and
   list items.

A directive's position is its opener line, column 1.

### 1.4 Markers

An HTML block consisting of a single comment whose content is exactly a name (`<!-- name -->`,
whitespace, including line breaks, allowed inside the delimiters) is a **marker directive**: no args,
no children, positioned at the `<`. Names are case-insensitive, as in §1.3.

```markdown
<!-- col -->        column separator inside a pptx slide
<!-- pagebreak -->  explicit page break (docx/pdf)
```

Any other HTML block consisting only of a comment is ignored without a diagnostic. An HTML block that
starts with a comment and has other content (`<!-- col --> more text`) is an HTML block (§4.8).

### 1.5 Directive registry *(C8, C20)*

Every directive the dialect defines, with its kind and its effect in each profile. "Ignored" means no
diagnostic: an ignored **annotation**'s children render in its place, an ignored **marker** produces
nothing, and an ignored **container** is omitted with its children. This lets one document target all
four formats without warnings for directives meant for another format.

| Name | Kind | docx / pdf | pptx | xlsx |
|------|------|------------|------|------|
| `toc` | Marker | Table of contents (§6.1) | Ignored | Ignored |
| `pagebreak` | Marker | Page break | Ignored | Ignored |
| `col` | Marker | Ignored | Column break (§8.4) | Ignored |
| `image` | Annotation | Image width (§6.1) | Image fit (§8.4) | Ignored |
| `layout` | Annotation | Ignored | Slide layout (§8.3) | Ignored |
| `columns` | Annotation | Ignored | Ignored | Column types (§9.4) |
| `anchor` | Annotation | Ignored | Ignored | Table placement (§9.4) |
| `table` | Annotation | Ignored | Ignored | Excel table (§9.4) |
| `notes` | Container | Ignored | Speaker notes (§8.2) | Ignored |

- **Container**: its content is its children only.
- **Annotation**: applies to the blocks it wraps. When it has no children (an empty fence, a one-line
  directive, or an unclosed opener), it applies to the content that follows, as defined by its
  profile.
- **Marker**: has no content.

A name outside this table produces `unknown_directive` (warning), and its children render in its
place. The names `docx`, `pdf`, `pptx` and `xlsx` are reserved (§3) and are unknown in 0.2.

"Render in its place" means the children are rendered where the directive is, keeping the tree as
parsed: sections inside the body stay in the body's own heading tree and never adopt content that
follows the directive. Profiles that walk the document in order (pptx slides §8.1, xlsx sheets §9.1)
enter the bodies of annotations (whether the profile uses or ignores them) and of unknown
directives, but not of containers.

A directive the profile uses, in a position where it can have no effect, produces
`directive_misplaced` (warning) and is then treated as ignored. Examples: a `layout` that isn't the
first block of a slide, a `col` inside a list, an `image` with no image to apply to. Each profile lists
its cases. A misplaced directive produces nothing else: its arguments aren't checked until it is
somewhere it has an effect. *(C46)*

### 1.6 Directive arguments

| Directive | Arguments |
|-----------|-----------|
| `toc`, `pagebreak`, `notes`, `col` | None |
| `layout` | Free text: the layout name (§8.3) |
| `image` | Options: `width=N` or `width=N%` (docx, pdf), where `N` is an integer 1–100 with no leading zero, giving the percentage of the text width *(C16)*; `fit=contain` or `fit=cover` (pptx) |
| `columns` | A comma-separated list of entries (§9.4) |
| `anchor` | One cell reference (§9.4) |
| `table` | Options: `name=…`, `style=striped\|plain`, and the flag `autofilter` (§9.4) |

**Options** are separated by commas and/or whitespace. Each is `key=value` or a bare flag. Keys and
flags are case-insensitive, and so are enumerated values (`fit=Cover`). A value runs to the next
separator, and there is no quoting in 0.2.
Options for another profile are ignored quietly, for the same reason as §1.5 (`fit=` in docx).

These produce `unknown_directive_option` (warning), and the option is ignored, so its default applies:
- an unknown key or flag
- a value outside an option's enumerated values (`style=fancy`, `fit=stretch`)
- arguments given to a directive that takes none

These produce `invalid_directive_args` (warning), and the directive or option falls back as its
profile defines:
- a repeated key
- a malformed or out-of-range value (`width=150%`)
- a missing required argument (`::: layout` with no name)

## 2. Frontmatter *(C7, C28)*

```yaml
---
format: pptx            # optional; an explicit format from the caller wins
template: acme-brand-v2 # template reference, resolved by the host (§10.3)
title: Q3 Review        # document properties; default xlsx sheet name
author: J. Doe
lang: en
spec: "0.2"             # optional; the spec version the document was written for
---
```

- Frontmatter starts with a line that is exactly `---` (trailing spaces or tabs allowed) on line 1,
  and ends at the next such line. `...` does not end it. Without a closing line there is no
  frontmatter, and line 1 is ordinary markdown. A document whose first line is `---` as a break, and
  which contains a later `---` line, is read as frontmatter: start such a document with a blank line.
- Its content is a **JSON-compatible subset of YAML 1.2** (core schema): a mapping with string keys,
  whose values are mappings, sequences, strings, finite numbers (integers within ±(2⁵³−1)), booleans
  or null. **Not allowed:** anchors and aliases, explicit tags, merge keys (`<<`), complex keys,
  duplicate keys and multiple documents. Frontmatter longer than 64 KiB is not parsed and counts as
  disallowed. Empty frontmatter is an empty mapping.
- Content that isn't valid YAML, isn't a mapping, or uses a disallowed feature produces
  `frontmatter_invalid` (warning, at line 1), and `meta` is empty.
- Frontmatter lines are excluded from markdown parsing and from directive recognition. `meta` keeps
  the source order of keys.

**Known keys.** Values are checked and normalized:
- A number or boolean given for a string key is converted to its shortest decimal or `true`/`false`
  text (so `spec: 0.10` becomes `"0.1"`; quote versions).
- Enumerated values are matched case-insensitively and stored in canonical case (`format: PPTX` →
  `"pptx"`).
- A value of the wrong type or form produces `meta_value_invalid` (warning, at the key's line), and
  the key is removed from `meta`.

Unknown keys are preserved verbatim.

| Key | Type | Meaning |
|-----|------|---------|
| `format` | `docx` \| `pdf` \| `pptx` \| `xlsx` | Target format when the caller gives none (§10.2) |
| `template` | string | Template reference for the host's template resolver (§10.3) |
| `title` | string | Document title property; default xlsx sheet name. Not rendered as visible content |
| `author` | string, or a list of strings (joined with `, `) | Document author property |
| `lang` | BCP 47 tag (`[A-Za-z]{2,3}(-[A-Za-z0-9]{1,8})*`) | Document language: language metadata and hyphenation. When absent, the template's language, else `en` |
| `spec` | string | A value other than `0.2` produces `spec_version_unsupported` (warning), and the document is processed under 0.2 |
| `page_size`, `margins` | string | Page geometry for docx and pdf (§7), validated by those profiles |

Diagnostics about a key are reported at the line where that key appears.

**Fonts are never specified here or anywhere in the markdown** (§3).

## 3. Base Tokens, Per-Format Overrides, and Fonts

Markdown parses into the IR's **base tokens** (§4): format-agnostic intent that every profile can
render.

**Per-format overrides are reserved.** 0.1 sketched override blocks (`::: pptx` containing
`layout: Two Content`), but they were never implemented, and their semantics are not specified in
0.2. *(C17)* Until a later version specifies them, the directive names `docx`, `pdf`, `pptx` and
`xlsx` behave as unknown directives.

**Fonts.** Markdown never names a font. The **template** declares font families, and operations read
font bytes only through the chain below, so they run offline and pinned (architecture.md D14).
*(C62)*
- **Families checked:** the theme's major and minor fonts (`latin`, and `ea` and `cs` where they
  aren't empty), plus the fonts named by every style the output uses. With a pptx template, those
  are the typefaces (`a:latin`, `a:ea`, `a:cs`) of the parts a pptx output keeps: the presentation's
  default text style and table styles, each slide master and its layouts, and the notes and handout
  masters, each master with its own theme's major and minor fonts. Relationships to these parts are
  found by their type's last segment, compared without ASCII case, as the notes master is (§8.8).
  With a docx template, they are the fonts its document defaults name (`w:rFonts`). Theme references
  (`+mj-lt`, Word's `w:asciiTheme` and its siblings) aren't families. Symbol fonts used only for
  bullets (e.g. Wingdings) and a theme's per-script fonts (`a:font script="…"`) are not checked.
- **Families and styles.** A family is named as the template names it, and families compare by their
  ASCII-lowercase form. A style is regular, bold, italic or bold italic (`boldItalic` in JSON). Each
  style resolves on its own. A style that doesn't resolve while its family's regular does is
  measured with the regular face, as viewers synthesize bold and italic, and isn't reported.
- **Resolution.** A family and style resolve to font bytes through this chain; the first that has
  them wins:
  1. the font resolver (§10.3); an operation without one resolves nothing here;
  2. a font a docx template embeds (`w:embedRegular` and its siblings), obfuscated as ECMA-376 Part
     1 §17.8.1 describes or not, as its content type says. Word may embed only the characters a
     document used, which is why a pinned font comes first;
  3. a stand-in the engine bundles: **Carlito** (SIL Open Font License) in its four styles for
     Calibri and Calibri Light, and, for Consolas, its fixed advance (below), with no font file.

  A family whose regular style none of these resolves is measured with Carlito and produces
  `font_unresolved` (warning, strict-promotable, validate stage, §11.2) once; the viewer substitutes
  a font. In pptx output, a family the template embeds (`p:embeddedFont`, whose regular face
  `p:regular` is a part of the package) isn't reported: the output keeps the embedding and
  PowerPoint shows it. The engine doesn't read Embedded OpenType, so such a family is measured as
  the chain resolves it, else with Carlito; in other outputs (a pdf from a `.potx`) it is reported
  like any other.
- **Ahead of rendering,** `compyla fonts sync` pins a font for each family checked that has no
  stand-in and that the template doesn't embed in a form the engine reads: from a user-supplied
  file, else from **Google Fonts**. It may pin a metric-compatible open font under the template's
  family name (Arimo for Arial, Tinos for Times New Roman, Cousine for Courier New, Caladea for
  Cambria). Pins go in a lockfile (§10.3).
- **Font bytes** are a TrueType or OpenType font, or a collection, within §12.1's limit; bytes that
  aren't a font the engine reads count as not found. In a collection, the face is the first whose
  family name (name ID 1, in any of its records) matches the family and whose style matches (OS/2
  `fsSelection` bold and italic bits), else the first whose family matches, else the first face. A
  single font is used whatever its names say, so a pinned substitute measures under the template's
  name. A variable font is measured with `wght` at 400, or 700 for bold, and for italic `ital` at 1,
  or where it has no `ital`, `slnt` at its minimum, each where it has that axis and clamped to its
  range; its other axes stay at their defaults.
- **Measurement** serves the compose stage (D15): what fits a placeholder, a cell or a page. A run
  is text with one size and one set of run properties.
  - **Classes.** Each character has a class from its own Script and East Asian Width (UAX #24, UAX
    #11): *East Asian* if its script is Han, Hiragana, Katakana, Hangul or Bopomofo or its width is
    Wide or Fullwidth; *complex* if its script is Arabic, Hebrew, Syriac, Thaana, N'Ko, Devanagari,
    Bengali, Gurmukhi, Gujarati, Oriya, Tamil, Telugu, Kannada, Malayalam, Sinhala, Thai, Lao,
    Tibetan, Myanmar or Khmer; *latin* otherwise, Common characters included. A *mark* (a nonspacing
    or enclosing mark, Mn or Me, or an Inherited character) takes the class of its base, the nearest
    character before it in the run that isn't a mark, else of the first after it, else latin.
  - **Faces.** A character is measured with the run's font for its class: in pptx, its `a:latin`,
    `a:ea` or `a:cs` typeface; in docx, `w:ascii` for U+0000–U+007F and `w:hAnsi` otherwise,
    `w:eastAsia` or `w:cs`; theme references resolved through the theme. In docx, a complex
    character also takes the run's complex-script size, bold and italic (`w:szCs`, `w:bCs`,
    `w:iCs`). An East Asian or complex font that is empty or unnamed means the latin one. A
    character that face doesn't map is measured with the first of the run's latin, East Asian and
    complex faces, then Carlito, that maps it; the Consolas stand-in maps every character. A mark is
    measured with the face its base is measured with, whatever that face maps; where the face maps
    neither the mark nor a composition of it with its base, the mark advances nothing.
  - **Pieces.** A run is split into pieces wherever the face or size changes, the script changes
    (each character's resolved script: a Common or Inherited character takes the script of the
    nearest character before it that has one, else after it) or the bidi level changes (UAX #9; a
    paragraph's direction is its `rtl` in pptx or `w:bidi` in docx where set, else its first strong
    character's). A mark stays in its base's piece.
  - **Shaping.** Each piece is shaped with its face's OpenType layout, for its script and direction
    and the document's language (§2). These features apply: `rvrn`, `ccmp`, `locl`, `rlig`, `ltra`,
    `ltrm`, `rtla`, `rtlm`, `mark`, `mkmk`, `abvm`, `blwm`, `rclt`, `curs` and `dist`, `calt` (in
    docx only where the run's `w14:cntxtAlts` is on), and the features a shaper adds on its own for
    the piece's script (Arabic's joining forms, the Indic reordering and conjunct features). `calt`
    applies in pptx unverified: it changes no width in Calibri, Carlito or Aptos. Kerning (`kern`)
    applies in pptx where the run's effective `kern` is greater than 0 and the size is at least
    `kern`; with `kern` 0 or absent everywhere it doesn't. Standard ligatures don't apply in pptx.
    Both are as PowerPoint renders text (checked in PowerPoint 16.114 for Mac; ECMA-376's wording
    for an absent `kern` says otherwise). In docx, kerning applies where the run's effective
    `w:kern` is greater than 0 and the size is at least its value in half-points, and the ligatures
    its `w14:ligatures` names apply: each of `standard` (`liga`), `contextual` (`clig`),
    `historical` (`hlig`) and `discretional` (`dlig`) that the value's name contains, compared
    without case, all four for `all`, none for `none`. No other feature applies, including `frac`,
    `numr` and `dnom`, which shapers turn on around U+2044 FRACTION SLASH by themselves, and `rand`
    and `trak`.
  - **Width.** A piece's width is the sum of its shaped glyphs' advances in font units, times the
    size in EMU (1 pt = 12,700 EMU), divided by the face's units per em, rounded down, in exact
    integer arithmetic, plus the run's character spacing (pptx `spc`, docx `w:spacing`) once for
    each character outside the set that advances nothing (below). A run's width is the sum of its
    pieces'. Where lines break, whether trailing spaces count, and what overflows are the compose
    stage's (§8.6): lines break at UAX #14 opportunities, and inside a word only where it is wider
    than a whole line.
  - **Characters no face maps.** A default-ignorable code point (Default_Ignorable_Code_Point), a
    nonspacing or enclosing mark (Mn, Me) or a control (Cc) advances nothing, and stays in the piece
    around it, so it doesn't break shaping across it (kerning across a soft hyphen, say). Any other
    advances 1 em when its East Asian Width is Wide or Fullwidth; else a space separator (Zs)
    advances as the face's U+0020 where it maps one; else ½ em. Tabs and line breaks are the compose
    stage's. Unicode properties are those of the Unicode version the implementation uses.
  - **Consolas without a font** advances each character 1,126/2,048 em, its advance in every style,
    except characters the previous bullet makes advance nothing, and Wide and Fullwidth characters,
    which advance 1 em.
  - **Known bias.** Carlito measures Calibri exactly and Calibri Light about 1–2% wide. A family
    measured with Carlito because it is unresolved may differ more from the font the viewer
    substitutes: Arial is about 10% wider than Carlito, and Aptos about 4% wider.
- **Stand-ins** are part of the implementation, identical in every build and host (§13), and every
  distribution that bundles Carlito carries its copyright notice and licence. They are used for
  measurement, and Carlito also draws pdf text for Calibri and Calibri Light (D13). Docx, pptx and
  xlsx keep the template's family names.

## 4. IR Mapping

The IR is defined by `spec/ir.schema.json`. This section defines how markdown maps to it. The IR
produced by `parse` is fully determined by the input: two conforming implementations produce equal
IR JSON (key order aside).

### 4.1 Document

```json
{ "irVersion": "0.1", "meta": { }, "body": [ ] }
```

`meta` is the normalized frontmatter mapping (§2). `body` is a list of block nodes. Every node except
a list item has a `type`. Every node produced by `parse` has a `position` (`{ "line", "column" }`); IR
supplied as input may omit positions (§10.7).

**Where a position points.** Columns are counted from the start of the physical line, so container
prefixes (`> `, list indentation) are included:

| Node | Position |
|------|----------|
| `section` | The first `#` of an ATX heading; the first character of a setext heading's text |
| `paragraph` | Its first character. In a task-list item, the first character after the checkbox and its following whitespace |
| `media` | The image's `!`, or the `[` of the link around it |
| `table` | The first character of the header row |
| `list` | Its first item's marker |
| list item | Its marker |
| `code` | The first character of the opening fence; for indented code, the first character of its content (an indented block has no opening delimiter) |
| `quote` | The first `>` |
| `break` | The first character of the break line |
| `directive` | Column 1 of the opener line; for a marker, the `<` |

### 4.2 Sections

A heading (ATX or setext) opens a `section` with its `level` (1–6) and `title` (inline runs). A
section's `children` are the blocks that follow it up to the next heading of the same or a higher
level (smaller or equal number). Deeper headings nest as child sections. Heading trees are built
separately inside each list item, blockquote and directive body, and never cross out of it. An empty
heading (`#`) has an empty `title`.

### 4.3 Blocks

| Markdown | IR node | Notes |
|----------|---------|-------|
| Paragraph | `paragraph` (`runs`) | A paragraph whose runs end up empty (after §4.8 drops) is omitted |
| Paragraph made only of images (each optionally linked), separated by whitespace | one `media` (`src`, `alt`, `link`?) per image | §4.7 *(C34)* |
| GFM table | `table` (`header`, `align`, `rows`) | §4.6 |
| List | `list` (`ordered`, `start`?, `items`) | `start` is present for an ordered list whose first number isn't 1 *(C35)*. Each item has `children`, `checked` (`true`/`false` for a task-list item, else `null`) and `position`. Not represented: bullet characters, delimiters, tight/loose spacing |
| Fenced or indented code | `code` (`lang`?, `value`) | `lang` is the first word of the info string after CommonMark decoding, split on ASCII whitespace, omitted when empty. Other info-string words are not represented. `value` is the content with exactly one final line ending removed |
| Blockquote | `quote` (`children`) | — |
| Thematic break (`---`, `***`, `___`) | `break` | Meaning per profile (§5) |
| Directive fence or marker | `directive` (`name`, `args`, `children`) | `name` lowercase; `args` the unsplit argument string, `""` when absent |
| Link reference definition | — | Consumed: it resolves references (§4.5) and produces no node |

### 4.4 Inline runs *(C5, C22, C34)*

Inline content becomes a flat list of **runs**: `{ "text", "bold"?, "italic"?, "code"?, "strike"?,
"link"? }`.

- `**strong**` → `bold`; `*emphasis*` → `italic`; `~~strike~~` → `strike`; `` `code` `` → `code`.
  Marks nest: `***x***` is one run with both `bold` and `italic`. Delimiter rules, entity and numeric
  character references, backslash escapes and code-span content all follow CommonMark.
- Links set `link` (§4.5) on the runs of their text. A link with no text gets its destination as its
  text.
- A **soft line break** becomes a single space. A **hard line break** (two or more trailing spaces,
  or a trailing backslash) becomes `"\n"`. Inline `<br>`, `<br/>` and `<br />` (any case) also become
  `"\n"`.
- **Canonical form:** a mark that isn't set is absent, never `false`. Adjacent runs with identical
  marks and identical `link` are merged, and runs with empty text are dropped. This makes run
  boundaries independent of the parser.

### 4.5 Links *(C23, C24)*

- A link's `link` value is its destination after CommonMark processing (backslash escapes and
  character references decoded, no percent-encoding normalization). Link titles are not represented.
- **Reference-style links and images** (`[text][ref]`, `[text][]`, `[text]`) resolve per CommonMark.
  Definitions apply **document-wide**: a definition anywhere in the document, including inside a
  directive body, resolves references anywhere else. The first definition of a label wins.
- **Allowed destinations:**
  - `http:`, `https:`, `mailto:` and `tel:` URLs
  - fragment links (`#…`)
  - relative references that don't start with `/` or `\` and contain no `\`

  A scheme is `[A-Za-z][A-Za-z0-9+.-]*:` at the start of the destination, compared
  case-insensitively. Anything else produces `link_scheme_blocked` (warning), the link is removed,
  and its text is kept. That covers other schemes (`javascript:`, `data:`, `file:`, `ms-msdt:`, …),
  protocol-relative `//host` and UNC `\\host` paths, and absolute paths.

  A destination is checked as a browser reads it *(C59)*: without leading and trailing spaces and C0
  control characters, and without tabs and line breaks anywhere in it, so ` file:///…` (in IR
  input, §10.7, or a template's hyperlink, §10.4) is a `file:` link. Any other control character
  refuses a destination, and so does a leading space or formatting character that isn't ASCII
  (`U+00A0`, `U+FEFF`, …) or a replacement character. Percent-decoded, without the control
  characters it decodes to, the part before any `?` or `#` must pass these rules too: `%5C%5Chost`
  is a UNC path, `%2F…` an absolute path, `%C2%A0file:` a hidden scheme, and an encoded colon a
  scheme.
- **Fragment links** (`[see costs](#costs)`) target a heading or another per-format target (§6.2,
  §8.7, §9.5). A target's **slug** is computed from its plain text:
  1. Convert to lowercase (Unicode default case conversion).
  2. Remove every character that isn't a letter, mark or number (Unicode categories L, M, N), `_`,
     `-` or a space.
  3. Replace each space with `-`.
  4. If the slug is already taken by an earlier heading, append `-1`, `-2`, … (the smallest unused
     suffix).

  Heading slugs are assigned to every section in document order, matching GitHub's anchors for
  typical headings. A fragment is compared to slugs after percent-decoding.

### 4.6 Tables *(C1)*

- A table is recognized per GFM 0.29: cells are split on unescaped `|`, the outer pipes are optional,
  and `\|` is a literal pipe, even inside code spans.
- `header` is the header row's cells. `align` has one entry per header column: `"left"`, `"center"`,
  `"right"` or `null`.
- `rows` keeps **the number of cells each row actually has in the source**. GFM's display
  normalization (padding short rows, truncating long ones) is not applied in the IR, so profiles can
  report and repair ragged tables (§5). *Implementation note: parsers that normalize rows in their
  syntax tree must re-split each row line from the source, as the comrak check does
  (spikes/README.md).*
- Each cell has `runs` (its inline content, §4.4) and `raw`: **the cell's source text between its
  delimiting pipes, with surrounding spaces and tabs trimmed and `\|` unescaped to `|`**. Formula
  detection uses `raw` (§9.3); everything else uses `runs`.

### 4.7 Images *(C30, C34)*

- Images in a paragraph made only of images (each optionally inside a link, separated by whitespace)
  become one `media` node each, in order. `src` is the destination, processed as in §4.5. `alt` is
  the plain text of the description, per CommonMark. `link` is the surrounding link's destination,
  when there is one and it is allowed (§4.5). A disallowed one produces `link_scheme_blocked` at the
  link, and the media has no `link`. Image titles are not represented.
- Image bytes come from the asset resolver (§10.3), except `data:` URIs with base64 content, which the
  core decodes itself. An image `src` is not subject to the link allowlist.
- An image's format is determined from its **bytes**, not its extension. PNG, JPEG and GIF are
  supported in docx, pdf and pptx. Any other format produces `image_unsupported` (warning,
  strict-promotable) when its bytes are available, and the image is rendered as an unresolved image
  (§5).

### 4.8 Unsupported content *(C6, C36)*

These constructs are recognized but have no IR representation in 0.2. Each produces one
`unsupported_content` warning at its position, and the construct is dropped:

- an image in a paragraph that also contains text
- a footnote reference and a footnote definition (GFM footnotes)
- an HTML block other than a comment or a marker (§1.4)
- an inline HTML tag, open or close, whose name is a **known HTML element** (below), other than `<br>`.
  The text between tags is kept as ordinary runs.

An inline tag whose name isn't a known HTML element (`Promise<void>`, `List<String>`, `Vec<T>`) is
**literal text**, not HTML. Known HTML elements (case-insensitive): `a abbr address area article
aside audio b base bdi bdo big blockquote body br button canvas caption center cite code col colgroup
data datalist dd del details dfn dialog div dl dt em embed fieldset figcaption figure font footer
form h1 h2 h3 h4 h5 h6 head header hgroup hr html i iframe img input ins kbd label legend li link
main map mark menu meta meter nav noscript object ol optgroup option output p picture pre progress q
rp rt ruby s samp script search section select slot small source span strike strong style sub
summary sup table tbody td template textarea tfoot th thead time title tr track tt u ul var video
wbr`.

### 4.9 Parse diagnostics *(C29, C37)*

Besides the codes above, parsing reports:

- `ambiguous_break` (warning, advisory) when a setext heading is underlined with `-` characters,
  reported at the underline's line. CommonMark makes a paragraph line directly above `---` a level-2
  heading, and in LLM output this is almost always meant as a break. The hint says to add a blank
  line before `---` for a break, or to use `## ` for a heading.
- `table_malformed` (warning) when a paragraph's first line contains `|` and its second line is a
  delimiter row (`[ |:-]+` containing `-`) with a different cell count, so it isn't recognized as a
  table. The paragraph is kept as text.
- `document_in_code_fence` (warning, advisory) when the whole body (after frontmatter) is one fenced
  code block whose language is `markdown` or `md`, which LLMs produce when asked for markdown. The
  hint says to remove the fence.
- `limit_exceeded` (error) when the input exceeds a limit in §12.1.

---

## 5. Rendering Rules Common to All Formats

| IR | docx | pdf | pptx | xlsx |
|----|------|-----|------|------|
| `break` | Horizontal rule *(C19)* | Horizontal rule *(C19)* | **Slide break** (§8.1) | Ignored, no diagnostic |
| `"\n"` in run text | Line break | Line break | Line break | Line break in the cell, with wrap text on |
| `link` | Hyperlink | Link annotation | Hyperlink | Cell hyperlink (§9.5) |
| `media` | Picture | Image | Picture | Ignored (§9.1) |
| `media.alt` | Picture description | Alternative text | Picture description | — |
| `meta.title` / `author` / `lang` | Document properties, language | Document metadata, language | Document properties, language | Workbook properties |

- Page breaks in docx and pdf are explicit: `::: pagebreak` or `<!-- pagebreak -->`.
- **Unresolved images.** An image that doesn't resolve (`image_unresolved`) or isn't supported
  (`image_unsupported`) is rendered as its alt text in italics, in brackets (`[alt]`), or `[image]`
  when the alt text is empty. Both diagnostics are strict-promotable.
- **Ragged tables.** In every profile, a row with fewer cells than the header is padded with empty
  cells, and a row with more cells widens the table: the extra columns get empty header cells. Either
  produces `table_not_rectangular` (warning, strict-promotable) at the row's line.
- **Images are sized** at their intrinsic size (96 DPI unless the image declares a resolution, with
  JPEG EXIF orientation applied), scaled down to fit the available width and never scaled up, unless
  `::: image` says otherwise.
- **Document properties:** title, author and language come from `meta`, and are not written when `meta`
  lacks them. Everything else (application, dates) is a fixed value (§13). They are never copied from the
  template (§10.4) or from the rendering library's defaults. *(C44)*
- Sections inside list items and quotes are rendered as emphasized paragraphs, not headings. They
  don't appear in a table of contents or outline, and are not link targets.

---

## 6. Profile: docx

Output is written **into the template** (architecture.md D12). IR nodes map to the template's styles
by **style name**, compared case-insensitively against the names stored in the template. Word stores
built-in style names in English (e.g. `heading 1`) whatever the user's language. A style missing from
the template is added from the house template.

| IR | Word output |
|----|-------------|
| `section` level n title | Paragraph style `Heading n`; the template's heading numbering, if any, applies |
| `paragraph` | `Normal` |
| bold / italic / strike runs | Character formatting |
| code runs | Character style `Inline Code` |
| link runs | The `Hyperlink` character style |
| `quote` | Paragraphs inside it use `Quote`; other blocks inside it render normally, indented |
| `table` | The template's default table style; the header row repeats across pages |
| `code` | Paragraph style `Code`; no syntax highlighting |
| `media` | Inline picture (§5) |
| `list` | The template's bullet and numbering definitions. Each IR list is its own numbering instance (restarting at `start`). Nesting beyond 9 levels renders at the 9th level |
| task-list item | Checkbox glyph (☐ / ☒), not an interactive control |

Headers, footers, page size and margins come from the template. `page_size` and `margins` in
frontmatter override the page geometry (§7).

### 6.1 Directives

| Directive | Effect |
|-----------|--------|
| `::: toc` | Table-of-contents field covering heading levels 1–3 (§5), updated when the file is opened |
| `::: pagebreak` / `<!-- pagebreak -->` | Page break |
| `::: image width=N%` | Sizes the images it wraps, or, when empty, the immediately following block if it is a `media` block; otherwise `directive_misplaced` |

Other directives: see §1.5.

### 6.2 Fragment links

Every heading (§5) gets a bookmark named after its slug (§4.5). A fragment link resolving to a slug
becomes an internal hyperlink. A fragment that matches no slug produces `link_anchor_unresolved`
(warning), and the text is kept without a link.

### 6.3 Validation

| Code | Severity | When |
|------|----------|------|
| `heading_skip` | warning, advisory | A heading skips a level relative to its parent section in the same heading tree (H1 → H3) *(C42)* |
| `image_unresolved` | warning | A `media` source doesn't resolve through the asset resolver |
| `image_unsupported` | warning | §4.7 |
| `table_not_rectangular` | warning | §5 |
| `link_anchor_unresolved` | warning | §6.2 |
| `invalid_page_size` / `invalid_margins` | warning | §7 |
| `invalid_directive_args` / `unknown_directive_option` | warning | §1.6 |
| `unknown_directive` / `directive_misplaced` | warning | §1.5 |
| `font_unresolved` | warning | §3, §11.2 |

## 7. Profile: pdf

Same authoring surface, mapping and validation as docx (§6), rendered by the native PDF renderer via
Typst (architecture.md D13), not converted from docx. Fragment links become internal links, and
headings become PDF outline entries (bookmarks).

```yaml
page_size: A4 | Letter   # case-insensitive
margins: normal | narrow # normal: 72 pt on every side; narrow: 36 pt on every side
```

Page geometry precedence, for docx and pdf alike: frontmatter, then a supplied docx template's page
setup, then A4 / normal. In pdf, fonts and colors come from the template's theme tokens.

| Code | Severity | When |
|------|----------|------|
| `invalid_page_size` | warning | `page_size` isn't `A4` or `Letter`; the template's or A4 is used. At the key's line |
| `invalid_margins` | warning | `margins` isn't `normal` or `narrow`; the template's or normal is used. At the key's line |
| `glyph_missing` | warning | Render stage: text contains characters no available font can display (CJK, emoji, ☐/☒ without a font that has them). Typst doesn't report these. Implementations find glyphs laid out as `.notdef` (glyph id 0) in the laid-out document |

Plus every code in §6.3.

---

## 8. Profile: pptx

### 8.1 Slide segmentation *(C9, C26)*

Slides are formed by walking the body **in document order**, entering every section (its heading,
then its children) and the bodies of annotations other than the slide's leading `layout`, and of
unknown directives (§1.5). A `layout` that isn't leading is misplaced and so ignored (§1.5): it is
entered like any annotation. Every `break` reached this way ends the current slide and starts a new
one, however deeply it is nested under headings. List items, quotes, `notes` bodies and the leading
`layout`'s body are not entered, so a `break` inside them has no effect. A slide with
no title, no content and no notes is **omitted**: consecutive breaks, or a break at the start or end,
don't make empty slides.

### 8.2 Slide structure

- The slide **title** is the first heading reached in the slide, including inside a leading `layout`
  directive (§8.3).
- The slide's **content** is every other block reached, in order, after unwrapping a leading `layout`
  directive. Later headings become **demoted headings** in the content.
- `notes` directives among the content's top-level blocks are removed from the content. Their
  children, concatenated in order, become the speaker notes. Paragraphs, lists and headings keep their
  text formatting. Tables, media and code are rendered as plain text (table rows tab-separated, media
  as their alt text). An empty `notes` directive contributes nothing. A `notes` directive anywhere
  else produces `directive_misplaced`.

### 8.3 Layout selection *(C38)*

Priority order:

1. **Explicit**: a `::: layout <name>` directive that is the first content block. `<name>` is the args
   string, trimmed, with internal whitespace collapsed to single spaces. If the directive has children,
   those are the content. If it has none (empty, one-line or unclosed), the blocks after it are the
   content. *(C8)* A `layout` directive anywhere else produces `directive_misplaced`. A name that
   doesn't resolve (§8.5) produces `unknown_layout` (warning), and a missing name
   `invalid_directive_args`. Either way, the layout is then selected **as if there were no layout
   directive** (rules 2 and 3), with the directive's children (if any) still as the content.
2. **Title slide**: the first slide of a deck with more than one slide (after omission, §8.1) is
   `Title Slide`, **if its content fits Title Slide** (§8.6). Otherwise it is inferred.
3. **Inferred** from the content, first match wins:

| Content | Inferred layout |
|---------|-----------------|
| Empty (title only) | Section Header |
| Contains a `col` marker | Two Content |
| Exactly one block, a table | Title and Content |
| One media block, optionally with one text block | Picture with Caption |
| Exactly one block, a quote | Quote |
| Exactly two blocks | Two Content |
| Anything else | Title and Content |

### 8.4 Directives

| Directive | Effect |
|-----------|--------|
| `::: layout <name>` | Chooses the layout (§8.3) |
| `::: notes` | Speaker notes (§8.2) |
| `::: image fit=contain\|cover` | How the images it wraps (or, when empty, the immediately following media block) fill their placeholder (render stage) |
| `<!-- col -->` | Starts the next column. Meaningful only among a slide's top-level content in a layout with two or more content slots; elsewhere it produces `directive_misplaced` |

Other directives: see §1.5.

### 8.5 Layout binding *(C11, C38)*

The **house set** is the standard Office layout set plus Quote: `Title Slide`, `Title and Content`,
`Section Header`, `Two Content`, `Comparison`, `Title Only`, `Blank`, `Content with Caption`,
`Picture with Caption`, `Quote`. The house template is 16:9.

A layout name resolves, in order, to:
1. an explicit alias (`layoutAliases`, §10.2) naming a template layout. *(C56)*
2. a template layout with the same name (compared after Unicode simple case folding; on duplicates
   across masters, the first in master order wins);
3. for a house-set name, the template's first layout whose OOXML type (`p:sldLayout/@type`) matches:
   `title` (Title Slide), `obj` (Title and Content), `secHead` (Section Header), `twoObj` (Two
   Content), `twoTxTwoObj` (Comparison), `titleOnly` (Title Only), `blank` (Blank), `objTx` (Content
   with Caption), `picTx` (Picture with Caption). This binds localized templates
   ("Titel und Inhalt");
4. otherwise, for a house-set name, the template's Title and Content layout (type `obj`), else its
   first layout, with `layout_not_in_template` (warning, once per house layout, naming the layout
   used). No second master is imported.

Inferred layouts (§8.3) are house-set names, and bind the same way. A house-set name keeps its slots
(§8.6) whatever template layout it binds to; any other name (an alias or the template's own) fills
the template layout as its own. Names compare after Unicode simple case folding. *(C56)*

Without a template, every house-set name binds to the house template. With a template, the valid
names are the house set plus the template's own layout names and aliases. A name outside these
produces `unknown_layout`. Slide size, footers, dates and slide numbers follow the bound master.

### 8.6 Placeholders and fit *(C10, C26, C38)*

Content is measured in **blocks**:
- Consecutive paragraphs, lists and demoted headings form one **text block**.
- Every other node (table, media, code, quote) is one block.
- The children of annotations other than the leading `layout` (used, like `::: image fit=…`, or
  ignored, including a misplaced `layout`) and of unknown directives count in their place. A wrapped image is therefore a media block. Markers other
  than `col` count nothing. A `col` marker ends the current text block and separates columns.

| House layout | Content slots | Fits when |
|--------------|---------------|-----------|
| Title Slide | subtitle (text) | The content is at most one text block |
| Section Header, Title Only | none | There is no content |
| Blank | none, and no title | There is no title and no content |
| Title and Content | body (any) | At most one block |
| Two Content | left, right (any) | With `col` markers: at most two columns of at most one block each. Without: at most two blocks; the first goes left, the second right |
| Comparison | left heading (text), left (any), right heading (text), right (any) | With `col` markers: at most two columns, each an optional text block followed by at most one block (the text block fills the column's heading). Without: as Two Content |
| Content with Caption | content (any), caption (text) | At most one non-text block and at most one text block |
| Picture with Caption | picture (media), caption (text) | At most one media block and at most one text block, in either order |
| Quote | quote (quote or text) | At most one block, which is a quote or a text block |

A template layout outside the house set provides one slot per content placeholder (§8.8), in
placeholder order, each holding one block. *(C56)*

Content that doesn't fit produces `layout_overflow` (warning) against the **resolved** layout,
explicit or inferred. With an explicit layout it is positioned at the layout directive; otherwise at
the first block that doesn't fit. When the layout wasn't chosen explicitly, the hint suggests
splitting the slide. Content beyond the last slot is placed in the last slot; on a layout without
slots, it forms one slot of its own (§8.8), so content is never dropped. Text that doesn't fit the
box it is written into produces `content_overflow` (below).

**Measured fit.** *(C63)* Text is measured against the box §8.8 writes it into, by the rules
PowerPoint lays it out with (each checked in PowerPoint for Mac 16.114), at the validate stage, so
`lint` and `convert` agree (C40). Measurement is a diagnostic: PowerPoint does the final layout. Its
widths differ from the engine's by a fraction of a point a line (it appears to round each advance to
⅛ pt), and a family measured with a stand-in that is a little wide (Calibri Light, §3) runs wider,
so a word that only just fits or doesn't may wrap differently.

- **The box.** A placeholder's box is its own `a:xfrm`, else its layout placeholder's, else that of
  its master placeholder: the master's title placeholder for `title` and `ctrTitle`, and its body
  placeholder for any content placeholder (`body`, `obj` or no type, `subTitle`, `tbl`, `pic`, …). A
  text box, a band of a shared slot and a table have the geometry §8.8 gives them. Text is set
  inside the box's insets: `a:bodyPr`'s `lIns`, `tIns`, `rIns` and `bIns` from the shape, else its
  layout placeholder, else its master placeholder, else 91,440 EMU left and right and 45,720 EMU top
  and bottom. A box with more than one column (`numCol`) or vertical text (`vert` other than `horz`)
  isn't measured.
- **Text styles.** Each paragraph and run property is the first set of: the paragraph or run itself;
  the `a:lstStyle` of the placeholder, then of its layout placeholder, then of its master
  placeholder, at the paragraph's level (`a:lvlNpPr`); the master's `p:titleStyle` for a title
  placeholder and `p:bodyStyle` for a content placeholder; the presentation's `p:defaultTextStyle`.
  Only the level's properties count: PowerPoint ignores a list style's `a:defPPr`. A text box takes
  only its own properties and `p:defaultTextStyle`; a table cell takes its own, the table style's
  text properties (`a:tcTxStyle` of the parts `a:tblPr`'s flags apply to the cell, later ones
  overriding earlier: the whole table, banded rows, banded columns, last column, first column, last
  row, first row), the master's `p:otherStyle`, then `p:defaultTextStyle`. Where none sets them: 18
  pt, single spacing (100%), no space before or after, no margins, tab stops every 914,400 EMU. A
  typeface `+mj-lt` or `+mn-lt` (and `-ea`, `-cs`) is the theme's major or minor font, and fonts
  resolve as §3 says.
- **Lines.** A paragraph's lines run from its left margin (`marL`) to the box's right inset, its
  first line from `marL` + `indent`. A bullet or number sits at the start of the first line, and the
  text then starts at `marL` or right after it, whichever is further right: a bullet character is as
  wide as it is in its `a:buFont` (else the first run's font), and a number as wide as it is in the
  first run's font, whatever `a:buFont` says, both at the first run's size. Lines break at UAX #14
  opportunities (§3), and where a paragraph sets `latinLnBrk="1"` also between any two characters of
  a word; a line's trailing spaces hang past its end without counting, and a segment wider than a
  whole line breaks after the last grapheme cluster that fits, at least one per line. A body that
  doesn't wrap (`wrap="none"`) breaks lines only at line breaks. A line break (`a:br`) ends a line.
  A tab advances to the paragraph's next tab stop (`a:tabLst`), then to the next default one, every
  `defTabSz`, all counted from the box's left inset, not the margin. Text in capitals (`cap="all"`)
  is measured as capitals, and in small capitals (`cap="small"`) with its lowercase letters as
  capitals at 80% of its size.
- **Height.** A line's pitch is its largest run's size × 1.2 × its percentage line spacing
  (`a:lnSpc`, `a:spcPct`), whatever the font, or exactly its spacing in points (`a:spcPts`). The
  text's last line reaches further where the spacing is tight: it is as tall as its pitch or as ¾ of
  its pitch plus the size × 1.2 × its face's `usWinDescent` ÷ (`usWinAscent` + `usWinDescent`),
  whichever is taller (the size and face of its largest run). Each paragraph but the first adds its
  space before (`a:spcBef`) and each but the last its space after (`a:spcAft`): in points as given,
  or as a percentage of 1.2 × its first run's size. An empty paragraph is one line at its
  end-of-paragraph size (`a:endParaRPr`). The text fits when its lines and spacing add up to at most
  the box's height inside its insets.
- **In integer EMU,** with sizes in EMU and percentages in thousandths of a percent (OOXML's `90000`
  for 90%): a pitch is `size × 12 × spacing ÷ 1,000,000`, a percentage of spacing before or after
  `size × 12 × percentage ÷ 1,000,000`, and the last line's further reach `pitch × 3 ÷ 4 + size × 6
  × usWinDescent ÷ (5 × (usWinAscent + usWinDescent))`, each part rounded down before they are
  added.
- **Autofit.** A box whose `a:bodyPr` (inherited as its insets are) has `a:normAutofit` shrinks text
  that doesn't fit. PowerPoint does this only once the text is edited, not when it opens a file, so
  the engine does it and writes the result. It tries, in order, these steps of font scale and
  line-spacing reduction (in percentage points), and keeps the first at which the text fits: 100%
  and 0, 100% and 10, 92.5% and 10, 92.5% and 20, then 85%, 77.5% and on down by 7.5% to 25%, each
  with 20. A scaled size, in hundredths of a point, is `sz × scale ÷ 100,000` rounded to a whole
  point (a multiple of 100), halves up. The reduction lowers percentage line spacing (90% becomes
  70%) and leaves spacing in points alone; space before and after in points isn't scaled, and as a
  percentage follows the scaled size. For any step past the first, the shape's own `a:bodyPr` gets
  `<a:normAutofit fontScale="…" lnSpcReduction="…"/>`, both in thousandths of a percent (`92500` and
  `10000`). `content_overflow` is reported only where 25% doesn't fit either, and that step is
  written. A box that grows to fit its text (`a:spAutoFit`) is measured as one without autofit:
  PowerPoint resizes it only when its text is edited. The house template's placeholders have no
  autofit, so a house deck keeps its sizes and reports what overflows.
- **What is measured:** the title, each slot's text (paragraphs, lists, demoted headings, code and
  quotes, as §8.8 writes them) and each table. A table row is as tall as its height (`h`) or its
  tallest cell's text plus the cell's top and bottom margins (`marT`, `marB`, else 45,720 EMU),
  whichever is taller; a cell's text is set in its column's width less `marL` and `marR` (else
  91,440 EMU); the table fits when its rows fit its box. Tables don't shrink. Images are fitted
  (§8.8) and speaker notes aren't measured. Content past a layout's last slot shares it (above) and
  is measured there, so a slide can report both `layout_overflow` (its structure) and
  `content_overflow` (its space).
- **The diagnostic** is reported once for each box that overflows, at the first block whose text
  reaches past the box (a title at its heading), with the measured height and the box's. Its hint
  suggests shortening the text, splitting the slide or a layout with more room.

### 8.7 Validation

| Code | Severity | When |
|------|----------|------|
| `unknown_layout` | warning | §8.3, §8.5. The message lists the valid names |
| `layout_overflow` | warning | §8.6 |
| `layout_not_in_template` | warning | §8.5; validate stage when a template is available |
| `heading_too_deep` | warning, advisory | A demoted heading of level 4 or deeper (likely a docx-oriented document) |
| `image_unresolved` / `image_unsupported` | warning | §4.7, §5 *(C18)* |
| `table_not_rectangular` | warning | §5 |
| `link_anchor_unresolved` | warning | A fragment link matches no **slide title**'s slug. Matches become links to that slide. Slide-title slugs are numbered over the slide titles alone (§4.5 step 4) |
| `invalid_directive_args` / `unknown_directive_option` | warning | §1.6 |
| `unknown_directive` / `directive_misplaced` | warning | §1.5, §8.2–§8.4 |
| `pptx_ignored_content` | warning | An image or table inside a list item or a quote in a slide's content: a text body can't hold it, so it shows as its alt text or as tab-separated rows (§8.8) |
| `content_overflow` | warning | Validate stage, §8.6 |
| `font_unresolved` | warning | §3 |

### 8.8 Rendering *(C51, C53, C55)*

Each slide is written into its bound layout (§8.5). The title goes into the layout's title
placeholder; on a layout without one (Blank) it is a title placeholder of its own, placed over the
master's title area, so it is still the slide's title. *(C55)* Slot `i` (§8.6) goes into the layout's `i`-th content placeholder, in shape order: any
placeholder but the title, date, footer, header and slide number. An empty slot's placeholder is
left out. A slot past the bound layout's last content placeholder (a layout bound by fallback can
have fewer placeholders than the house layout has slots, §8.5) shares that placeholder, as several
blocks in one slot (below), rather than overlap it; on a layout with no content placeholder, every
slot's content is laid out together, as below. *(C56)* Content on a layout without slots
(`layout_overflow`, §8.6) goes into the layout's first content placeholder if it has one (Section
Header's text). Otherwise it is laid out over the
master's body area with no placeholder: text shares one text box, and other content divides the
area among its blocks in order, each a shape of its own. Where the master has no title or body
placeholder, the 16:9 house areas apply. Shapes are written in reading order: the title, then the
slots' shapes by column. Slots whose areas overlap horizontally form one column (joined
transitively); columns go left to right by their left edges, and a column's slots top to bottom,
then left to right. So a caption beside a picture or a table is read after the title above it, not
after the picture. A slot whose area isn't known comes last. *(C55)* Placeholders carry no geometry or formatting of
their own except where their content needs it, so they inherit the layout's.

- **Text** (a text block, code, a quote) shares one text body:
  - Paragraphs, and demoted headings (in bold), have no bullet.
  - List items are bulleted at their nesting level (at most nine levels). An item's first line takes
    the bullet, whatever it is (an empty item keeps its bullet), and its further lines align with
    its text. Numbered lists count from their start; a number outside 1–32,767, which DrawingML
    can't number, is written as text instead. Task items begin with ☑ or ☐. A content placeholder
    (type `obj`) shows the template's bullets; other placeholders (subtitles, captions, notes),
    whose styles usually have none, get explicit ones.
  - An image or a table inside a list item or a quote can't sit in a text body: it shows as its alt
    text or as tab-separated rows, with `pptx_ignored_content`.
  - Text takes the document's language (`lang`, §2), and otherwise the template's.
  - Code is one paragraph per line, indentation kept, in a monospace face (Consolas), as inline code
    is. A quote's content is italic.
- **Tables** are a table frame in the slot's geometry: the header first, the template's default
  table style, and each column's alignment. Tables and images take a placeholder only where one
  can hold them (types `obj`, `tbl` and `pic`); elsewhere they are shapes of their own in its
  geometry.
- **Images** are a picture in the slot. With `fit=contain`, the default, the whole image is scaled
  to fit and centered; with `fit=cover` it fills the slot, cropped evenly from both sides (always
  leaving some of the image). A JPEG's EXIF orientation is honored: the picture is turned and
  flipped so the image shows upright, and fitted by its upright shape. The image is written with
  its orientation tag set to 1, so a viewer that applies the tag itself doesn't turn it twice. An image whose bytes can't be
  written shows its alt text (§5), in its share of the slot.
- **Several blocks in one slot** (§8.6): if all are text, they share the placeholder's text body.
  Otherwise the slot's height is divided among them in order; the first keeps the placeholder, and
  the rest are shapes of their own.
- **Links:** an external destination is a hyperlink. A fragment matching a slide title's slug
  (§8.7) jumps to that slide; one that matches none has no link.
- **Speaker notes** (§8.2) are the body of the slide's notes page, on the template's notes master. A
  deck with notes in a template without one gets the house template's notes master and its theme,
  under the first part names the template doesn't use (`ppt/notesMasters/notesMasterN.xml`,
  `ppt/theme/themeN.xml`), with its slide image fitted to the template's slide size, and the
  presentation lists it, so no notes are lost; a deck without notes is written without one. A notes
  master is found by its relationship's type, compared as §10.4 compares types. *(C61)* Code and
  images there are plain text (an image is its alt text), and a fragment link in the notes doesn't
  jump: a notes page's own relationship to its slide must stay unambiguous.
- **The package** (§10.4): the template's own slides are removed, with every part no longer reached
  from the presentation, and a `.potx` is written as a presentation. What named those slides goes
  with them: custom shows, sections, the show settings' slide range or custom show (so the show
  plays every slide), and the outline view's slide list. So does every relationship a kept part has
  to one of those slides, with what refers to it, as §10.4 removes what refers to a removed
  relationship: one of the slide type, or one to a part whose content type is a slide's (without
  case or parameters) or whose name a slide or notes page of the deck takes (`ppt/slides/slideN.xml`
  for each of its slides, `ppt/notesSlides/notesSlideN.xml` for each with notes), since what the
  template had there is gone. A master's or layout's jump to a template slide loses its link, and
  its shape stays. A slide that only such a relationship reached isn't kept. A jump within the show
  (`hlinkshowjump`: to the next, previous, first or last slide, the last one viewed, or the end),
  which names no slide, stays. Nothing is reported for these, unless §10.4 removes the relationship
  whatever refers to it (an external target, say). *(C60)* A part the output writes anew has only
  the relationships it gives it, never a relationships part the template has for that name, and the
  presentation's new relationships take ids it doesn't already use (`cmpSlideN`, `cmpNotesMaster`,
  or that with `_2`, `_3`, … after it). *(C61)* An edited part in UTF-16 is written as UTF-8.

---

## 9. Profile: xlsx

A workbook is authored as sheets of tables.

### 9.1 Structure mapping *(C39)*

The body is walked in document order, entering sections, wrapping annotations, and the bodies of
ignored and unknown directives (§1.5).

| IR | xlsx output |
|----|-------------|
| `section` level 1 | Starts a **sheet** named after its title (§9.6) |
| `table` | A data range (§9.2) |
| `section` level 2 | **Range title** for the next table |
| `section` level 3+ | Its heading is ignored with `xlsx_ignored_content`; its children are processed in place |
| `paragraph` | **Note** for the next table |
| `columns` / `anchor` / `table` directives | Annotations for a table (§9.4) |
| `break` | Ignored |
| Anything else (quotes, code, media, lists) | Ignored, with `xlsx_ignored_content` |

Sheets appear in document order. An H1 inside a directive's body starts a sheet like any other, and
the content after the directive stays in that sheet. Content before the first H1 forms a default sheet named after
`meta.title`, else `Sheet1`. It exists only if it contains at least one table or produces
`xlsx_ignored_content`. A document that yields no sheets gets one empty sheet named the same way, with
`xlsx_empty_workbook` (warning, advisory). *(C27)*

### 9.2 Placement (normative)

Each sheet keeps a stacking cursor, starting at row 1, column A. Tables are placed in document order:

1. The table's top-left is the `anchor` cell if it has one, else column A at the cursor row.
2. The range title (if any) takes the next row, merged across the table's columns (not merged when
   the table has one column), in bold.
3. The note (if any) takes the next row, merged the same way, in italics.
4. The header row comes next, in bold on a light fill (the house style), followed by one row per
   data row. In an Excel table (§9.6), the table's style formats the header instead. *(C48)*
5. An unanchored table moves the cursor to *header row + data rows + 2* (one blank row between
   tables). An anchored table does not move the cursor.

**Theme tokens.** With a docx or pptx template (§10.4), the workbook takes the template's theme
tokens: its twelve colors and its heading and body fonts (Latin, East Asian, complex script) become
the workbook's theme, written afresh with a plain format scheme, so nothing else of the template
comes along. A color slot the theme leaves out, or whose color can't be read (an sRGB value, or a
system color's last value), takes Office's default, and a font without a Latin typeface a workbook
can name (blank, or longer than Excel's 31 UTF-16 code units) Office's (Calibri Light, Calibri). A
theme part that isn't an `a:theme` counts as no theme (§10.4). Cells use the body font and range titles the heading font, by theme
reference; the house header fill is the first accent, lighter 80%; links take the theme's hyperlink
color, unless it contrasts with the light 1 color by less than 4.5:1 (WCAG 2), when they keep the
house one. Without a template, or with one without a theme (`no_theme`), the house style applies:
Calibri 11, a light header fill (`E8ECF3`) and the house link color (`0563C1`). *(C57)*

**Column widths.** A column is as wide as the widest header or data cell in it displays, in
characters, plus 2 for padding, at most 60, and never narrower than Excel's default. *(C55)*
- Text displays its longest line. Characters in U+1100–115F, U+2E80–303E, U+3041–33FF,
  U+3400–4DBF, U+4E00–9FFF, U+A000–A4CF, U+AC00–D7A3, U+F900–FAFF, U+FE30–FE4F, U+FF00–FF60,
  U+FFE0–FFE6 and U+20000–3FFFD (East Asian wide and fullwidth) count 2.
- A number displays as its format writes it, its decimal value rounded half up to the format's
  decimals: sign, currency prefix, digits with thousands separators, decimals, `%`. A `General` number displays as written, at most 11 characters, and a date 10.
  A boolean counts 5.
- A formula's result isn't known, and can outgrow the numbers it is computed from. A formula
  displays 2 more than the widest of: the numbers in its column of its own table (or, when that
  column has none, in its whole table) and its format's `0`. A date formula displays 10.
- An Excel table's header cell with a filter button counts 2 more. Range titles and notes, merged
  across their range, widen nothing.

The placed address of data cell (row *r*, column *c*), both 0-based, is column *firstColumn + c* and
row *headerRow + 1 + r*. A table's **placed range** runs from its first row (title, note or header)
to its last data row, across its columns.
- **Overlap.** Placed ranges on a sheet MUST NOT overlap. A table, anchored or not, whose range would
  overlap an earlier one is instead placed at column A, two rows below the lowest row placed so far on
  the sheet, with `xlsx_range_overlap` (warning) at its line. The cursor then moves to *its header row
  + data rows + 2*. A table placed this way can't overlap anything. *(C27)*
- **Off the grid.** An anchor whose range would extend past row 1,048,576 or column XFD is ignored,
  with `invalid_directive_args`. Stacking past row 1,048,576 produces `xlsx_limit_exceeded` (error).

### 9.3 Formulas *(C1, C12, C33)*

A data cell is a **formula cell** when its `raw` source text (§4.6) starts with `=`. Its formula is
`raw` without the leading `=`. Header cells are always literal text. To write a literal string that
starts with `=`, escape it: `\=`. The cell's `raw` then starts with `\`, so it isn't a formula, and
its value is the run text (`=…`).

**Grammar.** A formula must match:

```
formula   = expr
expr      = concat *( cmp-op concat )
concat    = sum *( "&" sum )
sum       = term *( ( "+" / "-" ) term )
term      = power *( ( "*" / "/" ) power )
power     = unary *( "^" unary )
unary     = *( "+" / "-" ) postfix
postfix   = primary *( "%" )
primary   = number / string / boolean / call / reference / "(" expr ")"
call      = fname [ws] "(" [ arg *( "," arg ) ] ")"      ; empty arguments allowed: IF(A1,,1)
arg       = [ expr ]
reference = [ sheet "!" ] cell [ ":" cell ]            ; the sheet qualifies both cells
cell      = ["$"] col ["$"] row                        ; col A..XFD, row 1..1048576; A01 is A1
sheet     = uname / "'" 1*( qchar / "''" ) "'"
uname     = ( ALPHA / "_" ) *( ALPHA / DIGIT / "_" )    ; must not itself be a valid cell
qchar     = any character except ' : \ / ? * [ ]
fname     = ALPHA *( ALPHA / DIGIT / "." )
number    = ( 1*DIGIT [ "." *DIGIT ] / "." 1*DIGIT ) [ ( "e" / "E" ) [ "+" / "-" ] 1*DIGIT ]
string    = DQUOTE *( any character except DQUOTE / 2DQUOTE ) DQUOTE
boolean   = "TRUE" / "FALSE"                           ; case-insensitive
cmp-op    = "=" / "<>" / "<=" / ">=" / "<" / ">"
```

- Whitespace is allowed between tokens, but not between two operands, so the intersection operator
  is not supported. A reference is one token, with no whitespace inside it (`A1 : B2` is an error).
  Names, functions and booleans are case-insensitive.
- A name followed (optionally after whitespace) by `(` is **always a function call**, even if it looks
  like a cell (`LOG10(`).
- Not supported, so a formula error: defined names, structured references (`Table1[Col]`), error
  literals (`#N/A`), array constants (`{1,2}`), whole-column and whole-row ranges (`B:B`, `2:5`),
  3-D references, and a sheet name on a range's second cell (`A1:Revenue!B2`,
  `Revenue!A1:Revenue!B2`).
- A formula that doesn't match, calls a function not on the allowlist, or qualifies a sheet the
  workbook doesn't have produces `formula_error` (warning). The cell is then written as **literal
  text**, never as a formula: its `raw` text, as authored. *(C49)*

**Allowlist.** Pure spreadsheet functions only:

```
SUM SUMIF SUMIFS AVERAGE AVERAGEIF AVERAGEIFS COUNT COUNTA COUNTBLANK COUNTIF COUNTIFS
MIN MAX MEDIAN IF IFS IFERROR IFNA AND OR NOT XOR VLOOKUP HLOOKUP INDEX MATCH LOOKUP
ROUND ROUNDUP ROUNDDOWN ABS SQRT POWER MOD INT TRUNC CONCATENATE CONCAT TEXT LEFT RIGHT MID
LEN TRIM UPPER LOWER SUBSTITUTE DATE YEAR MONTH DAY TODAY NOW DATEDIF
ISBLANK ISNUMBER ISTEXT ISERROR ISNA
```

That closes the network, command and DDE vectors (`WEBSERVICE`, `RTD`, `DDE`, external `HYPERLINK`)
by construction. A non-formula cell is always written as a literal value.

**References are table-relative.** Write formulas **as if the table's header row were row 1 and its
first column were column A**, which is how a markdown table reads on its own:

```markdown
| Region | Revenue     |
|--------|-------------|
| APAC   | 120000      |
| EMEA   | 98000       |
| Total  | =SUM(B2:B3) |
```

- An **unqualified** reference (`B2`, `$B$2`, `B2:B3`) is translated to the placed address (§9.2):
  each column is shifted by the table's first column and each row by *headerRow − 1*. `$` markers
  are kept. If this table is placed under a range title and a note (header at row 3), the formula is
  written as `=SUM(B4:B5)`. References inside string literals are not references and are not
  translated.
- A **sheet-qualified** reference (`Revenue!B5`, `'Q3 Data'!A1:B2`) is an absolute sheet address,
  even to the same sheet, and is written unchanged. Referencing another table therefore requires
  knowing its placed address (§9.2).
- An unqualified reference outside the table's own grid (columns A to the table's last column, rows
  1 to *1 + data rows*) produces `formula_ref_out_of_range` (warning), once per reference, and is
  still translated. A translation past the sheet limits produces `xlsx_limit_exceeded`.

**Written form.** The written formula is the source text with references translated; function names,
booleans and cell references in uppercase; and row numbers without leading zeros. Sheet names,
strings and whitespace are kept as written. `IFS`, `IFNA`, `XOR` and `CONCAT` are stored with Excel's
`_xlfn.` prefix, as Excel requires for functions newer than 2010.

### 9.4 Directives and attachment *(C8, C13, C39)*

| Directive | Effect |
|-----------|--------|
| `::: columns B:currency, Growth:percent` | Column types (§9.5) for every table it applies to |
| `::: anchor C5` | Places the table at this cell (`$` allowed, case-insensitive, within A1:XFD1048576) instead of stacking it |
| `::: table name=sales_q3, style=striped, autofilter` | Registers an Excel table (§9.6) |

All three are **annotations** (§1.5):
- `columns` applies to every table it wraps, or, when empty, to the next table in the same sheet.
- `anchor` and `table` apply to exactly one table: the first table they wrap, or, when empty, the
  next one in the same sheet. Each further wrapped table produces `directive_misplaced` at the
  directive's line.
- An annotation never crosses a sheet boundary: an H1 inside or after it ends its effect.
- When annotations of the same name are nested, only the innermost applies to the tables inside
  it. An enclosing one that applies to no table produces `directive_misplaced`. *(C47)*
- Wrapping is the canonical form:

```markdown
::: columns B:currency, C:percent
| Region | Revenue | Growth |
|--------|---------|--------|
| APAC   | 120000  | 14%    |
:::
```

**Attachment.** Annotations waiting for the next table (an H2 range title, a paragraph note, an empty
directive) attach to the next table in the same sheet, including a table inside a wrapping
annotation.
- An H2 or paragraph that is replaced by another of its kind before a table arrives, or is still
  waiting at the end of the sheet, produces `xlsx_ignored_content`.
- An empty directive in the same situation produces `directive_misplaced`.
- A wrapping annotation overrides a waiting empty one of the same name, which then produces
  `directive_misplaced`: it was replaced before its table arrived. *(C46)*
- An annotation that applies to no table produces only `directive_misplaced` (§1.5).

**`columns` entries** are `KEY:type` or `KEY:type(param)`. `KEY` is a header cell's text
(case-insensitive) if one matches, else a column letter (1–3 letters, A = the table's first column).

| Type | Param | Number format |
|------|-------|---------------|
| `number` | decimal places, 0–10 | `General`; with a param, `#,##0` plus that many decimals |
| `currency` | A three-letter code, ASCII letters in any case (default `USD`) | The currency symbol (`$` USD, `€` EUR, `£` GBP, `¥` JPY, `₹` INR; else the uppercase code and a space) before `#,##0.00` (`#,##0` for JPY) |
| `percent` | decimal places, 0–10 (default: just enough to show every value in the column as written, at most 10) | `0%`, plus that many decimals |
| `date` | `iso` (the default and only value) | `yyyy-mm-dd` |
| `text` | — | `@` |
| `boolean` | — | `General` |

- Currency codes aren't checked against a list, which changes over time: any three letters are a
  code, written in uppercase. *(C46)*
- Parameters follow §1.6. An unknown type, a param for a type that takes none (`text(x)`) or a `date`
  param other than `iso` produces `unknown_directive_option`. A malformed or out-of-range param
  (`number(12)`, `currency(EURO)`) produces `invalid_directive_args`. An unknown type ignores the
  entry; a param problem leaves the type with its default.
- A key matching no column also produces `unknown_directive_option`, and the entry is ignored. When
  the directive applies to several tables, a key is matched in each one, and is reported once, only
  if it matches no column in any of them.
- A malformed entry, or a column named twice in one directive, produces `invalid_directive_args`, once
  per offending entry.

### 9.5 Values *(C27, C39)*

**Literal tests** (ASCII only):
- **Number**: `-?[0-9]+(\.[0-9]+)?` with at most 15 significant digits and no leading zero before
  other digits (`007` and `02134` are text; `0.5` is a number). Significant digits run from the first
  non-zero digit to the last digit written, trailing zeros included, so `1000000000000000` is text.
  Long IDs and zero-padded codes stay intact.
- **Percent**: a number followed by `%`. Its value is the number divided by 100 (`14%` → 0.14).
- **Date**: `[0-9]{4}-[0-9]{2}-[0-9]{2}`, a real Gregorian date from 1900-03-01 to 9999-12-31, written
  as a date serial in the 1900 date system with no time zone involved. `2026-02-30` is text.
- **Boolean**: `true` or `false`, any case.

**Inference.** Without a `columns` entry, a column's type is inferred from the plain text of its data
cells, skipping formula cells and blank cells: all numbers → `number`; all percents → `percent`; all
dates → `date`; all booleans → `boolean`; all matching no test → `text`. Any other column is
**`general`**: its cells match different tests (plain text counting as one), or it has no literal
values at all. *(C46)*
- In a `general` column each literal converts with the test it matches and takes that type's default
  format (a percent with just enough decimals for its own value). Text stays text, and formula cells
  use `General`. A key/value table therefore keeps `42` a number and `4%` a percent. A mix is never
  forced into one type, because that would change a value (`42` → 4200%) or turn numbers into text.
- A column that is `general` because its cells matched different tests also produces
  `mixed_column_type` (warning, advisory) at the table's line. A mix in a column that should have one
  type is often a typo (`12O00`).
- `general` is inferred only. A `columns` entry can't name it.

**Conversion.** A literal in a typed column converts with its type's test. Numeric types accept
numbers, and percent also accepts percents. In a `percent` column a number is the fraction itself
(`0.14` shows as 14%). A value that fails is written as text, with
`typed_value_invalid` (warning) at its row's line. A column's number format also applies to its
formula cells. Markdown numeric literals always use `.` as the decimal separator with no grouping,
whatever `lang` is.

**Rich text and links.**
- A cell's bold, italic and strike runs become rich text. `code` runs are plain text.
- A cell whose runs carry one link destination gets that hyperlink. Further distinct links in the same
  cell produce `xlsx_ignored_content`, and only the first is kept.
- Range titles and notes are written to cells, so this rule and the cell text limit (§9.7) apply to
  them too.
- A cell with a hyperlink shows its text underlined, in the link color (§9.2), except in an Excel table
  (its header and data rows; a title or note above it isn't in it), where it keeps the table style's
  text color, which the style's fills contrast with. *(C55)* A cell
  whose text has a line break wraps its text. *(C48)*
- A number or boolean whose text is bold, italic or struck through keeps that emphasis as the cell's
  font. Rich text keeps the cell's own styling (a bold title, a link's underline) in every run.
- A text cell whose text is a decimal number, optionally signed and with an exponent (`007`, a
  `text` column's `42`), is text on purpose: it is marked so Excel doesn't flag it as a number stored
  as text. Other text Excel reads as a number (`1,000`, `$5`) stays flagged: it is text because this
  profile doesn't read it as a number (§15), and Excel's offer to convert it is the right one. *(C55)*
- Text reads back exactly as written, including text that looks like a SpreadsheetML character
  escape (`_x0041_` is written as `_x005F_x0041_`), in cells, sheet names and table names. *(C49)*
- Fragment links resolve against the slugs of the **repaired** sheet names (§9.6), and link to cell
  A1 of that sheet.

**Recalculation.** Formula results are not cached in the file, and the workbook requests full
recalculation when opened. Viewers that don't calculate (some previewers) show these cells empty.

### 9.6 Sheet names and Excel tables *(C14)*

**Sheet names** come from the heading's plain text, with line breaks turned into spaces and the
result trimmed. They are repaired, never rejected, by these rules in order:

| Problem | Repair | Code |
|---------|--------|------|
| Contains `: \ / ? * [ ]` | Each replaced with `-` | `sheet_name_invalid` |
| Starts or ends with `'` | Removed | `sheet_name_invalid` |
| Empty | `Sheet{n}` (n = its position) | `sheet_name_invalid` |
| Longer than 31 UTF-16 code units | Truncated to at most 31, never splitting a character (a surrogate pair) | `sheet_name_too_long` |
| Equal to an earlier name or to `History` (Unicode simple case folding) | ` (2)`, ` (3)`, … appended, truncating first to stay within 31 | `duplicate_sheet_name` |

A name that ends in `'` after truncation loses it too. All three codes are warnings and
strict-promotable. Each is reported at most once per sheet, at the heading's line (or at the `title`
key for a default sheet). Formulas and fragment links refer to sheets by their repaired names.

**Excel tables** (`::: table`):
- **Headers.** Excel requires header cells to be non-empty and unique (case-insensitive). An empty
  header becomes `Column{n}`, and a repeated one gets `2`, `3`, … appended, with `table_header_invalid`
  (warning) at the directive.
- **Name.** It must be a valid Excel table name: `[A-Za-z_\\][A-Za-z0-9_.]*`, at most 255 characters,
  not `TRUE` or `FALSE` (in any case), not shaped like a cell reference (one to three letters, then
  digits, inside the grid or not: `A1`, `XFE1`), and not `R`, `C` or an R1C1 reference
  (`R[0-9]*C[0-9]*`, `R[0-9]+`, `C[0-9]+`). *(C54)* It must also be unique in the workbook (case-insensitive). An invalid or repeated name
  produces `invalid_directive_args`, and the table gets an automatic name.
- **Automatic names** are `Table1`, `Table2`, … in workbook order, skipping names already used.
- **Style.** `striped` maps to `TableStyleMedium2` (the default), `plain` to `TableStyleLight1`. With
  theme tokens (§9.2) whose light 1 color contrasts with accent 1 by less than 4.5:1, `striped` maps
  to `TableStyleMedium1` instead, whose header pairs light 1 with dark 1, the theme's own text
  colors. *(C57)*
- **`autofilter`** shows filter buttons on the table's header row.
- **Header cells** keep their links and rich text when their text is already the column name; a
  repaired header is written as its new name. *(C49)*
- **Data rows.** An Excel table needs at least one: a table with none is placed with one empty data
  row (§9.2), which its range includes. *(C48)*

### 9.7 Validation

Diagnostics about a table row or cell report **that row's source line**, and cell addresses in
messages are placed addresses (§9.2). *(C15)* A cell's diagnostic carries the cell's path (§11.3):
`…/rows/r/c` for a data cell, and `…/header/c` for a header cell, whose line is the table's. *(C54)*

| Code | Severity | When |
|------|----------|------|
| `table_not_rectangular` | warning | §5 |
| `sheet_name_invalid` / `sheet_name_too_long` / `duplicate_sheet_name` | warning | §9.6 |
| `formula_error` | warning | §9.3 |
| `formula_ref_out_of_range` | warning | §9.3 |
| `typed_value_invalid` | warning | §9.5 |
| `mixed_column_type` | warning, advisory | §9.5. Reported at the table's line |
| `table_header_invalid` | warning | §9.6 |
| `xlsx_range_overlap` | warning | §9.2 |
| `xlsx_limit_exceeded` | error | Stacking past row 1,048,576 (at the first data row past it; at the table when the header, or a title or note above it, is past it, or when that row is the empty one an Excel table without data rows is given) or translation past the grid (§9.2, §9.3); text longer than 32,767 UTF-16 code units in a cell (data cells, range titles and notes); a formula longer than 8,192 UTF-16 code units as written in the cell or after translation; Excel's workbook limits, each reported once, at the cell that passes it when sheets and their ranges are counted in document order (each range's title, note, header row, then data rows, left to right): more than 65,530 hyperlinks on a sheet, and more than 200 custom number formats in the workbook (formats other than the built-in `General`, `0`, `0.00`, `#,##0`, `#,##0.00`, `0%`, `0.00%` and `@`; 200 is the fewest any language version of Excel holds) *(C54)* |
| `xlsx_empty_workbook` | warning, advisory | §9.1 |
| `xlsx_ignored_content` | warning | §9.1, §9.4, §9.5 |
| `link_anchor_unresolved` | warning | A fragment link matches no sheet name's slug |
| `invalid_directive_args` / `unknown_directive_option` | warning | §1.6, §9.4, §9.6 |
| `unknown_directive` / `directive_misplaced` | warning | §1.5, §9.4 |
| `xlsx_template_unsupported` | warning | An `.xlsx`/`.xltx` template was supplied (§10.4) |

---

## 10. Operations

### 10.1 The operations

Every host exposes the same four operations (architecture.md, "Operations"):

| Operation | Input | Output |
|-----------|-------|--------|
| `parse` | markdown | `{ ir, diagnostics }` (parse-stage diagnostics) |
| `lint` | markdown or IR, options | `{ diagnostics }` (parse and validate stages) |
| `convert` | markdown or IR, options | `{ bytes?, diagnostics }` (all stages); `bytes` absent when blocked (§11.1) |
| `inspect_template` | template bytes | a template report (`spec/template-report.schema.json`) |

An operation either returns its output, or fails with an **operation error** (§10.6), which is a
problem with the call rather than with the document. Diagnostics never cause an operation error, and
no input may make an operation crash or hang (§12.1).

### 10.2 Options

| Option | Type | Default | Meaning |
|--------|------|---------|---------|
| `format` | `docx` \| `pdf` \| `pptx` \| `xlsx` | `meta.format` | Target format. Precedence: this option, then `meta.format`, else the operation error `format_missing` |
| `strict` | boolean | `false` | Escalation (§11.1) |
| `template` | bytes | — | Template package. Precedence: this option, then `meta.template` through the template resolver, then the house template |
| `layoutAliases` | map: layout name → template layout name | `{}` | pptx layout binding (§8.5): each maps a name, a house-set name or a new one, to a layout of the supplied template. A target that names none of its layouts, or aliases with no template asked for, is `option_invalid`; when a template was asked for but can't be used, its own error is reported instead. Other formats ignore it *(C56)* |

### 10.3 Resolvers *(C41)*

The core performs no I/O (architecture.md D11). Hosts supply resolvers:

| Resolver | Called with | Returns |
|----------|-------------|---------|
| Asset | an image `src`, verbatim | the bytes, or *exists* (without bytes, permitted for `lint` only), or *not found* |
| Template | a `meta.template` reference, verbatim | the bytes, or *not found* |
| Font | a family, as the template names it, and a style (regular, bold, italic, bold italic) (§3) | the bytes of a font or a collection holding it, from the font cache or a user-supplied file, or *not found* |

- A host that supplies no asset resolver behaves as if every image source exists without bytes, in
  `lint` only. In `convert`, an image without bytes is unresolved (§5).
- A `meta.template` that can't be resolved (not found, or no template resolver) produces
  `template_unresolved` (error, at the `template` key's line). A brand template that can't be applied
  never silently becomes the house style.

**Host resolver requirements:**
- **Server and MCP hosts:** by default, resolvers touch neither the filesystem nor the network.
  Assets and templates come only from the request, or from roots configured explicitly by the
  operator.
- **Any configured root:** references resolve only inside it, after normalization. References with
  `..` segments, absolute paths, symlinks leaving the root, `file:` URLs and UNC paths are *not
  found*.
- **Remote fetching** is opt-in: `https:` only, with private, loopback and link-local addresses
  blocked, plus size and time limits.
- **The CLI** resolves relative references against the input file's directory.
- **The CLI's font resolver** *(C62)* reads the lockfile `compyla.lock`
  (`spec/lockfile.schema.json`): the one `--lockfile` names, else the first found in the input
  file's directory and then in each directory above it (for standard input, starting at the working
  directory). For a family and style the lockfile lists, it returns the cached file named by the
  pinned SHA-256; a file whose digest doesn't match is *not found*, so a lockfile pins bytes, not
  names. The cache is `$COMPYLA_CACHE/fonts` when that is set, else the user's cache directory:
  `$XDG_CACHE_HOME/compyla/fonts` (`~/.cache/compyla/fonts` when it is unset or not an absolute
  path) on Linux, `~/Library/Caches/compyla/fonts` on macOS and `%LOCALAPPDATA%\compyla\fonts` on
  Windows. Each file is named by its digest in lower-case hex, without an extension. `compyla fonts
  sync` writes the lockfile and fills the cache; the resolver never touches the network.
- **The lockfile** lists a family and style at most once, families compared by their ASCII-lowercase
  form (§3) and spelled as the template spells them. Entries are sorted by that form, by code point,
  then by style in the order regular, bold, italic, bold italic. A lockfile that can't be read,
  doesn't match its schema or lists a family and style twice stops the CLI with exit 2 (§10.8),
  whether `--lockfile` names it or it was found, and whatever the format: a pin is never skipped
  silently.
- In every host, the bytes a resolver returns are checked by content (§4.7), never trusted by
  extension.

### 10.4 Templates *(C41)*

| Format | Accepted templates |
|--------|--------------------|
| docx | `.docx`, `.dotx` |
| pdf | `.docx`, `.dotx` (page setup and theme tokens) or `.pptx`, `.potx` (theme tokens) |
| pptx | `.pptx`, `.potx` |
| xlsx | `.docx`, `.dotx`, `.pptx`, `.potx` (theme tokens, §9.2). `.xlsx`/`.xltx` produce `xlsx_template_unsupported` (warning) and are not used |

A template's kind is determined from its package's main content type, not its file name. These
produce `template_invalid` (error):
- a package that isn't a well-formed OOXML package of an accepted kind
- a macro-enabled package (`.docm`, `.dotm`, `.pptm`, `.potm`, or any package containing a VBA project
  or its data, or an Excel 4.0 macro sheet, found by part name, content type or relationship type;
  content types compare without case)
- an encrypted or password-protected package
- package XML containing a DTD (XML parts may be UTF-8 or UTF-16 and are checked as characters)
- a package that violates the limits in §12.1, or has entries with absolute paths or `..` segments
  (after percent-decoding; a segment of dots, or with trailing dots or spaces, counts as `..`), or
  whose central directory and local headers disagree about an entry's name

A template without a theme part, or whose theme part isn't an `a:theme`, produces `no_theme`
(warning; defaults are used). *(C57)* A docx document
whose template has no styles part produces `no_styles_xml` (warning; house styles are used).

**Template-first output** (architecture.md D12) keeps the template's styles, numbering, theme,
masters, layouts, headers and footers, and replaces its content. In addition:
- The template's own body content (docx) and slides (pptx) are removed, together with every part no
  longer referenced (such as sample-slide media). A kept part's relationships to those slides go
  too, with what refers to them, as for active content below (§8.8). *(C60)*
- Document properties (title, author, dates, application, company) are written from `meta` and fixed
  values (§13), never copied from the template.
- Active and external content is removed from every kept part *(C58)*:
  - each relationship with an external target, other than a hyperlink: remote attached templates,
    linked images, media and OLE objects. A target is external unless its relationship has no
    `TargetMode` or exactly `Internal`, and it stays in the package: no scheme, no leading `//`, no
    `\`, and no `..` past the package root once percent-decoded;
  - each hyperlink whose destination §4.5 doesn't allow (`file:`, `ms-msdt:`, UNC paths, …), and
    each hyperlink (`a:hlinkClick`, `a:hlinkHover`, `a:hlinkMouseOver`) whose `action` runs
    something instead of navigating: anything but `ppaction://` followed by `noaction`,
    `hlinkshowjump`, `hlinksldjump`, `hlinkpres`, `hlinkfile`, `customshow` or `media`, then
    nothing or a query (`?…`), compared without case. A
    relationship of the hyperlink type (its type compared exactly, Transitional or Strict) is a
    hyperlink only where hyperlinks refer to it: if anything else refers to it, it is external
    content like any other;
  - each OLE object, embedded or linked: in pptx, the graphic frame holding a `p:oleObj`;
  - each ActiveX control (`p:control`), and a `p:controls` left with nothing else in it;
  - each relationship of type `oleObject` or `control` that remains;
  - each Office add-in (a `webextension` relationship), which runs code from wherever its manifest
    names;
  - each `vmlDrawing` relationship: in a deck, legacy VML only draws OLE objects and ActiveX
    controls.

  What refers to a removed relationship goes too. A picture's `a:blip` loses the attribute that
  names it, and keeps an embedded image if it has one. Any other element is removed. An element is
  removed together with:
  - the `mc:AlternateContent` it is a branch of, since its other branches show the same content;
  - the extension (`p:ext`, `a:ext`) it is in, at any depth;
  - the graphic frame whose payload it is (a child of `a:graphicData`), since a graphic can't be
    empty;
  - the animations (`p:timing`) or transition (`p:transition`) it is part of.

  So are the animations of a removed shape. A relationship that only removed elements referred to is
  removed as well, and so is embedded data (`package`, `activeXControlBinary`) that nothing kept
  refers to. A part that only removed relationships reached isn't kept: an embedded object's file, a
  control's parts, a legacy drawing. Elements are read by namespace, Transitional and Strict alike.

  Each OLE object, ActiveX control, Office add-in and refused action produces one
  `template_content_removed` (warning, advisory, render stage, without a line). So does each other
  removed relationship of the first two kinds (an external `vmlDrawing` among them), or of type
  `oleObject` or `control`, that none of them refers to, such as a linked image. Like every code,
  these stop at the cap (§11.2). Every kept XML part is read, so an object, a control or an action
  is found however it is written; a part is XML by its name, its content type, or because it
  decodes, all of it, as UTF-8 or UTF-16 text whose first character past byte order marks and whitespace is `<`. A part with nothing to remove is kept byte for byte. A kept XML
  part that isn't well-formed (anything but whitespace, comments and processing instructions outside a single root element counts, a second byte order mark included), or whose root element would
  be removed, makes the render fail (`render_failed`), unless validation already refused the
  template for it (`template_invalid`).

  Text from a template (a target, a type, a part name, a parser's reason) appears in any message
  shortened, with its control and bidi formatting characters escaped.

### 10.5 `inspect_template`

Returns the template report (`spec/template-report.schema.json`):
- kind, theme fonts and colors, and referenced fonts (with whether each is embedded)
- for docx: styles and the final section's page setup (Word's defaults, Letter with 1-inch margins,
  where the template gives none)
- for pptx: slide size, and layouts in master order with their OOXML type and placeholders
- template-level issues: diagnostics without a line (`no_theme`, `no_styles_xml`, `template_invalid`)

An invalid template (§10.4) is reported, not refused: its report has `kind: null` and only its
`template_invalid` issue, whose message says why. *(C50)*

Agents use the report to write markdown against a template's real layouts and styles.

### 10.6 Operation errors

Operation errors have stable codes, registered under `operationErrors` in `spec/diagnostics.json`:

| Code | When |
|------|------|
| `input_not_utf8` | The input bytes aren't valid UTF-8 (host decoding) |
| `format_missing` | No format from the options or `meta.format` |
| `format_unknown` | The `format` option isn't one of the four |
| `renderer_unavailable` | This build doesn't include the requested renderer (e.g. the core-only WASM build) |
| `ir_invalid` | IR input fails `spec/ir.schema.json` or an IR invariant (§10.7); the error carries the JSON Pointer of the first failure |
| `ir_version_unsupported` | IR input has an `irVersion` this implementation doesn't support |
| `option_invalid` | An option's value can't be used: a `layoutAliases` entry (§10.2) *(C56)* |

### 10.7 IR input

`lint` and `convert` accept IR JSON in place of markdown. It must validate against
`spec/ir.schema.json` and satisfy these invariants:
- each table's `align` has as many entries as its `header`
- a section's child sections have greater levels than it
- `start` appears only on ordered lists

`position` is optional on every node; a line or column is at most 4,294,967,295. Diagnostics about a
node carry its `path` (§11.3), and its line when it has a position; a node without a position gives
no `line`. IR input goes through the same text rules as parsed text: §1.1's character replacement
in every string (`meta` values, run text and links, cell `raw`, media `src`, `alt` and `link`, code
`value` and `lang`, directive `args`), §4.4's canonical runs, §4.5's link allowlist, and §2's
normalization of known `meta` keys. The limits of §12.1 apply as they do to markdown. *(C47)*

An implementation supporting IR `X.Y` accepts `X.Z` for every `Z ≤ Y` and rejects anything else with
`ir_version_unsupported`. Each part of a version is `0` or digits without a leading zero (`0.1`,
never `00.1` or `+0.1`). Adding an optional field bumps the minor version.

### 10.8 Host conventions

| Host | Success | Blocked by error diagnostics | Operation error |
|------|---------|------------------------------|-----------------|
| CLI | exit 0 | exit 1 | exit 2 |
| HTTP | `200` | `422` | `400`; `501` for `renderer_unavailable`; `413` when the request exceeds the input size limit |
| MCP | tool result with diagnostics as structured content | the same, with `isError: true` | a tool error naming the code |

Every host returns the full diagnostics list in the shape of `spec/diagnostic.schema.json`.

---

## 11. Diagnostics *(C31, C32)*

Every diagnostic matches `spec/diagnostic.schema.json`: a registered `code`, a `severity` (`error` or
`warning`), an optional `line` and `column`, an optional `path` (JSON Pointer into the IR, e.g.
`/body/2/rows/0/3`), a `message` and an optional `hint`. Document-level diagnostics (template issues,
fonts, an empty workbook) have no `line`.

### 11.1 Severity policy and strict

- **Errors** are reserved for problems with **no safe or faithful output**:
  - `limit_exceeded`, `xlsx_limit_exceeded`
  - `template_unresolved`, `template_invalid`
  - `render_failed`, a catch-all for any other failure while rendering
- Every other problem is a **warning** with a defined fallback, so plain markdown always renders.
- A warning is either **advisory** (registered with `strictPromotable: false`), meaning a style
  observation where the output is still faithful (`heading_skip`, `mixed_column_type`), or
  **strict-promotable**, meaning the output differs from what the input asked for.
- Under `strict`, every strict-promotable warning becomes an error. So `strict` means: nothing was
  dropped, guessed or substituted.
- **Blocking.** `convert` returns no bytes if any diagnostic, from any stage, is an error after
  escalation.

### 11.2 Stages and order

- **Stages.** `parse` diagnostics come from §1–§4 and are format-independent. `validate` diagnostics
  come from a profile. `render` diagnostics come from composing and serializing. `lint` returns parse
  and validate diagnostics. `convert` returns all three.
- `layout_not_in_template` is validate-stage whenever a template is available, and `font_unresolved`
  always is (an operation without a font resolver resolves nothing through it, §3), so `lint` and
  `convert` agree. *(C62)*
- **Order.** Operations return diagnostics sorted by line (diagnostics without a line first), then
  column (absent first), then code, then path (absent first). Paths compare in document order:
  reference token by token, array indices numerically and other tokens as strings, and a path comes
  before the longer paths it is a prefix of (`/body/2` < `/body/2/children/0` < `/body/10`).
- **Cap.** An implementation MAY report at most 1,000 diagnostics, ending the list with
  `diagnostics_truncated` (warning, advisory). The cap applies after escalation and sorting, and
  keeps errors before warnings: the first 1,000 errors in order, then as many of the first warnings
  as fit, all in order. So when an error blocks `convert`, the list always shows one. *(C49)*

### 11.3 Positions

Each code's registry entry has a `position` naming where it points:

| `position` | Points at | Column |
|------------|-----------|--------|
| `node` | The IR node's position (§4.1) | no |
| `row` | The source line of the table row | no |
| `table` | The table's line | no |
| `key` | The frontmatter key's line | no |
| `frontmatter` | Line 1 | no |
| `underline` | A setext heading's underline line | no |
| `construct` | The construct's first character (`[`, `<`, `!`, or a block's first character) | yes |
| `char` | The replaced character | yes |
| `fence` | The fence line's first colon | yes |
| `none` | No line: a document-level diagnostic | no |

Every diagnostic SHOULD also carry the `path` of the IR node it concerns.

**Conformance comparison.** Expected and actual diagnostics are compared as lists of
`(code, severity, line, column)` in the order above, keeping duplicates. `line` is always compared
(absent must match absent). `column` and `path` are compared only when the expected diagnostic has
them.

## 12. Limits and Security

### 12.1 Limits

Every implementation MUST accept input up to these defaults. Hosts MAY configure other values, and
every host uses the same defaults. Input beyond the effective limit produces `limit_exceeded` (error)
at the first offending construct and is not processed further: no validate-stage diagnostic follows
it. The limits hold for IR input (§10.7) exactly as for markdown. The implementation MUST NOT crash,
hang or exhaust memory on any input, and its time and memory SHOULD grow linearly with the input; in
particular, it needs to keep only the diagnostics it may report (§11.2). *(C47)*

| Limit | Default |
|-------|---------|
| Input size | 10 MiB of UTF-8 |
| Nesting depth: one level for each blockquote, list, list item and directive body enclosing a node | 32 |
| Table cells per document; cells per row | 1,000,000; 16,384 |
| Slides; sheets | 10,000; 1,000 |
| Image size, encoded / decoded | 50 MiB / 100 megapixels |
| Template package, compressed / uncompressed / entries / compression ratio | 50 MiB / 256 MiB / 10,000 / 100:1 |
| Elements in a template XML part read as a tree (a docx body is searched, not read whole) | 500,000 |
| Font file, from a template or a resolver *(C62)* | 64 MiB |
| Faces in a font collection; tables in a face; character-map encoding records in a face *(C62)* | 256; 512; 64 |

A font over a limit, or bytes that don't parse, count as *not found* (§3), and resolution goes
on down the chain rather than the operation failing, since the output can still name the family.
Shaping work is bounded too, so a hostile layout table can't hang an operation; a run whose shaping
reaches the bound is measured as shaped so far.

### 12.2 Security

The engine processes untrusted input: LLM output, and anything submitted to the HTTP server.
- **No I/O in the core** (architecture.md D11), and confined resolvers in hosts (§10.3).
- **No code execution from documents.** Formulas are parsed and allowlisted (§9.3). Link destinations
  are allowlisted (§4.5). HTML is never rendered (§4.8). Templates are macro-free and stripped of
  active and external content (§10.4).
- **No injection into target formats.** Renderers MUST NOT let IR text be interpreted as markup or
  code in any target language. XML text is escaped. Text reaches Typst only as data (Typst inputs)
  or through an escape function proven by fixtures with hostile content, never spliced into markup.
- **Typst sandbox.** Typst's file access exposes only the generated document, resolved assets and
  fonts: no filesystem, no package downloads, and a fixed document date.
- **Bounded work** (§12.1). Template packages are checked for path traversal, decompression bombs and
  DTDs (§10.4).
- **No leaks.** Output metadata comes from `meta` and fixed values, never from the template, the
  host's environment, file paths or the current time (§13).

## 13. Determinism

- `parse` and `lint` are pure functions of their input, options and resolver results. `convert` is a
  pure function of its input, options, template bytes, the font bytes the resolver returns, and the
  asset bytes.
- **Reproducible bytes:** the same input, options, template, fonts, assets and implementation version
  MUST produce byte-identical output. Byte equality across implementations or versions is not
  required. Across implementations, the render suite compares semantic projections and package
  assertions, never bytes (spec/conformance/README.md); within one implementation it renders every
  fixture twice and requires identical bytes.
- Renderers MUST NOT read the clock, the environment, random sources or the locale. Document
  timestamps are fixed (`1980-01-01T00:00:00Z`), zip entries are ordered, and identifiers are
  assigned in document order.
- Operation outputs, including diagnostics (§11.2), are deterministically ordered.
- **Measurement** (§3) is a function of the text, the font bytes and the implementation version
  only: no installed fonts, no platform font APIs, integer arithmetic. The bundled stand-ins are
  fixed bytes of the implementation, so measurement, and with it `lint`, gives the same results in
  every host of one implementation version whose resolvers return the same bytes. *(C62)*

## 14. Versioning

- A document may pin the spec version it targets with `spec:` in frontmatter (§2).
- Before 1.0, any behavior change bumps the minor version (0.1 → 0.2). From 1.0, breaking changes
  bump the major version.
- The IR has its own version (`irVersion`, §10.7). IR 0.1 is the first published IR; the prototype's
  internal token model was never public. Adding an optional field is a minor change. Renaming or
  removing a field, or changing a field's meaning, is a breaking change.
- Diagnostic codes and operation error codes are public API. A code is never reused for a different
  meaning. Retired codes stay in the registry marked `retired`.
- Every behavior change ships with fixtures that encode it (spec/README.md).

## 15. Open Questions (candidates for 0.3)

- **Override blocks** (§3): scoped YAML inside `::: pptx` vs. attribute syntax (`{.two-col}`).
- **Callouts:** map GitHub alerts (`> [!NOTE]`, `> [!WARNING]`, …), which LLMs emit often, to a
  callout node rendered natively in each format.
- **Heading IDs:** Pandoc-style `## Intro {#intro}` as an explicit slug.
- **Math:** `$…$` / `$$…$$` → native equations (OMML in docx/pptx, Typst math in pdf).
- **Footnotes:** native footnotes in docx/pdf; pptx/xlsx have no equivalent.
- **Charts:** `::: chart bar` over a table → native editable charts in pptx/xlsx.
- **Subscript/superscript** (`<sub>`, `<sup>`) as marks, and **SVG** images.
- **Accessibility:** a warning for images without alt text; tagged PDF.
- **End positions** on IR nodes (useful for editor diagnostics).
- **Lenient typed values:** accept `$120,000` in currency columns.
- **Whole-column ranges** (`SUM(B:B)`) translated to the table's own rows.
- **Directive attributes:** whether to also accept `:::name{key=value}` (remark-directive style)
  alongside free-text args.
- **`toc` options** (depth, title), and cached formula values for simple aggregates.
