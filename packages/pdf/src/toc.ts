import type { BodyNode, InlineRun } from "@compyla/core";
import { walkBody } from "@compyla/core";
import type { PageLayout } from "./layout.js";

// Word's default TOC field depth is 3 levels; docx's own `::: toc` directive uses the same
// `headingStyleRange: "1-3"` — matching that convention here since pdf shares docx's authoring surface
// and markdown-spec.md §6 doesn't specify a depth of its own.
const TOC_MAX_LEVEL = 3;
const TOC_TITLE_SIZE = 18;
const TOC_TITLE_GAP = 24;
const TOC_ENTRY_HEIGHT = 20;
const TOC_ENTRY_SIZE = 11;
const TOC_INDENT_PER_LEVEL = 14;

export interface TocEntry {
  readonly level: number;
  readonly title: string;
  pageNumber: number;
}

export interface TocPlaceholder {
  readonly startPageIndex: number;
  readonly pageCount: number;
}

function plainText(runs: readonly InlineRun[]): string {
  return runs.map((r) => r.text).join("");
}

/** Depth-first, matching document order exactly — the real render pass walks the same tree the same
 *  way, so entries are consumed by a simple position cursor rather than any identity/title matching. */
export function collectTocEntries(body: readonly BodyNode[]): TocEntry[] {
  const entries: TocEntry[] = [];
  walkBody(body, 0, {
    onSection(section) {
      if (section.level <= TOC_MAX_LEVEL) {
        entries.push({ level: section.level, title: plainText(section.title), pageNumber: 1 });
      }
    },
  });
  return entries;
}

/** How many blank pages a `::: toc` needs, given how many entries it will end up listing. */
export function tocPageCount(entryCount: number, layout: PageLayout): number {
  const neededHeight = TOC_TITLE_SIZE + TOC_TITLE_GAP + entryCount * TOC_ENTRY_HEIGHT;
  return Math.max(1, Math.ceil(neededHeight / layout.contentHeight));
}

/** Builds a dotted-leader TOC line ("Title ..... 3") sized to fill exactly `width` points, by growing
 *  the leader one character at a time until it would overflow — simple and always correct for the
 *  handful of entries a document TOC has; not meant for large-N performance. */
function buildLeaderLine(pdf: PDFKit.PDFDocument, title: string, pageNumber: number, width: number): string {
  const suffix = ` ${pageNumber}`;
  let line = `${title} ${suffix}`;
  if (pdf.widthOfString(line) >= width) return line;

  let dots = "";
  for (;;) {
    const candidate = `${title} ${dots}.${suffix}`;
    if (pdf.widthOfString(candidate) >= width) break;
    dots += ".";
  }
  return `${title} ${dots}${suffix}`;
}

export function drawTocBackfill(
  pdf: PDFKit.PDFDocument,
  placeholders: readonly TocPlaceholder[],
  entries: readonly TocEntry[],
  layout: PageLayout,
): void {
  for (const placeholder of placeholders) {
    let pageOffset = 0;
    pdf.switchToPage(placeholder.startPageIndex);
    pdf.x = layout.marginPt;
    pdf.y = layout.marginPt;

    pdf.font("Helvetica-Bold", TOC_TITLE_SIZE).fillColor("black").text("Table of Contents", { width: layout.contentWidth });
    pdf.y += TOC_TITLE_GAP - TOC_TITLE_SIZE;

    for (const entry of entries) {
      if (pdf.y + TOC_ENTRY_HEIGHT > layout.marginPt + layout.contentHeight) {
        pageOffset += 1;
        pdf.switchToPage(placeholder.startPageIndex + pageOffset);
        pdf.x = layout.marginPt;
        pdf.y = layout.marginPt;
      }

      const indent = (entry.level - 1) * TOC_INDENT_PER_LEVEL;
      pdf.x = layout.marginPt + indent;
      const width = layout.contentWidth - indent;
      const line = buildLeaderLine(pdf, entry.title, entry.pageNumber, width);
      pdf.font(entry.level === 1 ? "Helvetica-Bold" : "Helvetica", TOC_ENTRY_SIZE).text(line, { width });
    }
  }
}
