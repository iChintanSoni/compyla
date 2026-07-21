import type { Diagnostic } from "../diagnostics.js";
import type { Profile, ValidateOptions } from "../profile.js";
import type { CompylaDocument } from "../types.js";
import { walkBody } from "../walk.js";

const KNOWN_DIRECTIVES = ["toc", "pagebreak", "image"] as const;
const KNOWN_DIRECTIVE_SET = new Set<string>(KNOWN_DIRECTIVES);

export const docxProfile: Profile = {
  name: "docx",
  knownDirectives: KNOWN_DIRECTIVES,

  validate(doc: CompylaDocument, options: ValidateOptions): Diagnostic[] {
    const diagnostics: Diagnostic[] = [];

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
            message: `Unknown directive "${directive.name}" for the docx profile; it renders as if absent.`,
            hint: `Known docx directives: ${KNOWN_DIRECTIVES.join(", ")}.`,
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
