import type { Diagnostic } from "../diagnostics.js";
import { parseFormula } from "../formula.js";
import type { Profile, ValidateOptions } from "../profile.js";
import { groupSheets, isKnownXlsxDirective, type Sheet, type SheetTable } from "../sheets.js";
import type { CompylaDocument, DirectiveNode } from "../types.js";
import { walkBody } from "../walk.js";
import { columnLetterFromIndex, inferColumnType, parseAnchorDirective, parseColumnsDirective, parseTableDirective } from "../xlsx-directives.js";

const KNOWN_DIRECTIVES = ["columns", "anchor", "table"] as const;

export const xlsxProfile: Profile = {
  name: "xlsx",
  knownDirectives: KNOWN_DIRECTIVES,

  validate(doc: CompylaDocument, _options: ValidateOptions): Diagnostic[] {
    const diagnostics: Diagnostic[] = [];

    walkBody(doc.body, 0, {
      onDirective(directive) {
        if (!isKnownXlsxDirective(directive.name)) {
          diagnostics.push({
            severity: "warning",
            code: "unknown_directive",
            line: directive.position.line,
            message: `Unknown directive "${directive.name}" for the xlsx profile; it renders as if absent.`,
            hint: `Known xlsx directives: ${KNOWN_DIRECTIVES.join(", ")}.`,
          });
        }
      },
    });

    const defaultName = typeof doc.meta.title === "string" && doc.meta.title.trim() ? doc.meta.title.trim() : "Sheet1";
    const sheets = groupSheets(doc.body, defaultName);

    diagnostics.push(...validateSheetNames(sheets));

    for (const sheet of sheets) {
      for (const node of sheet.ignored) {
        diagnostics.push({
          severity: "warning",
          code: "xlsx_ignored_content",
          line: node.position.line,
          message: `Content of type "${node.type}" has no xlsx mapping and is ignored.`,
          hint: "xlsx sheets map H1 -> sheet, H2 -> range title, tables -> ranges, paragraphs -> notes. Other content (quotes, code, images, H3+) is dropped.",
        });
      }

      for (const entry of sheet.tables) {
        diagnostics.push(...validateTable(sheet, entry));
      }
    }

    return diagnostics;
  },
};

function validateSheetNames(sheets: readonly Sheet[]): Diagnostic[] {
  const diagnostics: Diagnostic[] = [];
  const seen = new Set<string>();

  for (const sheet of sheets) {
    if (sheet.name.length > 31) {
      diagnostics.push({
        severity: "error",
        code: "sheet_name_too_long",
        line: sheet.namePosition.line,
        message: `Sheet name "${sheet.name}" is ${sheet.name.length} characters; xlsx sheet names must be at most 31.`,
        hint: `Shorten it, e.g. "${sheet.name.slice(0, 31)}".`,
      });
    }

    const key = sheet.name.toLowerCase();
    if (seen.has(key)) {
      diagnostics.push({
        severity: "error",
        code: "duplicate_sheet_name",
        line: sheet.namePosition.line,
        message: `Sheet name "${sheet.name}" is already used by another sheet.`,
        hint: "xlsx sheet names must be unique (case-insensitive). Rename one of the headings.",
      });
    } else {
      seen.add(key);
    }
  }

  return diagnostics;
}

function validateTable(sheet: Sheet, entry: SheetTable): Diagnostic[] {
  const diagnostics: Diagnostic[] = [];
  const { table } = entry;
  const columnCount = table.header.length;

  table.rows.forEach((row, rowIndex) => {
    if (row.length !== columnCount) {
      diagnostics.push({
        severity: "error",
        code: "table_not_rectangular",
        line: table.position.line,
        message: `Table row ${rowIndex + 2} has ${row.length} cells; header defines ${columnCount} columns.`,
        hint: "xlsx sheets require rectangular tables. Add the missing cell or remove the extra column.",
      });
    }
  });

  let columnLetters: ReadonlySet<string> | undefined;
  if (entry.columnsDirective) {
    const parsed = parseColumnsDirective(entry.columnsDirective.argsRaw);
    if (!parsed.ok) {
      diagnostics.push(directiveError(entry.columnsDirective, parsed.error));
    } else {
      columnLetters = new Set(parsed.value.keys());
      for (const warning of parsed.warnings) diagnostics.push(directiveWarning(entry.columnsDirective, warning));
    }
  }

  if (entry.anchorDirective) {
    const parsed = parseAnchorDirective(entry.anchorDirective.argsRaw);
    if (!parsed.ok) diagnostics.push(directiveError(entry.anchorDirective, parsed.error));
  }

  if (entry.tableDirective) {
    const parsed = parseTableDirective(entry.tableDirective.argsRaw);
    if (!parsed.ok) {
      diagnostics.push(directiveError(entry.tableDirective, parsed.error));
    } else {
      for (const warning of parsed.warnings) diagnostics.push(directiveWarning(entry.tableDirective, warning));
    }
  }

  for (let col = 0; col < columnCount; col++) {
    const letter = columnLetterFromIndex(col);
    if (columnLetters?.has(letter)) continue;
    const values = table.rows.map((row) => row[col]?.raw ?? "");
    if (inferColumnType(values) === "mixed") {
      diagnostics.push({
        severity: "warning",
        code: "mixed_column_type",
        line: table.position.line,
        message: `Column ${letter} in sheet "${sheet.name}" has mixed value types; treated as text.`,
        hint: `Add a "::: columns ${letter}:<type>" directive above this table to force a type.`,
      });
    }
  }

  table.rows.forEach((row, rowIndex) => {
    row.forEach((cell, col) => {
      if (!cell.raw.startsWith("=")) return;
      const result = parseFormula(cell.raw.slice(1));
      if (!result.ok) {
        diagnostics.push({
          severity: "error",
          code: "formula_error",
          line: table.position.line,
          message: `${columnLetterFromIndex(col)}${rowIndex + 2} in sheet "${sheet.name}": ${result.error}`,
          hint: "Formulas are parsed and checked against a function allowlist (markdown-spec.md §8.2).",
        });
      }
    });
  });

  return diagnostics;
}

function directiveError(directive: DirectiveNode, error: string): Diagnostic {
  return {
    severity: "error",
    code: "invalid_directive_args",
    line: directive.position.line,
    message: `Invalid "${directive.name}" directive: ${error}`,
  };
}

function directiveWarning(directive: DirectiveNode, message: string): Diagnostic {
  return {
    severity: "warning",
    code: "unknown_directive_option",
    line: directive.position.line,
    message: `"${directive.name}" directive: ${message}`,
  };
}
