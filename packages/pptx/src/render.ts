import type {
  BodyNode,
  CompylaDocument,
  Diagnostic,
  DirectiveNode,
  MediaNode,
  PptxLayoutName,
  QuoteNode,
  Slide as CoreSlide,
  TableNode,
} from "@compyla/core";
import { groupSlides, resolveSlideContent, resolvePptxLayoutName } from "@compyla/core";
import { resolveFonts } from "@compyla/fonts";
import { repackageDeterministically } from "@compyla/ooxml";
import { imageSize } from "image-size";
// pptxgenjs is plain CJS at runtime (`module.exports = class PptxGenJS {...}`, verified: no
// `.default` property, no `__esModule` marker), but its .d.ts models it as an ESM `export
// default class` with a same-named `declare namespace` merge for its option types
// (PptxGenJS.Slide, PptxGenJS.TableRow, ...). Under this project's NodeNext module resolution
// that merge isn't reachable from an importing package no matter how the import is written (the
// default import, a namespace import's `.default`, and a type-only default import were all
// tried; each resolves to the whole module namespace, not the class). Every option shape below
// was verified empirically against real pptxgenjs output earlier in this session, so rather than
// keep fighting the type resolution, minimal local interfaces stand in for the library's own
// types, with a single boundary cast where the class is constructed.
import PptxGenJSCtor from "pptxgenjs";
import { inferLayout, SLIDE_LAYOUTS, splitColumns, type PlaceholderSpec } from "./layout.js";
import { flattenPlainText, flowingRichText, plainText, toRichText, type TextRunItem } from "./rich-text.js";
import { defaultTemplateReport, introspectTemplate, type TemplateReport } from "./template.js";

type PptxTableRow = readonly TextRunItem[];

interface PptxSlide {
  addText(text: string | readonly TextRunItem[], options?: Record<string, unknown>): PptxSlide;
  addTable(rows: readonly PptxTableRow[], options?: Record<string, unknown>): PptxSlide;
  addImage(options: Record<string, unknown>): PptxSlide;
  addNotes(notes: string): PptxSlide;
}

interface PptxGenJSInstance {
  author: string;
  title: string;
  defineSlideMaster(props: Record<string, unknown>): void;
  addSlide(props?: Record<string, unknown>): PptxSlide;
  write(props?: Record<string, unknown>): Promise<unknown>;
}

const PptxGenJS = PptxGenJSCtor as unknown as new () => PptxGenJSInstance;

export interface RenderOptions {
  readonly template?: Buffer;
  readonly resolveAsset?: (src: string) => Promise<Buffer> | Buffer;
  readonly userFonts?: ReadonlyMap<string, Buffer>;
  readonly fetchImpl?: typeof fetch;
}

export interface RenderResult {
  readonly buffer: Buffer;
  readonly diagnostics: readonly Diagnostic[];
}

interface RenderContext {
  readonly resolveAsset?: (src: string) => Promise<Buffer> | Buffer;
}

export async function renderPptx(doc: CompylaDocument, options: RenderOptions = {}): Promise<RenderResult> {
  const templateReport = options.template ? await introspectTemplate(options.template) : defaultTemplateReport();
  const diagnostics: Diagnostic[] = templateReport.issues.map((issue) => ({
    severity: "warning",
    code: issue.code,
    line: 1,
    message: issue.message,
  }));

  const headingFont = templateReport.theme.fonts[0];
  const bodyFont = templateReport.theme.fonts[1] ?? headingFont;
  const accentColor = templateReport.theme.colors[4] ?? templateReport.theme.colors[0];
  diagnostics.push(...(await validateThemeFonts([headingFont, bodyFont], options)));

  const pptx = new PptxGenJS();
  if (doc.meta.author) pptx.author = String(doc.meta.author);
  if (doc.meta.title) pptx.title = String(doc.meta.title);

  defineHouseMasters(pptx, { headingFont, bodyFont, accentColor });

  const ctx: RenderContext = { resolveAsset: options.resolveAsset };
  const slides = groupSlides(doc.body);
  for (const slide of slides) {
    await renderSlide(pptx, slide, slides.length, ctx);
  }

  const raw = (await pptx.write({ outputType: "nodebuffer" })) as unknown as Buffer;
  const buffer = await repackageDeterministically(raw);

  return { buffer, diagnostics };
}

