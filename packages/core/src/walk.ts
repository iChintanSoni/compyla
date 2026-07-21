import type { BodyNode, DirectiveNode, MediaNode, SectionNode } from "./types.js";

export interface WalkVisitor {
  onSection?: (node: SectionNode, parentLevel: number) => void;
  onDirective?: (node: DirectiveNode) => void;
  onMedia?: (node: MediaNode) => void;
}

/** Depth-first walk over sections, directives, quotes, and list items — the nodes that can nest. */
export function walkBody(nodes: readonly BodyNode[], parentSectionLevel: number, visit: WalkVisitor): void {
  for (const node of nodes) {
    switch (node.type) {
      case "section":
        visit.onSection?.(node, parentSectionLevel);
        walkBody(node.children, node.level, visit);
        break;
      case "directive":
        visit.onDirective?.(node);
        walkBody(node.children, parentSectionLevel, visit);
        break;
      case "quote":
        walkBody(node.children, parentSectionLevel, visit);
        break;
      case "list":
        for (const item of node.items) walkBody(item.children, parentSectionLevel, visit);
        break;
      case "media":
        visit.onMedia?.(node);
        break;
      default:
        break;
    }
  }
}
