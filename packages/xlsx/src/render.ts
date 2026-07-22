import type { CompylaDocument, Diagnostic, Sheet, SheetTable } from "@compyla/core";
import {
  columnIndexFromLetter,
  columnLetterFromIndex,
  groupSheets,
  inferColumnType,
  isFormulaCell,
  parseAnchorDirective,
  parseColumnsDirective,
  parseFormula,
  parseTableDirective,
  stripFormulaEscape,
  type ColumnSpec,
  type ColumnType,
} from "@compyla/core";
import { repackageDeterministically } from "@compyla/ooxml";
import ExcelJS from "exceljs";

export interface RenderOptions {
  /** Accepted for interface parity with the other renderers; xlsx template binding isn't
   *  implemented yet (see roadmap) — a house style is used instead, with a warning diagnostic. */
  readonly template?: Buffer;
  readonly resolveAsset?: (src: string) => Promise<Buffer> | Buffer;
}

export interface RenderResult {
  readonly buffer: Buffer;
  readonly diagnostics: readonly Diagnostic[];
}

const HEADER_FILL: ExcelJS.Fill = { type: "pattern", pattern: "solid", fgColor: { argb: "FFE8ECF3" } };
const NUMBER_FORMATS: Partial<Record<ColumnType, string>> = {
  currency: '"$"#,##0.00',
  percent: "0.0%",
  date: "yyyy-mm-dd",
  number: "#,##0",
};
const NUMBER_VALUE_RE = /^-?\d+(?:\.\d+)?$/;
const ISO_DATE_VALUE_RE = /^\d{4}-\d{2}-\d{2}$/;
const CELL_REF_RE = /^\$?([A-Za-z]{1,3})\$?(\d+)$/;
const TABLE_THEME_BY_STYLE: Record<string, ExcelJS.TableStyleProperties["theme"]> = {
  striped: "TableStyleMedium2",
  plain: "TableStyleLight1",
};

export async function renderXlsx(doc: CompylaDocument, options: RenderOptions = {}): Promise<RenderResult> {
  const diagnostics: Diagnostic[] = [];
  if (options.template) {
    diagnostics.push({
      severity: "warning",
      code: "xlsx_template_unsupported",
      line: 1,
      message: "xlsx template binding isn't supported yet; the built-in house style is used instead.",
    });
  }

  const workbook = new ExcelJS.Workbook();
  if (doc.meta.author) workbook.creator = String(doc.meta.author);

  const defaultName = typeof doc.meta.title === "string" && doc.meta.title.trim() ? doc.meta.title.trim() : "Sheet1";
  const sheets = groupSheets(doc.body, defaultName);

  let tableCounter = 0;
  for (const sheet of sheets) {
    const worksheet = workbook.addWorksheet(safeSheetName(sheet.name));
    let cursorRow = 1;
    for (const entry of sheet.tables) {
      const placedRow = renderTable(worksheet, sheet, entry, cursorRow, () => `xlsxTable${++tableCounter}`, diagnostics);
      if (placedRow !== undefined) cursorRow = placedRow;
    }
  }

  // exceljs's own @types/node instance resolves to a structurally-identical but nominally
  // distinct Buffer type under pnpm's isolated dependency layout; a boundary cast is the
  // pragmatic fix (same rationale as pptxgenjs's boundary cast in @compyla/pptx's render.ts).
  const raw = (await workbook.xlsx.writeBuffer()) as unknown as Buffer;
  const buffer = await repackageDeterministically(raw);
  return { buffer, diagnostics };
}

/** Renders one table (title/note/header/data) and returns the next stacking cursor row — or
 *  `undefined` when the table was placed by an `anchor` directive, which doesn't advance the
 *  stacking cursor for whatever comes after it (markdown-spec.md §8.3's "instead of stacking"). */