async function validateThemeFonts(families: (string | undefined)[], options: RenderOptions): Promise<Diagnostic[]> {
  const wanted = [...new Set(families.filter((f): f is string => !!f))];
  if (wanted.length === 0) return [];

  const resolved = await resolveFonts(wanted, { userFonts: options.userFonts, fetchImpl: options.fetchImpl });
  const diagnostics: Diagnostic[] = [];
  for (const family of wanted) {
    if (resolved.get(family)?.source === "unresolved") {
      diagnostics.push({
        severity: "warning",
        code: "font_unresolved",
        line: 1,
        message: `Font "${family}" from the template's theme could not be resolved on Google Fonts. pptx fonts are set by name, not embedded — PowerPoint substitutes a fallback if this family isn't installed on the viewer's machine.`,
        hint: "Use a Google Fonts family name for full portability.",
        strictPromotable: true,
      });
    }
  }
  return diagnostics;
}

function defineHouseMasters(
  pptx: PptxGenJSInstance,
  theme: { headingFont?: string; bodyFont?: string; accentColor?: string },
): void {
  for (const layoutName of Object.keys(SLIDE_LAYOUTS) as PptxLayoutName[]) {
    const placeholders = SLIDE_LAYOUTS[layoutName];
    pptx.defineSlideMaster({
      title: layoutName,
      objects: placeholders.map((p) => ({
        placeholder: {
          options: {
            name: p.name,
            type: p.type,
            x: p.x,
            y: p.y,
            w: p.w,
            h: p.h,
            fontFace: p.type === "title" ? theme.headingFont : theme.bodyFont,
            fontSize: p.fontSize,
            bold: p.bold,
            italic: p.italic,
            align: p.align,
            color: p.type === "title" ? theme.accentColor : undefined,
          },
          text: "",
        },
      })),
    });
  }
}

async function renderSlide(pptx: PptxGenJSInstance, slide: CoreSlide, totalSlides: number, ctx: RenderContext): Promise<void> {
  const { layoutDirective, notesDirective, content } = resolveSlideContent(slide);
  const layoutName = resolveLayoutName(slide.index, totalSlides, layoutDirective, content);

  const pptxSlide = pptx.addSlide({ masterName: layoutName });

  if (slide.title) pptxSlide.addText(toRichText(slide.title), { placeholder: "title" });

  await fillLayoutContent(pptxSlide, layoutName, content, ctx);

  if (notesDirective) {
    const notesText = flattenPlainText(notesDirective.children);
    if (notesText) pptxSlide.addNotes(notesText);
  }
}

function resolveLayoutName(
  slideIndex: number,
  totalSlides: number,
  layoutDirective: DirectiveNode | undefined,
  content: readonly BodyNode[],
): PptxLayoutName {
  if (layoutDirective) {
    const resolved = resolvePptxLayoutName(layoutDirective.argsRaw);
    // A layout name that doesn't resolve is already an `error` from validate(); render() must
    // still produce something reasonable if called standalone (e.g. tests, or a --strict-off run).
    if (resolved) return resolved;
  }
  // "Content before the first ---" (markdown-spec.md §6.1) implies a --- actually exists — a
  // single-slide document (no --- at all) is just one ordinary slide, not a title slide, so its
  // content (which may not be plain text — an image, a table, ...) isn't lost to subtitle-only handling.
  if (slideIndex === 0 && totalSlides > 1) return "Title Slide";
  return inferLayout(content);
}

async function fillLayoutContent(
  pptxSlide: PptxSlide,
  layoutName: PptxLayoutName,
  content: readonly BodyNode[],
  ctx: RenderContext,
): Promise<void> {
  switch (layoutName) {
    case "Section Header":
      return;
    case "Title Slide":
      await fillPlaceholder(pptxSlide, layoutName, "subtitle", content, ctx);
      return;
    case "Two Content": {
      const [left = [], right = []] = splitColumns(content);
      await fillPlaceholder(pptxSlide, layoutName, "body1", left, ctx);
      await fillPlaceholder(pptxSlide, layoutName, "body2", right, ctx);
      return;
    }
    case "Picture with Caption": {
      const media = content.find((n): n is MediaNode => n.type === "media");
      if (media) await addImageToPlaceholder(pptxSlide, layoutName, "pic", media, ctx);
      const captionText = flattenPlainText(content.filter((n) => n !== media));
      if (captionText) pptxSlide.addText(captionText, { placeholder: "caption" });
      return;
    }
    default:
      await fillPlaceholder(pptxSlide, layoutName, "body", content, ctx);
  }
}

type RenderBlock =
  | { readonly kind: "table"; readonly node: TableNode }
  | { readonly kind: "media"; readonly node: MediaNode }
  | { readonly kind: "quote"; readonly node: QuoteNode }
  | { readonly kind: "flow"; readonly nodes: readonly BodyNode[] };

