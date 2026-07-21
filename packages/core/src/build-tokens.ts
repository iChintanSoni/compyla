import { toString as mdastToString } from "mdast-util-to-string";
import type {
  Blockquote,
  Code,
  Heading,
  Html,
  Image,
  List,
  ListItem,
  Paragraph,
  PhrasingContent,
  Table,
  TableCell as MdastTableCell,
  TableRow,
} from "mdast";
import type { AugmentedContent, CompylaDirectiveMdastNode } from "./parser.js";
import type { ParseResult } from "./parser.js";
import type {
  BodyNode,
  ColumnAlign,
  CompylaDocument,
  DirectiveNode,
  InlineRun,
  ListItemNode,
  ListNode,
  MediaNode,
  Position,
  SectionNode,
  TableCell,
  TableNode,
} from "./types.js";

export function buildDocument(parseResult: ParseResult): CompylaDocument {
  return { meta: parseResult.meta, body: buildBody(parseResult.children) };
}

interface DraftSection extends SectionNode {
  children: BodyNode[];
}

function buildBody(nodes: readonly AugmentedContent[]): BodyNode[] {
  const root: BodyNode[] = [];
  const stack: DraftSection[] = [];
  const currentTarget = (): BodyNode[] => (stack.length > 0 ? stack[stack.length - 1]!.children : root);

  for (const node of nodes) {
    if (node.type === "heading") {
      const level = (node as Heading).depth as 1 | 2 | 3 | 4 | 5 | 6;
      while (stack.length > 0 && stack[stack.length - 1]!.level >= level) stack.pop();

      const section: DraftSection = {
        type: "section",
        level,
        title: buildInlineRuns((node as Heading).children),
        children: [],
        position: positionOf(node),
      };
      currentTarget().push(section);
      stack.push(section);
      continue;
    }

    const built = buildBodyNode(node);
    if (built) currentTarget().push(built);
  }

  return root;
}

function buildBodyNode(node: AugmentedContent): BodyNode | null {
  switch (node.type) {
    case "paragraph": {
      const media = tryBuildMedia(node as Paragraph);
      if (media) return media;
      return { type: "paragraph", runs: buildInlineRuns((node as Paragraph).children), position: positionOf(node) };
    }
    case "table":
      return buildTable(node as Table);
    case "list":
      return buildList(node as List);
    case "code": {
      const code = node as Code;
      return { type: "code", lang: code.lang ?? undefined, value: code.value, position: positionOf(node) };
    }
    case "blockquote":
      return {
        type: "quote",
        children: buildBody((node as Blockquote).children as AugmentedContent[]),
        position: positionOf(node),
      };
    case "thematicBreak":
      return { type: "break", position: positionOf(node) };
    case "html":
      return tryBuildLeafDirective(node as Html);
    case "compylaDirective":
      return buildDirective(node as CompylaDirectiveMdastNode);
    default:
      return null;
  }
}

function buildDirective(node: CompylaDirectiveMdastNode): DirectiveNode {
  return {
    type: "directive",
    name: node.name,
    args: node.argsRaw.length > 0 ? node.argsRaw.split(/\s+/) : [],
    children: buildBody(node.children),
    position: { line: node.position.start.line, column: node.position.start.column },
  };
}

const LEAF_MARKER_RE = /^<!--\s*([A-Za-z][\w-]*)\s*-->$/;

function tryBuildLeafDirective(node: Html): DirectiveNode | null {
  const match = LEAF_MARKER_RE.exec(node.value.trim());
  if (!match) return null;
  return { type: "directive", name: match[1]!, args: [], children: [], position: positionOf(node) };
}

function tryBuildMedia(node: Paragraph): MediaNode | null {
  if (node.children.length !== 1 || node.children[0]?.type !== "image") return null;
  const image = node.children[0] as Image;
  return { type: "media", src: image.url, alt: image.alt ?? "", position: positionOf(node) };
}

function buildTable(node: Table): TableNode {
  const [headerRow, ...bodyRows] = node.children;
  return {
    type: "table",
    header: headerRow ? buildRow(headerRow) : [],
    align: (node.align ?? []).map((a): ColumnAlign => (a ?? null)),
    rows: bodyRows.map(buildRow),
    position: positionOf(node),
  };
}

function buildRow(row: TableRow): TableCell[] {
  return row.children.map((cell: MdastTableCell) => ({
    runs: buildInlineRuns(cell.children),
    raw: mdastToString(cell),
  }));
}

function buildList(node: List): ListNode {
  return {
    type: "list",
    ordered: node.ordered ?? false,
    items: node.children.map(buildListItem),
    position: positionOf(node),
  };
}

function buildListItem(node: ListItem): ListItemNode {
  return {
    children: buildBody(node.children as AugmentedContent[]),
    checked: node.checked ?? null,
    position: positionOf(node),
  };
}

interface InlineMarks {
  bold?: boolean;
  italic?: boolean;
  code?: boolean;
  strike?: boolean;
  link?: string;
}

function buildInlineRuns(nodes: readonly PhrasingContent[], marks: InlineMarks = {}): InlineRun[] {
  const runs: InlineRun[] = [];
  for (const node of nodes) {
    switch (node.type) {
      case "text":
        runs.push({ text: node.value, ...marks });
        break;
      case "strong":
        runs.push(...buildInlineRuns(node.children, { ...marks, bold: true }));
        break;
      case "emphasis":
        runs.push(...buildInlineRuns(node.children, { ...marks, italic: true }));
        break;
      case "delete":
        runs.push(...buildInlineRuns(node.children, { ...marks, strike: true }));
        break;
      case "inlineCode":
        runs.push({ text: node.value, ...marks, code: true });
        break;
      case "link":
        runs.push(...buildInlineRuns(node.children, { ...marks, link: node.url }));
        break;
      case "break":
        runs.push({ text: "\n", ...marks });
        break;
      default:
        // Inline images, raw inline HTML, footnote references, etc. are unsupported inside
        // running text for Phase 0 and are dropped (graceful degradation, not an error).
        break;
    }
  }
  return runs;
}

function positionOf(node: { position?: { start: { line: number; column: number } } }): Position {
  return node.position
    ? { line: node.position.start.line, column: node.position.start.column }
    : { line: 1, column: 1 };
}
