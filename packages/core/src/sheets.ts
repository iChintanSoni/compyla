import type { BodyNode, DirectiveNode, InlineRun, ParagraphNode, Position, SectionNode, TableNode } from "./types.js";

export interface SheetTable {
  readonly table: TableNode;
  /** From the immediately preceding H2 heading, if any (markdown-spec.md §8.1's "range title"). */
  readonly rangeTitle?: readonly InlineRun[];
  /** From the immediately preceding paragraph, if any (the "note cell"). */
  readonly note?: ParagraphNode;
  readonly columnsDirective?: DirectiveNode;
  readonly anchorDirective?: DirectiveNode;
  readonly tableDirective?: DirectiveNode;
}

export interface Sheet {
  readonly name: string;
  readonly namePosition: Position;
  readonly tables: readonly SheetTable[];
  /** Quote/code/media/H3+ nodes that have no xlsx meaning — "ignored with warning" (§8.1). */
  readonly ignored: readonly BodyNode[];
}

interface Pending {
  rangeTitle?: readonly InlineRun[];
  note?: ParagraphNode;
  columnsDirective?: DirectiveNode;
  anchorDirective?: DirectiveNode;
  tableDirective?: DirectiveNode;
}

const XLSX_DIRECTIVE_NAMES = new Set(["columns", "anchor", "table"]);

function isH1(node: BodyNode): node is SectionNode {
  return node.type === "section" && node.level === 1;
}

/**
 * Groups a document's body into xlsx sheets: each H1 becomes a sheet (markdown-spec.md §8.1);
 * content before the first H1 becomes a default sheet named `defaultName`. Within a sheet, walks
 * depth-first (H2s nest one level under their H1 per the normal section tree) tracking the
 * nearest-preceding H2 title, paragraph, and `columns`/`anchor`/`table` directives, attaching
 * them to the next table encountered. Shared by the xlsx profile (validate) and the
 * `@compyla/xlsx` renderer so they can't disagree about which annotation belongs to which table —
 * same rationale as slides.ts's resolveSlideContent for pptx.
 */
export function groupSheets(body: readonly BodyNode[], defaultName: string): Sheet[] {
  const firstH1 = body.findIndex(isH1);
  const preamble = firstH1 === -1 ? body : body.slice(0, firstH1);
  const rest = firstH1 === -1 ? [] : body.slice(firstH1);

  const sheets: Sheet[] = [];

  if (preamble.length > 0) {
    const tables: SheetTable[] = [];
    const ignored: BodyNode[] = [];
    walkSheetChildren(preamble, tables, ignored, {});
    if (tables.length > 0 || ignored.length > 0) {
      sheets.push({ name: defaultName, namePosition: preamble[0]!.position, tables, ignored });
    }
  }

  for (const node of rest) {
    if (!isH1(node)) continue;
    const tables: SheetTable[] = [];
    const ignored: BodyNode[] = [];
    walkSheetChildren(node.children, tables, ignored, {});
    sheets.push({ name: runsToPlainText(node.title), namePosition: node.position, tables, ignored });
  }

  return sheets;
}

function walkSheetChildren(nodes: readonly BodyNode[], tables: SheetTable[], ignored: BodyNode[], pending: Pending): void {
  for (const node of nodes) collectInto(node, tables, ignored, pending);
}

function collectInto(node: BodyNode, tables: SheetTable[], ignored: BodyNode[], pending: Pending): void {
  switch (node.type) {
    case "section":
      if (node.level === 2) {
        pending.rangeTitle = node.title;
        walkSheetChildren(node.children, tables, ignored, pending);
      } else {
        ignored.push(node);
      }
      return;
    case "paragraph":
      pending.note = node;
      return;
    case "directive":
      if (node.name === "columns") pending.columnsDirective = node;
      else if (node.name === "anchor") pending.anchorDirective = node;
      else if (node.name === "table") pending.tableDirective = node;
      // Unknown directive names are flagged by the xlsx profile's own directive walk, not here.
      return;
    case "table":
      tables.push({
        table: node,
        rangeTitle: pending.rangeTitle,
        note: pending.note,
        columnsDirective: pending.columnsDirective,
        anchorDirective: pending.anchorDirective,
        tableDirective: pending.tableDirective,
      });
      pending.rangeTitle = undefined;
      pending.note = undefined;
      pending.columnsDirective = undefined;
      pending.anchorDirective = undefined;
      pending.tableDirective = undefined;
      return;
    case "break":
      return;
    default:
      ignored.push(node);
      return;
  }
}

export function isKnownXlsxDirective(name: string): boolean {
  return XLSX_DIRECTIVE_NAMES.has(name);
}

function runsToPlainText(runs: readonly InlineRun[]): string {
  return runs.map((r) => r.text).join("");
}
