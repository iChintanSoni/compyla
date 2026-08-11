import type {
  BodyNode,
  CompylaDocument,
  DirectiveNode,
  Diagnostic,
  InlineRun,
  ListNode,
  MediaNode,
  QuoteNode,
  SectionNode,
} from "@compyla/core";
import PDFDocument from "pdfkit";
import { resolvePageLayout, type PageLayout } from "./layout.js";
import { renderTable } from "./table.js";
import { collectTocEntries, drawTocBackfill, tocPageCount, type TocEntry, type TocPlaceholder } from "./toc.js";

export interface RenderOptions {
  // Accepted for interface parity with the other renderers' RendererModule contract — pdf has no
  // template-binding concept (markdown-spec.md §6), so this is currently unused.
  readonly template?: Buffer;
  readonly resolveAsset?: (src: string) => Promise<Buffer> | Buffer;
}

export interface RenderResult {
  readonly buffer: Buffer;
  readonly diagnostics: readonly Diagnostic[];
}

// Pinned like `@compyla/ooxml`'s `repackageDeterministically` — identical markdown -> identical bytes.
const PINNED_DATE = new Date(Date.UTC(2000, 0, 1));

const HEADING_SIZES: Record<1 | 2 | 3 | 4 | 5 | 6, number> = { 1: 24, 2: 20, 3: 16, 4: 13, 5: 12, 6: 11 };
const BODY_SIZE = 11;
const CODE_SIZE = 9;
const LINK_COLOR = "#1a5fb4";

interface RenderContext {
  readonly pdf: PDFKit.PDFDocument;
  readonly layout: PageLayout;
  readonly resolveAsset?: (src: string) => Promise<Buffer> | Buffer;
  readonly tocEntries: TocEntry[];
  readonly tocPlaceholders: TocPlaceholder[];
  tocCursor: number;
  readonly inQuote?: boolean;
}

export async function renderPdf(doc: CompylaDocument, options: RenderOptions = {}): Promise<RenderResult> {
  const diagnostics: Diagnostic[] = [];
  const layout = resolvePageLayout(doc.meta);

  const pdf = new PDFDocument({
    size: layout.size,
    margins: { top: layout.marginPt, bottom: layout.marginPt, left: layout.marginPt, right: layout.marginPt },
    bufferPages: true,
    autoFirstPage: false,
    // Undefined values must not become own properties here: pdfkit's file-ID hashing iterates every
    // key in `info` and calls `.valueOf()` on it, which throws on an explicit `undefined`.
    info: {
      CreationDate: PINNED_DATE,
      Producer: "compyla",
      Creator: "compyla",
      ...(doc.meta.title ? { Title: doc.meta.title } : {}),
      ...(doc.meta.author ? { Author: doc.meta.author } : {}),
    },
  });

  const chunks: Buffer[] = [];
  pdf.on("data", (chunk: Buffer) => chunks.push(chunk));
  const done = new Promise<Buffer>((resolve) => pdf.on("end", () => resolve(Buffer.concat(chunks))));

  pdf.addPage();

  const ctx: RenderContext = {
    pdf,
    layout,
    resolveAsset: options.resolveAsset,
    tocEntries: collectTocEntries(doc.body),
    tocPlaceholders: [],
    tocCursor: 0,
  };

  await renderBody(doc.body, ctx);

  if (ctx.tocPlaceholders.length > 0) {
    drawTocBackfill(pdf, ctx.tocPlaceholders, ctx.tocEntries, layout);
  }

  pdf.flushPages();
  pdf.end();

  const buffer = await done;
  return { buffer, diagnostics };
}

function ensureSpace(ctx: RenderContext, minHeight: number): void {
  const bottom = ctx.layout.marginPt + ctx.layout.contentHeight;
  if (ctx.pdf.y + minHeight > bottom) {
    ctx.pdf.addPage();
  }
  ctx.pdf.x = ctx.layout.marginPt;
}

