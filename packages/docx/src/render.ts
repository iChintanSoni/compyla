import type { Diagnostic, BodyNode, CompylaDocument, DirectiveNode, InlineRun, ListNode, MediaNode, TableNode } from "@compyla/core";
import {
  Document,
  ExternalHyperlink,
  type FileChild,
  HeadingLevel,
  ImageRun,
  PageBreak,
  Packer,
  Paragraph,
  Table,
  TableCell,
  TableOfContents,
  TableRow,
  TextRun,
  WidthType,
} from "docx";
import { imageSize } from "image-size";
import { resolveFonts, type ResolvedFont } from "./fonts.js";
import { repackageDeterministically } from "./repackage.js";
import { defaultTemplateReport, introspectTemplate, type TemplateReport } from "./template.js";

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

// Letter page, 1in margins, 96dpi — the same assumption the `docx` library's ImageRun pixel
// sizing implies.
const PAGE_CONTENT_WIDTH_PX = 624;
const HEADING_LEVELS = {
  1: HeadingLevel.HEADING_1,
  2: HeadingLevel.HEADING_2,
  3: HeadingLevel.HEADING_3,
  4: HeadingLevel.HEADING_4,
  5: HeadingLevel.HEADING_5,
  6: HeadingLevel.HEADING_6,
} as const;

interface RenderContext {
  readonly resolveAsset?: (src: string) => Promise<Buffer> | Buffer;
  readonly inQuote?: boolean;
}

export async function renderDocx(doc: CompylaDocument, options: RenderOptions = {}): Promise<RenderResult> {
  const templateReport = options.template ? await introspectTemplate(options.template) : defaultTemplateReport();
  const diagnostics: Diagnostic[] = templateReport.issues.map((issue) => ({
    severity: "warning",
    code: issue.code,
    line: 1,
    message: issue.message,
  }));

  const { headingFont, bodyFont, accentColor } = pickThemeDefaults(templateReport);
  const { embeddableFonts, fontDiagnostics } = await resolveTemplateFonts(
    [headingFont, bodyFont],
    options,
  );
  diagnostics.push(...fontDiagnostics);

  const ctx: RenderContext = { resolveAsset: options.resolveAsset };
  const children = await renderBody(doc.body, ctx);

  const document = new Document({
    title: doc.meta.title,
    creator: doc.meta.author ?? "Compyla",
    fonts: embeddableFonts.length > 0 ? embeddableFonts.map((f) => ({ name: f.family, data: f.data! })) : undefined,
    styles: {
      default: {
        document: bodyFont ? { run: { font: bodyFont } } : undefined,
        heading1: headingFont ? { run: { font: headingFont, color: accentColor, bold: true } } : undefined,
        heading2: headingFont ? { run: { font: headingFont, color: accentColor, bold: true } } : undefined,
        heading3: headingFont ? { run: { font: headingFont, color: accentColor, bold: true } } : undefined,
        heading4: headingFont ? { run: { font: headingFont, color: accentColor, italics: true } } : undefined,
        heading5: headingFont ? { run: { font: headingFont, color: accentColor } } : undefined,
        heading6: headingFont ? { run: { font: headingFont, color: accentColor } } : undefined,
      },
    },
    sections: [{ children }],
  });

  const raw = await Packer.toBuffer(document);
  const buffer = await repackageDeterministically(raw);

  return { buffer, diagnostics };
}

function pickThemeDefaults(report: TemplateReport): {
  headingFont?: string;
  bodyFont?: string;
  accentColor?: string;
} {
  const [majorFont, minorFont] = report.theme.fonts;
  return {
    headingFont: majorFont,
    bodyFont: minorFont ?? majorFont,
    accentColor: report.theme.colors[4] ?? report.theme.colors[0], // accent1 if present, else dk1
  };
}

async function resolveTemplateFonts(
  families: (string | undefined)[],
  options: RenderOptions,
): Promise<{ embeddableFonts: ResolvedFont[]; fontDiagnostics: Diagnostic[] }> {
  const wanted = [...new Set(families.filter((f): f is string => !!f))];
  if (wanted.length === 0) return { embeddableFonts: [], fontDiagnostics: [] };

  const resolved = await resolveFonts(wanted, { userFonts: options.userFonts, fetchImpl: options.fetchImpl });
  const embeddableFonts: ResolvedFont[] = [];
  const fontDiagnostics: Diagnostic[] = [];

  for (const family of wanted) {
    const result = resolved.get(family);
    if (result?.data) {
      embeddableFonts.push(result);
    } else {
      fontDiagnostics.push({
        severity: "warning",
        code: "font_unresolved",
        line: 1,
        message: `Font "${family}" from the template's theme could not be resolved (not on Google Fonts, no user-supplied file).`,
        hint: "Supply the font file via userFonts, or expect Word to substitute a fallback font.",
        strictPromotable: true,
      });
    }
  }

  return { embeddableFonts, fontDiagnostics };
}

async function renderBody(nodes: readonly BodyNode[], ctx: RenderContext): Promise<FileChild[]> {
  const out: FileChild[] = [];
  for (const node of nodes) {
    switch (node.type) {
      case "section":
        out.push(new Paragraph({ heading: HEADING_LEVELS[node.level], children: buildRuns(node.title, ctx) }));
        out.push(...(await renderBody(node.children, ctx)));
        break;
      case "paragraph":
        out.push(
          new Paragraph({
            children: buildRuns(node.runs, ctx, { forceItalic: ctx.inQuote }),
            indent: ctx.inQuote ? { left: 360 } : undefined,
          }),
        );
        break;
      case "table":
        out.push(buildTable(node, ctx));
        break;
      case "list":
        out.push(...buildList(node, ctx));
        break;
      case "media":
        out.push(await buildImageParagraph(node, ctx));
        break;
      case "code":
        out.push(...buildCodeParagraphs(node.value));
        break;
      case "quote":
        out.push(...(await renderBody(node.children, { ...ctx, inQuote: true })));
        break;
      case "break":
        out.push(new Paragraph({ children: [new PageBreak()] }));
        break;
      case "directive":
        out.push(...(await buildDirective(node, ctx)));
        break;
      default:
        break;
    }
  }
  return out;
}