function groupIntoRenderBlocks(nodes: readonly BodyNode[]): RenderBlock[] {
  const blocks: RenderBlock[] = [];
  let flow: BodyNode[] = [];
  const flush = (): void => {
    if (flow.length > 0) {
      blocks.push({ kind: "flow", nodes: flow });
      flow = [];
    }
  };

  for (const node of nodes) {
    if (node.type === "directive" && node.name === "col") continue; // already split before this point
    if (node.type === "table") {
      flush();
      blocks.push({ kind: "table", node });
    } else if (node.type === "media") {
      flush();
      blocks.push({ kind: "media", node });
    } else if (node.type === "quote") {
      flush();
      blocks.push({ kind: "quote", node });
    } else {
      flow.push(node);
    }
  }
  flush();
  return blocks;
}

/**
 * Fills one placeholder with a run of blocks. The common case — validated by the core pptx
 * profile's overflow check — is exactly one block. An unheeded overflow warning means multiple
 * blocks target the same placeholder position and will visually overlap; that degraded fallback
 * is a documented, disclosed limitation (markdown-spec.md §7.5's "flows into the last
 * placeholder"), not a crash.
 */
async function fillPlaceholder(
  pptxSlide: PptxSlide,
  layoutName: PptxLayoutName,
  placeholderName: string,
  nodes: readonly BodyNode[],
  ctx: RenderContext,
): Promise<void> {
  for (const block of groupIntoRenderBlocks(nodes)) {
    if (block.kind === "table") {
      pptxSlide.addTable(toTableRows(block.node), { placeholder: placeholderName });
    } else if (block.kind === "media") {
      await addImageToPlaceholder(pptxSlide, layoutName, placeholderName, block.node, ctx);
    } else if (block.kind === "quote") {
      pptxSlide.addText(toRichText(flattenQuoteRuns(block.node), { forceItalic: true }), { placeholder: placeholderName });
    } else {
      pptxSlide.addText(flowingRichText(block.nodes), { placeholder: placeholderName });
    }
  }
}

function flattenQuoteRuns(quote: QuoteNode) {
  return quote.children.filter((n): n is Extract<BodyNode, { type: "paragraph" }> => n.type === "paragraph").flatMap((p) => p.runs);
}

function toTableRows(table: TableNode): PptxTableRow[] {
  const headerRow: PptxTableRow = table.header.map((cell) => ({
    text: plainText(cell.runs),
    options: { bold: true, fill: { color: "D9E2F3" } },
  }));
  const bodyRows: PptxTableRow[] = table.rows.map((row) => row.map((cell) => ({ text: plainText(cell.runs) })));
  return [headerRow, ...bodyRows];
}

async function addImageToPlaceholder(
  pptxSlide: PptxSlide,
  layoutName: PptxLayoutName,
  placeholderName: string,
  media: MediaNode,
  ctx: RenderContext,
): Promise<void> {
  if (!ctx.resolveAsset) {
    throw new Error(`Cannot render image "${media.src}": no asset resolver was provided to renderPptx().`);
  }
  const data = await ctx.resolveAsset(media.src);
  const dims = imageSize(data);
  if (!dims.width || !dims.height) {
    throw new Error(`Could not determine dimensions for image "${media.src}".`);
  }

  const box = SLIDE_LAYOUTS[layoutName].find((p) => p.name === placeholderName);
  if (!box) {
    throw new Error(`No placeholder "${placeholderName}" defined for layout "${layoutName}".`);
  }

  const { x, y, w, h } = fitWithinBox(box, dims.width, dims.height);
  const mime = mimeTypeFor(dims.type, media.src);
  pptxSlide.addImage({ data: `${mime};base64,${data.toString("base64")}`, x, y, w, h });
}

/** Contain-fits an image's intrinsic aspect ratio inside a placeholder's box, centered. */
function fitWithinBox(
  box: PlaceholderSpec,
  intrinsicWidth: number,
  intrinsicHeight: number,
): { x: number; y: number; w: number; h: number } {
  const boxAspect = box.w / box.h;
  const imageAspect = intrinsicWidth / intrinsicHeight;

  let w: number;
  let h: number;
  if (imageAspect > boxAspect) {
    w = box.w;
    h = box.w / imageAspect;
  } else {
    h = box.h;
    w = box.h * imageAspect;
  }

  return { x: box.x + (box.w - w) / 2, y: box.y + (box.h - h) / 2, w, h };
}

function mimeTypeFor(type: string | undefined, src: string): string {
  switch (type) {
    case "png":
      return "image/png";
    case "jpg":
    case "jpeg":
      return "image/jpeg";
    case "gif":
      return "image/gif";
    case "bmp":
      return "image/bmp";
    default:
      throw new Error(`Unsupported image type "${type}" for "${src}" (pptx supports png, jpg, gif, bmp).`);
  }
}

// Re-exported for callers that want to build rich-text runs themselves (e.g. tests).
export type { TextRunItem };
