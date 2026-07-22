/**
 * Formula parsing for the xlsx profile (markdown-spec.md §8.2). A cell whose raw content starts
 * with `=` is a formula cell; this module tokenizes the formula and checks every function call
 * against an allowlist. It's a tokenizer, not a full grammar — enough to (a) find every function
 * call and reject anything not on the allowlist (closing the WEBSERVICE/RTD/HYPERLINK/DDE
 * injection hole by construction, since unlisted names are rejected whether or not they're
 * "known" attack vectors) and (b) reject any character that doesn't belong in a spreadsheet
 * formula at all (e.g. `[` `]` from external-workbook references, or stray punctuation).
 *
 * Runs offline in `@compyla/core` (no `exceljs` dependency) so `compyla lint --format xlsx`
 * doesn't require the renderer package to be installed.
 */

export const FORMULA_FUNCTION_ALLOWLIST: ReadonlySet<string> = new Set([
  "SUM", "SUMIF", "SUMIFS",
  "AVERAGE", "AVERAGEIF", "AVERAGEIFS",
  "COUNT", "COUNTA", "COUNTBLANK", "COUNTIF", "COUNTIFS",
  "MIN", "MAX", "MEDIAN",
  "IF", "IFS", "IFERROR", "IFNA",
  "AND", "OR", "NOT", "XOR",
  "VLOOKUP", "HLOOKUP", "INDEX", "MATCH", "LOOKUP",
  "ROUND", "ROUNDUP", "ROUNDDOWN", "ABS", "SQRT", "POWER", "MOD", "INT", "TRUNC",
  "CONCATENATE", "CONCAT", "TEXT", "LEFT", "RIGHT", "MID", "LEN", "TRIM", "UPPER", "LOWER", "SUBSTITUTE",
  "DATE", "YEAR", "MONTH", "DAY", "TODAY", "NOW", "DATEDIF",
  "ISBLANK", "ISNUMBER", "ISTEXT", "ISERROR", "ISNA",
]);

const BOOLEAN_LITERALS = new Set(["TRUE", "FALSE"]);

const REF_RE = /^(?:(?:'[^']*'|[A-Za-z_]\w*)!)?\$?[A-Za-z]{1,3}\$?\d+(?::(?:(?:'[^']*'|[A-Za-z_]\w*)!)?\$?[A-Za-z]{1,3}\$?\d+)?/;
const NUMBER_RE = /^\d+(?:\.\d+)?(?:[eE][+-]?\d+)?/;
const IDENT_RE = /^[A-Za-z_][A-Za-z0-9_.]*/;
const SIMPLE_TOKEN_CHARS = "+-*/^%&=(),:";

export interface FormulaParseSuccess {
  readonly ok: true;
  /** Distinct allowlisted function names the formula calls, uppercased. */
  readonly functions: readonly string[];
  /** Distinct cell/range references the formula contains, as written. */
  readonly refs: readonly string[];
}

export interface FormulaParseError {
  readonly ok: false;
  readonly error: string;
}

export type FormulaParseResult = FormulaParseSuccess | FormulaParseError;

/** `raw` is the cell's full content, including the leading `=`. */
export function isFormulaCell(raw: string): boolean {
  return raw.startsWith("=");
}

/** Strips the `\` escape from a literal cell that would otherwise start with `=` (§8.2). */
export function stripFormulaEscape(raw: string): string {
  return raw.startsWith("\\=") ? raw.slice(1) : raw;
}

/** `body` is the formula content *after* the leading `=`. */
export function parseFormula(body: string): FormulaParseResult {
  let i = 0;
  const functions = new Set<string>();
  const refs = new Set<string>();

  while (i < body.length) {
    const ch = body[i]!;

    if (/\s/.test(ch)) {
      i++;
      continue;
    }

    if (ch === "<" && (body[i + 1] === "=" || body[i + 1] === ">")) {
      i += 2;
      continue;
    }
    if (ch === ">" && body[i + 1] === "=") {
      i += 2;
      continue;
    }
    if (SIMPLE_TOKEN_CHARS.includes(ch) || ch === "<" || ch === ">") {
      i++;
      continue;
    }

    if (ch === '"') {
      const closeAt = findStringEnd(body, i + 1);
      if (closeAt === -1) return { ok: false, error: "Unterminated string literal in formula." };
      i = closeAt + 1;
      continue;
    }

    const rest = body.slice(i);

    const refMatch = REF_RE.exec(rest);
    if (refMatch) {
      refs.add(refMatch[0]);
      i += refMatch[0].length;
      continue;
    }

    const numberMatch = NUMBER_RE.exec(rest);
    if (numberMatch) {
      i += numberMatch[0].length;
      continue;
    }

    const identMatch = IDENT_RE.exec(rest);
    if (identMatch) {
      const name = identMatch[0];
      i += name.length;

      let lookaheadEnd = i;
      while (lookaheadEnd < body.length && /\s/.test(body[lookaheadEnd]!)) lookaheadEnd++;
      const isCall = body[lookaheadEnd] === "(";
      const upper = name.toUpperCase();

      if (isCall) {
        if (!FORMULA_FUNCTION_ALLOWLIST.has(upper)) {
          return { ok: false, error: `Disallowed function "${name}" in formula.` };
        }
        functions.add(upper);
        continue;
      }

      if (BOOLEAN_LITERALS.has(upper)) continue;

      return { ok: false, error: `Unrecognized name "${name}" in formula.` };
    }

    return { ok: false, error: `Unexpected character "${ch}" in formula.` };
  }

  return { ok: true, functions: [...functions], refs: [...refs] };
}

function findStringEnd(body: string, from: number): number {
  let j = from;
  while (j < body.length) {
    if (body[j] === '"') {
      if (body[j + 1] === '"') {
        j += 2;
        continue;
      }
      return j;
    }
    j++;
  }
  return -1;
}
