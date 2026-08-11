import { walkBody } from "@compyla/core";
import type { BodyNode, SectionNode } from "@compyla/core";
import { plainText } from "./render-html.js";

export interface SearchRecord {
  readonly pageSlug: string;
  readonly pageTitle: string;
  readonly headingText: string;
  readonly anchor: string;
  readonly text: string;
}

export interface SearchPageInput {
  readonly slug: string;
  readonly title: string;
  readonly body: readonly BodyNode[];
  readonly headingIds: ReadonlyMap<SectionNode, string>;
}

/** One record per heading (any level), capturing that section's own direct content only —
 *  nested subsections become their own separate records rather than bleeding into their
 *  parent's text, which keeps records granular enough for useful search results. */
export function buildSearchIndex(pages: readonly SearchPageInput[]): SearchRecord[] {
  const records: SearchRecord[] = [];
  for (const page of pages) {
    walkBody(page.body, 0, {
      onSection: (node) => {
        const id = page.headingIds.get(node)!;
        records.push({
          pageSlug: page.slug,
          pageTitle: page.title,
          headingText: plainText(node.title),
          anchor: `#${id}`,
          text: flattenOwnText(node.children),
        });
      },
    });
  }
  return records;
}

/** Concatenates a node list's own text content, stopping at nested SectionNodes (their content
 *  becomes their own record instead). */
function flattenOwnText(nodes: readonly BodyNode[]): string {
  const parts: string[] = [];
  const visit = (list: readonly BodyNode[]): void => {
    for (const node of list) {
      switch (node.type) {
        case "section":
          break;
        case "paragraph":
          parts.push(plainText(node.runs));
          break;
        case "table":
          for (const cell of node.header) parts.push(plainText(cell.runs));
          for (const row of node.rows) for (const cell of row) parts.push(plainText(cell.runs));
          break;
        case "list":
          for (const item of node.items) visit(item.children);
          break;
        case "code":
          parts.push(node.value);
          break;
        case "quote":
        case "directive":
          visit(node.children);
          break;
        case "media":
          parts.push(node.alt);
          break;
        case "break":
          break;
      }
    }
  };
  visit(nodes);
  return parts.join(" ");
}
