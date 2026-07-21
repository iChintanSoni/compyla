import { unified } from "unified";
import remarkParse from "remark-parse";
import remarkGfm from "remark-gfm";
import remarkFrontmatter from "remark-frontmatter";
import { parse as parseYaml } from "yaml";
import type { Paragraph, Root, RootContent, Text, Yaml } from "mdast";
import {
  extractDirectiveFences,
  matchPlaceholder,
  padForRecursiveParse,
  type ExtractedDirective,
} from "./directive-fences.js";
import type { DocumentMeta } from "./types.js";

export interface CompylaDirectiveMdastNode {
  readonly type: "compylaDirective";
  readonly name: string;
  readonly argsRaw: string;
  readonly children: readonly AugmentedContent[];
  readonly position: { start: { line: number; column: number }; end: { line: number; column: number } };
}

export type AugmentedContent = RootContent | CompylaDirectiveMdastNode;

export interface ParseResult {
  readonly meta: DocumentMeta;
  readonly children: readonly AugmentedContent[];
}

const processor = unified().use(remarkParse).use(remarkFrontmatter, ["yaml"]).use(remarkGfm);

export function parseMarkdown(source: string): ParseResult {
  // Fences are extracted exactly once, up front: extractDirectiveFences already resolves
  // arbitrary nesting depth into one flat map keyed by placeholder id. Directive bodies are
  // then parsed by walking that SAME map recursively (see resolveChildren) rather than
  // re-invoking fence extraction on already-stripped inner text, which would find nothing.
  const { text, directives } = extractDirectiveFences(source);
  return parseWithDirectives(text, directives);
}

function parseWithDirectives(
  text: string,
  directives: ReadonlyMap<string, ExtractedDirective>,
): ParseResult {
  const tree = processor.parse(text) as Root;

  let meta: DocumentMeta = {};
  const children: AugmentedContent[] = [];

  for (const node of tree.children) {
    if (node.type === "yaml") {
      meta = parseFrontmatter((node as Yaml).value);
      continue;
    }
    children.push(spliceDirective(node, directives));
  }

  return { meta, children };
}

function parseFrontmatter(value: string): DocumentMeta {
  if (!value.trim()) return {};
  const parsed: unknown = parseYaml(value);
  if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
    return parsed as DocumentMeta;
  }
  return {};
}

function spliceDirective(
  node: RootContent,
  directives: ReadonlyMap<string, ExtractedDirective>,
): AugmentedContent {
  const placeholderId = extractPlaceholderId(node);
  if (placeholderId === null) return node;

  const directive = directives.get(placeholderId);
  if (!directive) return node;

  const inner = parseWithDirectives(padForRecursiveParse(directive), directives);

  const result: CompylaDirectiveMdastNode = {
    type: "compylaDirective",
    name: directive.name,
    argsRaw: directive.argsRaw,
    children: inner.children,
    position: {
      start: { line: directive.startLine, column: 1 },
      end: { line: directive.endLine, column: 4 },
    },
  };
  return result;
}

function extractPlaceholderId(node: RootContent): string | null {
  if (node.type !== "paragraph") return null;
  const paragraph = node as Paragraph;
  if (paragraph.children.length !== 1 || paragraph.children[0]?.type !== "text") return null;
  const text = paragraph.children[0] as Text;
  return matchPlaceholder(text.value);
}
