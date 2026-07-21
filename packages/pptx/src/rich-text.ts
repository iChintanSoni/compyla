import type { BodyNode, InlineRun, ListNode, ParagraphNode } from "@compyla/core";

export interface TextRunItem {
  readonly text: string;
  readonly options?: Record<string, unknown>;
}

export function toRichText(runs: readonly InlineRun[], opts: { forceItalic?: boolean } = {}): TextRunItem[] {
  if (runs.length === 0) return [{ text: "" }];
  return runs.map((r) => ({
    text: r.text,
    options: {
      bold: r.bold || undefined,
      italic: opts.forceItalic || r.italic || undefined,
      strike: r.strike || undefined,
      fontFace: r.code ? "Courier New" : undefined,
      hyperlink: r.link ? { url: r.link } : undefined,
    },
  }));
}

function withTrailingBreak(items: readonly TextRunItem[]): TextRunItem[] {
  if (items.length === 0) return [];
  const last = items[items.length - 1]!;
  return [...items.slice(0, -1), { ...last, options: { ...last.options, breakLine: true } }];
}

/**
 * Flattens a run of paragraphs/lists/demoted-headings/code into pptxgenjs rich-text paragraphs
 * for a single text placeholder. Tables/media/quotes are their own indivisible blocks, handled
 * separately (see render.ts's groupIntoRenderBlocks) — they never reach this function.
 */
export function flowingRichText(nodes: readonly BodyNode[]): TextRunItem[] {
  const out: TextRunItem[] = [];
  for (const node of nodes) {
    if (node.type === "paragraph") {
      out.push(...withTrailingBreak(toRichText(node.runs)));
    } else if (node.type === "section") {
      out.push({ text: plainText(node.title), options: { bold: true, breakLine: true } });
      out.push(...flowingRichText(node.children));
    } else if (node.type === "list") {
      out.push(...listToRichText(node));
    } else if (node.type === "code") {
      const lines = node.value.length > 0 ? node.value.split("\n") : [""];
      for (const line of lines) {
        out.push({ text: line.length > 0 ? line : " ", options: { fontFace: "Courier New", breakLine: true } });
      }
    }
  }
  return out;
}

function listToRichText(list: ListNode, level = 0): TextRunItem[] {
  const out: TextRunItem[] = [];
  list.items.forEach((item) => {
    const firstParagraph = item.children.find((c): c is ParagraphNode => c.type === "paragraph");
    const text = firstParagraph ? plainText(firstParagraph.runs) : "";

    if (item.checked !== null) {
      out.push({ text: `${item.checked ? "☒" : "☐"} ${text}`, options: { indent: 18 * (level + 1), breakLine: true } });
    } else {
      out.push({
        text,
        options: { bullet: list.ordered ? { type: "number" } : true, indent: 18 * (level + 1), breakLine: true },
      });
    }

    for (const child of item.children) {
      if (child.type === "list") out.push(...listToRichText(child, level + 1));
    }
  });
  return out;
}

export function plainText(runs: readonly InlineRun[]): string {
  return runs.map((r) => r.text).join("");
}

/** Joins each top-level paragraph/section as one line — used for subtitles, captions, and notes,
 *  where a single plain string (not rich text) is what pptxgenjs expects. */
export function flattenPlainText(nodes: readonly BodyNode[]): string {
  const lines: string[] = [];
  for (const node of nodes) {
    if (node.type === "paragraph") lines.push(plainText(node.runs));
    else if (node.type === "section") lines.push(plainText(node.title), ...flattenPlainText(node.children).split("\n").filter(Boolean));
  }
  return lines.join("\n");
}
