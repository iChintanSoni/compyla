/**
 * remark-directive's grammar (`:::name{attr}` / `:::name[label]`) doesn't match the Compyla
 * dialect's `::: name arg1, arg2 ... :::` free-text-args fences (see markdown-spec.md §1), so
 * fences are extracted here at the line level before the markdown ever reaches remark.
 *
 * Placeholders use Private Use Area codepoints, which cannot appear in real prose, so the
 * substitution is unambiguous to find again in the parsed mdast tree.
 */
const PLACEHOLDER_OPEN = "COMPYLA_DIRECTIVE:";
const PLACEHOLDER_CLOSE = "";

const CODE_FENCE_RE = /^(`{3,}|~{3,})/;
const OPEN_RE = /^:::[ \t]+([A-Za-z][\w-]*)[ \t]*(.*)$/;
const CLOSE_RE = /^:::[ \t]*$/;

export interface ExtractedDirective {
  readonly name: string;
  readonly argsRaw: string;
  readonly startLine: number;
  readonly endLine: number;
  readonly innerText: string;
}

export interface ExtractionResult {
  readonly text: string;
  readonly directives: ReadonlyMap<string, ExtractedDirective>;
}

export function placeholderFor(id: string): string {
  return `${PLACEHOLDER_OPEN}${id}${PLACEHOLDER_CLOSE}`;
}

export function matchPlaceholder(value: string): string | null {
  if (!value.startsWith(PLACEHOLDER_OPEN) || !value.endsWith(PLACEHOLDER_CLOSE)) return null;
  return value.slice(PLACEHOLDER_OPEN.length, value.length - PLACEHOLDER_CLOSE.length);
}

interface Frame {
  name: string;
  argsRaw: string;
  startLine: number;
  innerLines: string[];
}

export function extractDirectiveFences(source: string): ExtractionResult {
  const lines = source.split(/\r\n|\r|\n/);
  const outLines: string[] = [];
  const directives = new Map<string, ExtractedDirective>();
  const stack: Frame[] = [];
  let inCodeFence = false;
  let counter = 0;

  const emit = (line: string): void => {
    if (stack.length > 0) {
      stack[stack.length - 1]!.innerLines.push(line);
    } else {
      outLines.push(line);
    }
  };

  for (let i = 0; i < lines.length; i++) {
    const line = lines[i]!;
    const lineNo = i + 1;

    if (CODE_FENCE_RE.test(line)) {
      inCodeFence = !inCodeFence;
      emit(line);
      continue;
    }

    if (!inCodeFence) {
      const openMatch = OPEN_RE.exec(line);
      if (openMatch) {
        stack.push({ name: openMatch[1]!, argsRaw: (openMatch[2] ?? "").trim(), startLine: lineNo, innerLines: [] });
        continue;
      }
      if (CLOSE_RE.test(line) && stack.length > 0) {
        const frame = stack.pop()!;
        const id = `d${counter++}`;
        directives.set(id, {
          name: frame.name,
          argsRaw: frame.argsRaw,
          startLine: frame.startLine,
          endLine: lineNo,
          innerText: frame.innerLines.join("\n"),
        });
        emit(placeholderFor(id));
        // The block spanned (lineNo - frame.startLine + 1) lines; one was consumed by the
        // placeholder itself, so pad the rest with blanks to keep absolute line numbers correct
        // for everything after the block.
        const blankLinesNeeded = lineNo - frame.startLine;
        for (let p = 0; p < blankLinesNeeded; p++) emit("");
        continue;
      }
    }

    emit(line);
  }

  // Unclosed fences: best-effort — flush each opener and its content back as literal lines,
  // outermost first, rather than silently dropping content.
  for (const frame of stack) {
    outLines.push(`::: ${frame.name} ${frame.argsRaw}`.trimEnd());
    outLines.push(...frame.innerLines);
  }

  return { text: outLines.join("\n"), directives };
}

/**
 * Pads inner directive content with blank lines so a recursive parse of just that block
 * reports line numbers absolute to the original source, not relative to the block.
 */
export function padForRecursiveParse(directive: ExtractedDirective): string {
  return "\n".repeat(directive.startLine) + directive.innerText;
}