function renderTable(
  worksheet: ExcelJS.Worksheet,
  sheet: Sheet,
  entry: SheetTable,
  cursorRow: number,
  nextTableName: () => string,
  diagnostics: Diagnostic[],
): number | undefined {
  const { table } = entry;
  const columnCount = table.header.length;

  let startRow = cursorRow;
  let startCol = 0;
  let anchored = false;

  if (entry.anchorDirective) {
    const parsed = parseAnchorDirective(entry.anchorDirective.argsRaw);
    if (parsed.ok) {
      const ref = CELL_REF_RE.exec(parsed.value.cell);
      if (ref) {
        startCol = columnIndexFromLetter(ref[1]!);
        startRow = Number(ref[2]);
        anchored = true;
      }
    }
  }

  let columnSpecs: ReadonlyMap<string, ColumnSpec> | undefined;
  if (entry.columnsDirective) {
    const parsed = parseColumnsDirective(entry.columnsDirective.argsRaw);
    if (parsed.ok) columnSpecs = parsed.value;
  }

  const columnTypes: ColumnType[] = [];
  for (let col = 0; col < columnCount; col++) {
    const letter = columnLetterFromIndex(col);
    const spec = columnSpecs?.get(letter);
    if (spec) {
      columnTypes.push(spec.type);
      continue;
    }
    const values = table.rows.map((row) => row[col]?.raw ?? "");
    const inferred = inferColumnType(values);
    columnTypes.push(inferred === "mixed" ? "text" : inferred);
  }

  let row = startRow;
  if (entry.rangeTitle) {
    writeLabelRow(worksheet, row, startCol, columnCount, plainText(entry.rangeTitle), { bold: true });
    row++;
  }
  if (entry.note) {
    writeLabelRow(worksheet, row, startCol, columnCount, plainText(entry.note.runs), { italic: true });
    row++;
  }

  const headerRow = row;
  for (let col = 0; col < columnCount; col++) {
    const cell = worksheet.getCell(headerRow, startCol + col + 1);
    cell.value = plainText(table.header[col]?.runs ?? []);
    cell.font = { bold: true };
    cell.fill = HEADER_FILL;
  }

  table.rows.forEach((tableRow, rowIndex) => {
    const dataRow = headerRow + 1 + rowIndex;
    for (let col = 0; col < columnCount; col++) {
      const raw = tableRow[col]?.raw ?? "";
      const cell = worksheet.getCell(dataRow, startCol + col + 1);
      cell.value = cellValueFor(raw, columnTypes[col]!, sheet, headerRow, col, rowIndex, diagnostics);
      const fmt = NUMBER_FORMATS[columnTypes[col]!];
      if (fmt) cell.numFmt = fmt;
    }
  });

  for (let col = 0; col < columnCount; col++) {
    const headerText = plainText(table.header[col]?.runs ?? []);
    const widest = table.rows.reduce((max, r) => Math.max(max, (r[col]?.raw ?? "").length), headerText.length);
    worksheet.getColumn(startCol + col + 1).width = Math.min(40, Math.max(8, widest + 2));
  }

  if (entry.tableDirective) {
    const parsed = parseTableDirective(entry.tableDirective.argsRaw);
    if (parsed.ok) {
      worksheet.addTable({
        name: sanitizeTableName(parsed.value.name) ?? nextTableName(),
        ref: `${columnLetterFromIndex(startCol)}${headerRow}`,
        headerRow: true,
        style: {
          theme: (parsed.value.style ? TABLE_THEME_BY_STYLE[parsed.value.style] : undefined) ?? "TableStyleMedium2",
          showRowStripes: true,
        },
        columns: table.header.map((_, col) => ({
          name: plainText(table.header[col]?.runs ?? []),
          filterButton: parsed.value.autofilter,
        })),
        rows: table.rows.map((tableRow, rowIndex) =>
          tableRow.map((_, col) =>
            cellValueFor(tableRow[col]?.raw ?? "", columnTypes[col]!, sheet, headerRow, col, rowIndex, []),
          ),
        ),
      });
    }
  }

  if (anchored) return undefined;
  return headerRow + table.rows.length + 2;
}

function cellValueFor(
  raw: string,
  type: ColumnType,
  sheet: Sheet,
  headerRow: number,
  col: number,
  rowIndex: number,
  diagnostics: Diagnostic[],
): ExcelJS.CellValue {
  if (isFormulaCell(raw)) {
    const result = parseFormula(raw.slice(1));
    if (result.ok) return { formula: raw.slice(1) };
    diagnostics.push({
      severity: "error",
      code: "formula_error",
      line: headerRow,
      message: `${columnLetterFromIndex(col)}${rowIndex + 2} in sheet "${sheet.name}": ${result.error}`,
      hint: "Formulas are parsed and checked against a function allowlist (markdown-spec.md §8.2).",
    });
    return raw;
  }

  const text = stripFormulaEscape(raw);
  switch (type) {
    case "number":
    case "currency":
    case "percent":
      return NUMBER_VALUE_RE.test(text) ? Number(text) : text;
    case "date":
      return ISO_DATE_VALUE_RE.test(text) ? new Date(text) : text;
    case "boolean":
      return /^true$/i.test(text) ? true : /^false$/i.test(text) ? false : text;
    default:
      return text;
  }
}

function writeLabelRow(
  worksheet: ExcelJS.Worksheet,
  row: number,
  startCol: number,
  columnCount: number,
  text: string,
  font: Partial<ExcelJS.Font>,
): void {
  const cell = worksheet.getCell(row, startCol + 1);
  cell.value = text;
  cell.font = font;
  if (columnCount > 1) {
    worksheet.mergeCells(row, startCol + 1, row, startCol + columnCount);
  }
}

function plainText(runs: readonly { text: string }[]): string {
  return runs.map((r) => r.text).join("");
}

const FORBIDDEN_SHEET_CHARS = /[\\/*?:[\]]/g;

function safeSheetName(name: string): string {
  return name.replace(FORBIDDEN_SHEET_CHARS, "-").slice(0, 31) || "Sheet1";
}

function sanitizeTableName(name: string | undefined): string | undefined {
  if (!name) return undefined;
  const cleaned = name.replace(/[^A-Za-z0-9_]/g, "_");
  return /^[A-Za-z_]/.test(cleaned) ? cleaned : `t_${cleaned}`;
}