async function renderBody(nodes: readonly BodyNode[], ctx: RenderContext): Promise<void> {
  for (const node of nodes) {
    switch (node.type) {
      case "section":
        await drawSection(node, ctx);
        break;
      case "paragraph":
        ensureSpace(ctx, BODY_SIZE * 2);
        drawRuns(ctx, node.runs, {
          width: ctx.layout.contentWidth,
          size: BODY_SIZE,
          forceItalic: ctx.inQuote,
        });
        ctx.pdf.moveDown(0.6);
        break;
      case "table":
        ensureSpace(ctx, BODY_SIZE * 3);
        renderTable(ctx.pdf, node, ctx.layout);
        ctx.pdf.moveDown(0.6);
        break;
      case "list":
        drawList(node, ctx, 0);
        ctx.pdf.moveDown(0.4);
        break;
      case "media":
        await drawMedia(node, ctx);
        break;
      case "code":
        drawCode(node.value, ctx);
        break;
      case "quote":
        await drawQuote(node, ctx);
        break;
      case "break":
        ctx.pdf.addPage();
        break;
      case "directive":
        await drawDirective(node, ctx);
        break;
      default:
        break;
    }
  }
}

async function drawSection(node: SectionNode, ctx: RenderContext): Promise<void> {
  const size = HEADING_SIZES[node.level];
  ensureSpace(ctx, size * 3);

  const isTocEntry = node.level <= 3;
  const entry = isTocEntry ? ctx.tocEntries[ctx.tocCursor++] : undefined;
  if (entry) entry.pageNumber = ctx.pdf.bufferedPageRange().count;

  drawRuns(ctx, node.title, { width: ctx.layout.contentWidth, size, forceBold: true });
  ctx.pdf.moveDown(0.5);

  await renderBody(node.children, ctx);
}

interface DrawRunsOptions {
  readonly width: number;
  readonly size: number;
  readonly forceBold?: boolean;
  readonly forceItalic?: boolean;
  readonly align?: "left" | "center" | "right" | "justify";
}

function pickFont(bold: boolean, italic: boolean): string {
  if (bold && italic) return "Helvetica-BoldOblique";
  if (bold) return "Helvetica-Bold";
  if (italic) return "Helvetica-Oblique";
  return "Helvetica";
}

function drawRuns(ctx: RenderContext, runs: readonly InlineRun[], opts: DrawRunsOptions): void {
  const { pdf } = ctx;
  if (runs.length === 0) {
    pdf.font("Helvetica", opts.size).text(" ", { width: opts.width, align: opts.align });
    return;
  }

  runs.forEach((run, i) => {
    const bold = !!opts.forceBold || !!run.bold;
    const italic = !!opts.forceItalic || !!run.italic;
    const font = run.code ? "Courier" : pickFont(bold, italic);
    pdf.font(font, opts.size).fillColor(run.link ? LINK_COLOR : "black");

    const textOptions: PDFKit.Mixins.TextOptions = {
      continued: i < runs.length - 1,
      underline: !!run.link,
      strike: !!run.strike,
      link: run.link,
    };
    if (i === 0) {
      textOptions.width = opts.width;
      textOptions.align = opts.align;
    }
    pdf.text(run.text, textOptions);
  });
}

function drawList(node: ListNode, ctx: RenderContext, level: number): void {
  const indent = level * 16;
  node.items.forEach((item, index) => {
    let prefix: string;
    if (item.checked !== null) {
      // Standard PDF fonts only cover WinAnsiEncoding — no embedded font for M1 (see plan), so
      // checkboxes are literal ASCII rather than docx's ☐/☒ glyphs.
      prefix = item.checked ? "[x] " : "[ ] ";
    } else if (node.ordered) {
      prefix = `${index + 1}. `;
    } else {
      prefix = "• "; // bullet — present in WinAnsiEncoding/CP1252
    }

    const [firstChild, ...rest] = item.children;
    const isTextItem = firstChild?.type === "paragraph";
    const runs: InlineRun[] = [{ text: prefix }, ...(isTextItem ? firstChild.runs : [])];

    ensureSpace(ctx, BODY_SIZE * 2);
    ctx.pdf.x = ctx.layout.marginPt + indent;
    drawRuns(ctx, runs, { width: ctx.layout.contentWidth - indent, size: BODY_SIZE });
    ctx.pdf.moveDown(0.3);

    const remainingChildren = isTextItem ? rest : item.children;
    for (const child of remainingChildren) {
      if (child.type === "list") drawList(child, ctx, level + 1);
    }
  });
}