interface RunOverrides {
  forceBold?: boolean;
  forceItalic?: boolean;
}

function buildRuns(
  runs: readonly InlineRun[],
  _ctx: RenderContext,
  overrides: RunOverrides = {},
): (TextRun | ExternalHyperlink)[] {
  return runs.map((r) => {
    const textRun = new TextRun({
      text: r.text,
      bold: overrides.forceBold || r.bold,
      italics: overrides.forceItalic || r.italic,
      strike: r.strike,
      font: r.code ? "Courier New" : undefined,
    });
    return r.link ? new ExternalHyperlink({ children: [textRun], link: r.link }) : textRun;
  });
}

function buildTable(node: TableNode, ctx: RenderContext): Table {
  const headerRow = new TableRow({
    tableHeader: true,
    children: node.header.map(
      (cell) =>
        new TableCell({
          shading: { fill: "D9E2F3" },
          children: [new Paragraph({ children: buildRuns(cell.runs, ctx, { forceBold: true }) })],
        }),
    ),
  });
  const bodyRows = node.rows.map(
    (row) =>
      new TableRow({
        children: row.map((cell) => new TableCell({ children: [new Paragraph({ children: buildRuns(cell.runs, ctx) })] })),
      }),
  );
  return new Table({ rows: [headerRow, ...bodyRows], width: { size: 100, type: WidthType.PERCENTAGE } });
}

function buildList(node: ListNode, ctx: RenderContext, level = 0): Paragraph[] {
  const out: Paragraph[] = [];

  node.items.forEach((item, index) => {
    const prefix: TextRun[] = [];
    if (item.checked !== null) {
      prefix.push(new TextRun({ text: item.checked ? "☒ " : "☐ " }));
    } else if (node.ordered) {
      prefix.push(new TextRun({ text: `${index + 1}. ` }));
    }

    const [firstChild, ...rest] = item.children;
    const isTextItem = firstChild?.type === "paragraph";
    const inlineRuns = isTextItem ? buildRuns(firstChild.runs, ctx) : [];

    const useBullet = item.checked === null && !node.ordered;
    out.push(
      new Paragraph({
        children: [...prefix, ...inlineRuns],
        bullet: useBullet ? { level } : undefined,
        indent: useBullet ? undefined : { left: 360 * (level + 1) },
      }),
    );

    const remainingChildren = isTextItem ? rest : item.children;
    for (const child of remainingChildren) {
      if (child.type === "list") out.push(...buildList(child, ctx, level + 1));
    }
  });

  return out;
}

async function buildImageParagraph(node: MediaNode, ctx: RenderContext, widthPercentOverride?: number): Promise<Paragraph> {
  if (!ctx.resolveAsset) {
    throw new Error(`Cannot render image "${node.src}": no asset resolver was provided to renderDocx().`);
  }
  const data = await ctx.resolveAsset(node.src);
  const dims = imageSize(data);
  if (!dims.width || !dims.height) {
    throw new Error(`Could not determine dimensions for image "${node.src}".`);
  }

  const percent = widthPercentOverride ?? node.widthPercent ?? 100;
  const width = Math.round(PAGE_CONTENT_WIDTH_PX * (percent / 100));
  const height = Math.round(width * (dims.height / dims.width));

  return new Paragraph({
    children: [new ImageRun({ type: mapImageType(dims.type, node.src), data, transformation: { width, height } })],
  });
}

function mapImageType(type: string | undefined, src: string): "png" | "jpg" | "gif" | "bmp" {
  switch (type) {
    case "png":
      return "png";
    case "jpg":
    case "jpeg":
      return "jpg";
    case "gif":
      return "gif";
    case "bmp":
      return "bmp";
    default:
      throw new Error(`Unsupported image type "${type}" for "${src}" (docx supports png, jpg, gif, bmp).`);
  }
}

function buildCodeParagraphs(value: string): Paragraph[] {
  const lines = value.length > 0 ? value.split("\n") : [""];
  return lines.map(
    (line) =>
      new Paragraph({
        children: [new TextRun({ text: line.length > 0 ? line : " ", font: "Courier New" })],
        shading: { fill: "F2F2F2" },
        spacing: { after: 0 },
      }),
  );
}

async function buildDirective(node: DirectiveNode, ctx: RenderContext): Promise<FileChild[]> {
  switch (node.name) {
    case "toc":
      return [new TableOfContents("Table of Contents", { hyperlink: true, headingStyleRange: "1-3" })];
    case "pagebreak":
      return [new Paragraph({ children: [new PageBreak()] })];
    case "image": {
      const media = node.children.find((c): c is MediaNode => c.type === "media");
      if (!media) return [];
      const widthArg = node.args.find((a) => a.startsWith("width="));
      const percent = widthArg ? Number(widthArg.slice("width=".length).replace("%", "")) : undefined;
      return [await buildImageParagraph(media, ctx, percent)];
    }
    default:
      // Unknown directives refine, never gatekeep — render children as if the directive were
      // absent (the validator already warns separately).
      return renderBody(node.children, ctx);
  }
}
