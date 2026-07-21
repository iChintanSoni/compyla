import type { BodyNode, PptxLayoutName } from "@compyla/core";

export interface PlaceholderSpec {
  readonly name: string;
  readonly type: "title" | "body" | "pic";
  readonly x: number;
  readonly y: number;
  readonly w: number;
  readonly h: number;
  readonly fontSize?: number;
  readonly bold?: boolean;
  readonly italic?: boolean;
  readonly align?: "left" | "center" | "right";
}

// Compyla's house layouts (see architecture.md D3/D4): pptxgenjs builds slides from these
// definitions rather than reusing an uploaded template's actual slide layouts. Positions are in
// inches against pptxgenjs's default 10in x 5.63in (16:9) slide.
export const SLIDE_LAYOUTS: Record<PptxLayoutName, readonly PlaceholderSpec[]> = {
  "Title Slide": [
    { name: "title", type: "title", x: 0.5, y: 1.8, w: 9, h: 1.2, fontSize: 36, bold: true, align: "center" },
    { name: "subtitle", type: "body", x: 0.5, y: 3.1, w: 9, h: 0.8, fontSize: 20, align: "center" },
  ],
  "Section Header": [{ name: "title", type: "title", x: 0.5, y: 2.3, w: 9, h: 1.3, fontSize: 32, bold: true }],
  "Title and Content": [
    { name: "title", type: "title", x: 0.4, y: 0.3, w: 9.2, h: 0.9, fontSize: 28, bold: true },
    { name: "body", type: "body", x: 0.4, y: 1.3, w: 9.2, h: 4.0, fontSize: 16 },
  ],
  "Two Content": [
    { name: "title", type: "title", x: 0.4, y: 0.3, w: 9.2, h: 0.9, fontSize: 28, bold: true },
    { name: "body1", type: "body", x: 0.4, y: 1.3, w: 4.5, h: 4.0, fontSize: 16 },
    { name: "body2", type: "body", x: 5.1, y: 1.3, w: 4.5, h: 4.0, fontSize: 16 },
  ],
  "Picture with Caption": [
    { name: "title", type: "title", x: 0.4, y: 0.3, w: 9.2, h: 0.7, fontSize: 24, bold: true },
    { name: "pic", type: "pic", x: 0.4, y: 1.1, w: 9.2, h: 3.4 },
    { name: "caption", type: "body", x: 0.4, y: 4.6, w: 9.2, h: 0.7, fontSize: 14, italic: true },
  ],
  Quote: [
    { name: "title", type: "title", x: 0.4, y: 0.3, w: 9.2, h: 0.7, fontSize: 24, bold: true },
    { name: "body", type: "body", x: 0.9, y: 1.3, w: 8.2, h: 3.5, fontSize: 20, italic: true },
  ],
};

/** Priority-2 inference from content shape (markdown-spec.md §6.3), used when no explicit
 *  `::: layout` directive is given. */
export function inferLayout(content: readonly BodyNode[]): PptxLayoutName {
  if (content.length === 0) return "Section Header";
  if (content.some((n) => n.type === "directive" && n.name === "col")) return "Two Content";
  if (content.length === 1) {
    const only = content[0]!;
    if (only.type === "table") return "Title and Content"; // table fills the body placeholder
    if (only.type === "media") return "Picture with Caption";
    if (only.type === "quote") return "Quote";
  }
  return "Title and Content";
}

/** `<!-- col -->` markers split content into columns for the Two Content layout. */
export function splitColumns(nodes: readonly BodyNode[]): BodyNode[][] {
  const columns: BodyNode[][] = [[]];
  for (const node of nodes) {
    if (node.type === "directive" && node.name === "col") {
      columns.push([]);
      continue;
    }
    columns[columns.length - 1]!.push(node);
  }
  return columns;
}