async function drawQuote(node: QuoteNode, ctx: RenderContext): Promise<void> {
  const indent = 18;
  const barX = ctx.layout.marginPt + 4;
  const startY = ctx.pdf.y;

  ctx.pdf.x = ctx.layout.marginPt + indent;
  const savedWidth = ctx.layout.contentWidth - indent;
  await renderBody(node.children, { ...ctx, inQuote: true, layout: { ...ctx.layout, contentWidth: savedWidth } });

  ctx.pdf
    .save()
    .lineWidth(2)
    .strokeColor("#CCCCCC")
    .moveTo(barX, startY)
    .lineTo(barX, ctx.pdf.y)
    .stroke()
    .restore();
  ctx.pdf.x = ctx.layout.marginPt;
}

function drawCode(value: string, ctx: RenderContext): void {
  const lines = value.length > 0 ? value.split("\n") : [""];
  const { pdf, layout } = ctx;
  const lineHeight = CODE_SIZE * 1.4;
  const padding = 6;
  const totalHeight = lines.length * lineHeight + padding * 2;

  ensureSpace(ctx, totalHeight);
  const top = pdf.y;
  pdf.rect(layout.marginPt, top, layout.contentWidth, totalHeight).fill("#F2F2F2");

  pdf.font("Courier", CODE_SIZE).fillColor("black");
  let y = top + padding;
  for (const line of lines) {
    pdf.text(line.length > 0 ? line : " ", layout.marginPt + padding, y, {
      width: layout.contentWidth - padding * 2,
      lineBreak: false,
    });
    y += lineHeight;
  }

  pdf.x = layout.marginPt;
  pdf.y = top + totalHeight;
  pdf.moveDown(0.4);
}

async function drawMedia(node: MediaNode, ctx: RenderContext, widthPercentOverride?: number): Promise<void> {
  if (!ctx.resolveAsset) {
    throw new Error(`Cannot render image "${node.src}": no asset resolver was provided to renderPdf().`);
  }
  const data = await ctx.resolveAsset(node.src);
  const percent = widthPercentOverride ?? node.widthPercent ?? 100;
  const width = ctx.layout.contentWidth * (percent / 100);

  ensureSpace(ctx, 1); // images always start a fresh page position; exact height is unknown until drawn
  ctx.pdf.image(data, ctx.layout.marginPt, ctx.pdf.y, { width });
  ctx.pdf.moveDown(1);
}

async function drawDirective(node: DirectiveNode, ctx: RenderContext): Promise<void> {
  switch (node.name) {
    case "toc": {
      const pageCount = tocPageCount(ctx.tocEntries.length, ctx.layout);
      const startPageIndex = ctx.pdf.bufferedPageRange().count;
      for (let i = 0; i < pageCount; i++) ctx.pdf.addPage();
      ctx.tocPlaceholders.push({ startPageIndex, pageCount });
      // The reserved placeholder page(s) must stay blank until the backfill pass at the very end —
      // move onto a fresh page so subsequent real content doesn't get drawn into the same content
      // stream as the (not-yet-drawn) TOC entries.
      ctx.pdf.addPage();
      break;
    }
    case "pagebreak":
      ctx.pdf.addPage();
      break;
    case "image": {
      const media = node.children.find((c): c is MediaNode => c.type === "media");
      if (!media) break;
      const widthArg = node.args.find((a) => a.startsWith("width="));
      const percent = widthArg ? Number(widthArg.slice("width=".length).replace("%", "")) : undefined;
      await drawMedia(media, ctx, percent);
      break;
    }
    default:
      // Unknown directives refine, never gatekeep — render children as if the directive were
      // absent (the validator already warns separately).
      await renderBody(node.children, ctx);
      break;
  }
}
