import type { Diagnostic } from "../diagnostics.js";
import { KNOWN_PPTX_LAYOUTS, resolvePptxLayoutName } from "../pptx-layouts.js";
import type { Profile, ValidateOptions } from "../profile.js";
import { groupSlides, resolveSlideContent, type Slide } from "../slides.js";
import type { BodyNode, CompylaDocument, DirectiveNode } from "../types.js";
import { walkBody } from "../walk.js";

const KNOWN_DIRECTIVES = ["layout", "notes", "image", "col"] as const;
const KNOWN_DIRECTIVE_SET = new Set<string>(KNOWN_DIRECTIVES);

const LAYOUT_MAX_CONTENT_BLOCKS: Record<string, number> = {
  "title slide": Number.POSITIVE_INFINITY,
  "section header": 0,
  "title and content": 1,
  "two content": 2,
  "picture with caption": 2,
  quote: 1,
};

export const pptxProfile: Profile = {
  name: "pptx",
  knownDirectives: KNOWN_DIRECTIVES,

  validate(doc: CompylaDocument, _options: ValidateOptions): Diagnostic[] {
    const diagnostics: Diagnostic[] = [];

    for (const slide of groupSlides(doc.body)) {
      walkBody(slide.body, 0, {
        onSection(section) {
          if (section.level >= 4) {
            diagnostics.push({
              severity: "warning",
              code: "heading_too_deep",
              line: section.position.line,
              message: `H${section.level} heading inside a slide body (likely a docx-oriented document).`,
              hint: "A pptx slide has one heading — its title. Demote or remove this heading.",
            });
          }
        },
        onDirective(directive) {
          if (!KNOWN_DIRECTIVE_SET.has(directive.name)) {
            diagnostics.push({
              severity: "warning",
              code: "unknown_directive",
              line: directive.position.line,
              message: `Unknown directive "${directive.name}" for the pptx profile; it renders as if absent.`,
              hint: `Known pptx directives: ${KNOWN_DIRECTIVES.join(", ")}.`,
            });
          }
        },
      });

      diagnostics.push(...validateLayout(slide));
    }

    return diagnostics;
  },
};

function validateLayout(slide: Slide): Diagnostic[] {
  const { layoutDirective, content } = resolveSlideContent(slide);
  if (!layoutDirective) return [];

  const requested = layoutDirective.argsRaw.trim();
  const resolved = resolvePptxLayoutName(requested);
  if (!resolved) {
    return [
      {
        severity: "error",
        code: "unknown_layout",
        line: layoutDirective.position.line,
        message: `Layout "${requested}" does not resolve to a known pptx layout.`,
        hint: `Available layouts: ${KNOWN_PPTX_LAYOUTS.join(", ")}.`,
      },
    ];
  }
  const name = resolved;

  const max = LAYOUT_MAX_CONTENT_BLOCKS[name.toLowerCase()] ?? Number.POSITIVE_INFINITY;
  const blocks = countContentBlocks(content);
  if (blocks <= max) return [];

  return [
    {
      severity: "warning",
      code: "layout_overflow",
      line: layoutDirective.position.line,
      message: `Layout "${name}" has room for ${max} content block(s); this slide has ${blocks}.`,
      hint: "Extra content flows into the last placeholder. Split the slide or choose a layout with more placeholders.",
      strictPromotable: true,
    },
  ];
}

/**
 * A single placeholder holds a flowing run of paragraphs/list items fine (that's not overflow —
 * PowerPoint text boxes hold arbitrary amounts of running text); a table, image, quote, code
 * block, or demoted heading is its own indivisible unit that can't share a placeholder with
 * something else. `<!-- col -->` markers always start a new block (a new column).
 */
function countContentBlocks(nodes: readonly BodyNode[]): number {
  let blocks = 0;
  let inTextRun = false;
  for (const node of nodes) {
    if (isColMarker(node)) {
      inTextRun = false;
      continue;
    }
    if (node.type === "paragraph" || node.type === "list") {
      if (!inTextRun) {
        blocks++;
        inTextRun = true;
      }
      continue;
    }
    blocks++;
    inTextRun = false;
  }
  return blocks;
}

function isColMarker(node: BodyNode): node is DirectiveNode {
  return node.type === "directive" && node.name === "col";
}
