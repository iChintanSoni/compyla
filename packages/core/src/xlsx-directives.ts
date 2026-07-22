/**
 * Argument parsers for the three xlsx directives (markdown-spec.md §8.3). Shared by the xlsx
 * profile (validate) and the `@compyla/xlsx` renderer so they can never disagree about what a
 * directive means — same rationale as slides.ts's resolveSlideContent for pptx.
 */

export const COLUMN_TYPES = ["number", "currency", "percent", "date", "text", "boolean"] as const;
export type ColumnType = (typeof COLUMN_TYPES)[number];
const COLUMN_TYPE_SET = new Set<string>(COLUMN_TYPES);

export interface ColumnSpec {
  readonly type: ColumnType;
  /** e.g. "iso" in `date(iso)`. Undefined when the type has no parameter. */
  readonly param?: string;
}

export interface AnchorSpec {
  readonly cell: string;
}

export interface TableSpec {
  readonly name?: string;
  readonly style?: string;
  readonly autofilter: boolean;
}

export type DirectiveParseResult<T> =
  | { readonly ok: true; readonly value: T; readonly warnings: readonly string[] }
  | { readonly ok: false; readonly error: string };

const COLUMN_LETTER_RE = /^[A-Za-z]{1,3}$/;
const TYPE_TOKEN_RE = /^([A-Za-z]+)(?:\(([^)]*)\))?$/;
const CELL_REF_RE = /^\$?[A-Za-z]{1,3}\$?\d+$/;

/** `argsRaw` looks like `B:currency, C:percent, D:date(iso)`. */
export function parseColumnsDirective(argsRaw: string): DirectiveParseResult<ReadonlyMap<string, ColumnSpec>> {
  const columns = new Map<string, ColumnSpec>();
  const warnings: string[] = [];
  const tokens = splitArgs(argsRaw);
  if (tokens.length === 0) return { ok: false, error: "columns directive needs at least one COLUMN:type entry." };

  for (const token of tokens) {
    const [letterRaw, ...typeParts] = token.split(":");
    const typeRaw = typeParts.join(":").trim();
    const letter = (letterRaw ?? "").trim().toUpperCase();

    if (!COLUMN_LETTER_RE.test(letter)) {
      return { ok: false, error: `"${token}" isn't a valid COLUMN:type entry (expected a column letter, e.g. "B:currency").` };
    }
    if (typeRaw === "") {
      return { ok: false, error: `Column "${letter}" is missing a type (expected e.g. "${letter}:number").` };
    }

    const typeMatch = TYPE_TOKEN_RE.exec(typeRaw);
    if (!typeMatch) {
      return { ok: false, error: `"${typeRaw}" isn't a valid column type for column "${letter}".` };
    }
    const type = typeMatch[1]!.toLowerCase();
    const param = typeMatch[2]?.trim() || undefined;

    if (!COLUMN_TYPE_SET.has(type)) {
      warnings.push(`Unknown column type "${type}" for column "${letter}"; known types: ${COLUMN_TYPES.join(", ")}.`);
      continue;
    }

    columns.set(letter, { type: type as ColumnType, param });
  }

  return { ok: true, value: columns, warnings };
}

/** `argsRaw` looks like `C5`. */
export function parseAnchorDirective(argsRaw: string): DirectiveParseResult<AnchorSpec> {
  const cell = argsRaw.trim().toUpperCase();
  if (!CELL_REF_RE.test(cell)) {
    return { ok: false, error: `"${argsRaw.trim()}" isn't a valid anchor cell reference (expected e.g. "C5").` };
  }
  return { ok: true, value: { cell }, warnings: [] };
}

const TABLE_KNOWN_KEYS = new Set(["name", "style"]);
const TABLE_KNOWN_FLAGS = new Set(["autofilter"]);

/** `argsRaw` looks like `name=sales_q3, style=striped, autofilter`. */
export function parseTableDirective(argsRaw: string): DirectiveParseResult<TableSpec> {
  const tokens = splitArgs(argsRaw);
  const warnings: string[] = [];
  let name: string | undefined;
  let style: string | undefined;
  let autofilter = false;

  for (const token of tokens) {
    const eq = token.indexOf("=");
    if (eq === -1) {
      const flag = token.toLowerCase();
      if (flag === "autofilter") {
        autofilter = true;
      } else if (TABLE_KNOWN_FLAGS.has(flag)) {
        // reserved for future flags
      } else {
        warnings.push(`Unknown table directive flag "${token}"; ignored.`);
      }
      continue;
    }

    const key = token.slice(0, eq).trim().toLowerCase();
    const value = token.slice(eq + 1).trim();
    if (key === "name") {
      name = value;
    } else if (key === "style") {
      style = value;
    } else if (!TABLE_KNOWN_KEYS.has(key)) {
      warnings.push(`Unknown table directive option "${key}"; ignored.`);
    }
  }

  return { ok: true, value: { name, style, autofilter }, warnings };
}

function splitArgs(argsRaw: string): string[] {
  return argsRaw
    .split(",")
    .map((t) => t.trim())
    .filter((t) => t.length > 0);
}

/** 0-based column index -> spreadsheet letter ("A", "B", ..., "Z", "AA", ...). */
export function columnLetterFromIndex(index: number): string {
  let n = index + 1;
  let letters = "";
  while (n > 0) {
    const rem = (n - 1) % 26;
    letters = String.fromCharCode(65 + rem) + letters;
    n = Math.floor((n - 1) / 26);
  }
  return letters;
}

/** Spreadsheet letter ("A", "B", ..., "AA", ...) -> 0-based column index. */
export function columnIndexFromLetter(letter: string): number {
  let n = 0;
  for (const ch of letter.toUpperCase()) n = n * 26 + (ch.charCodeAt(0) - 64);
  return n - 1;
}

const NUMBER_VALUE_RE = /^-?\d+(?:\.\d+)?$/;
const ISO_DATE_VALUE_RE = /^\d{4}-\d{2}-\d{2}$/;
const BOOLEAN_VALUE_RE = /^(?:true|false)$/i;

export type InferredColumnType = ColumnType | "mixed";

/**
 * Per-column type inference for columns with no `columns` directive entry (§8.4): all-numeric ->
 * number; ISO dates -> date; true/false -> boolean; else text. Formula cells (raw starts with
 * `=`) carry no literal value and are skipped when sampling a column's type. A column whose
 * sampled literal values disagree becomes "mixed" — callers treat that as text and warn.
 */
export function inferColumnType(values: readonly string[]): InferredColumnType {
  let inferred: ColumnType | undefined;
  for (const raw of values) {
    if (raw.startsWith("=") || raw.trim() === "") continue;
    const type: ColumnType = NUMBER_VALUE_RE.test(raw)
      ? "number"
      : ISO_DATE_VALUE_RE.test(raw)
        ? "date"
        : BOOLEAN_VALUE_RE.test(raw)
          ? "boolean"
          : "text";
    if (inferred === undefined) inferred = type;
    else if (inferred !== type) return "mixed";
  }
  return inferred ?? "text";
}
