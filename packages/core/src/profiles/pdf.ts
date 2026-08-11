import type { Diagnostic } from "../diagnostics.js";
import type { Profile, ValidateOptions } from "../profile.js";
import type { CompylaDocument } from "../types.js";
import { walkBody } from "../walk.js";

const KNOWN_DIRECTIVES = ["toc", "pagebreak", "image"] as const;
const KNOWN_DIRECTIVE_SET = new Set<string>(KNOWN_DIRECTIVES);

const PAGE_SIZES = ["A4", "Letter"] as const;
const MARGIN_PRESETS = ["normal", "narrow"] as const;

export const pdfProfile: Profile = {
  name: "pdf",
  knownDirectives: KNOWN_DIRECTIVES,

  validate(doc: CompylaDocument, options: ValidateOptions): Diagnostic[] {
    const diagnostics: Diagnostic[] = [];

    const pageSize = doc.meta.page_size;
    if (pageSize !== undefined && !PAGE_SIZES.includes(pageSize as (typeof PAGE_SIZES)[number])) {
      diagnostics.push({
        severity: "warning",
        code: "invalid_page_size",
        line: 1,
        message: `Unknown page_size "${String(pageSize)}"; falling back to A4.`,
        hint: `Known page sizes: ${PAGE_SIZES.join(", ")}.`,
      });
    }

    const margins = doc.meta.margins;
    if (margins !== undefined && !MARGIN_PRESETS.includes(margins as (typeof MARGIN_PRESETS)[number])) {
      diagnostics.push({
        severity: "warning",
        code: "invalid_margins",
        line: 1,
        message: `Unknown margins "${String(margins)}"; falling back to normal.`,
        hint: `Known margin presets: ${MARGIN_PRESETS.join(", ")}.`,
      });
    }

    walkBody(doc.body, 0, {
      onSection(section, parentLevel) {
        if (parentLevel > 0 && section.level - parentLevel > 1) {
          diagnostics.push({
            severity: "warning",
            code: "heading_skip",
            line: section.position.line,
            message: `Heading level jumps from H${parentLevel} to H${section.level}.`,
            hint: `Insert an H${parentLevel + 1} before this heading, or promote it to H${parentLevel + 1}.`,
          });
        }
      },
      onDirective(directive) {
        if (!KNOWN_DIRECTIVE_SET.has(directive.name)) {
          diagnostics.push({
            severity: "warning",
            code: "unknown_directive",
            line: directive.position.line,
            message: `Unknown directive "${directive.name}" for the pdf profile; it renders as if absent.`,
            hint: `Known pdf directives: ${KNOWN_DIRECTIVES.join(", ")}.`,
          });
        }
      },
      onMedia(media) {
        const resolves = options.resolveAsset ? options.resolveAsset(media.src) : true;
        if (!resolves) {
          diagnostics.push({
            severity: "error",
            code: "image_unresolved",
            line: media.position.line,
            message: `Image "${media.src}" could not be resolved.`,
            hint: `Check the path is correct, or pass an assets directory so relative paths resolve.`,
          });
        }
      },
    });

    return diagnostics;
  },
};
