export interface Position {
  readonly line: number;
  readonly column: number;
}

export interface InlineRun {
  readonly text: string;
  readonly bold?: boolean;
  readonly italic?: boolean;
  readonly code?: boolean;
  readonly strike?: boolean;
  readonly link?: string;
}

export interface TableCell {
  readonly runs: readonly InlineRun[];
  readonly raw: string;
}

export type ColumnAlign = "left" | "center" | "right" | null;

export interface SectionNode {
  readonly type: "section";
  readonly level: 1 | 2 | 3 | 4 | 5 | 6;
  readonly title: readonly InlineRun[];
  readonly children: readonly BodyNode[];
  readonly position: Position;
}

export interface ParagraphNode {
  readonly type: "paragraph";
  readonly runs: readonly InlineRun[];
  readonly position: Position;
}

export interface TableNode {
  readonly type: "table";
  readonly header: readonly TableCell[];
  readonly align: readonly ColumnAlign[];
  readonly rows: readonly (readonly TableCell[])[];
  readonly position: Position;
}

export interface ListItemNode {
  readonly children: readonly BodyNode[];
  /** null = plain list item, boolean = GFM task-list checkbox state. */
  readonly checked: boolean | null;
  readonly position: Position;
}

export interface ListNode {
  readonly type: "list";
  readonly ordered: boolean;
  readonly items: readonly ListItemNode[];
  readonly position: Position;
}

export interface MediaNode {
  readonly type: "media";
  readonly src: string;
  readonly alt: string;
  readonly widthPercent?: number;
  readonly position: Position;
}

export interface CodeNode {
  readonly type: "code";
  readonly lang?: string;
  readonly value: string;
  readonly position: Position;
}

export interface QuoteNode {
  readonly type: "quote";
  readonly children: readonly BodyNode[];
  readonly position: Position;
}

export interface BreakNode {
  readonly type: "break";
  readonly position: Position;
}

export interface DirectiveNode {
  readonly type: "directive";
  readonly name: string;
  /** Raw whitespace-tokenized arguments; each directive/profile interprets its own shape. */
  readonly args: readonly string[];
  readonly children: readonly BodyNode[];
  readonly position: Position;
}

export type BodyNode =
  | SectionNode
  | ParagraphNode
  | TableNode
  | ListNode
  | MediaNode
  | CodeNode
  | QuoteNode
  | BreakNode
  | DirectiveNode;

export interface DocumentMeta {
  readonly format?: string;
  readonly template?: string;
  readonly title?: string;
  readonly author?: string;
  readonly lang?: string;
  readonly spec?: string;
  readonly [key: string]: unknown;
}

export interface CompylaDocument {
  readonly meta: DocumentMeta;
  readonly body: readonly BodyNode[];
}
