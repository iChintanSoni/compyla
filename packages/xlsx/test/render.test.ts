import { buildDocument, parseMarkdown } from "@compyla/core";
import ExcelJS from "exceljs";
import { describe, expect, it } from "vitest";
import { renderXlsx } from "../src/render.js";

function parse(source: string) {
  return buildDocument(parseMarkdown(source));
}

async function load(buffer: Buffer): Promise<ExcelJS.Workbook> {
  const workbook = new ExcelJS.Workbook();
  await workbook.xlsx.load(buffer);
  return workbook;
}

const BUDGET_MD = `# Revenue

## By Region

::: columns B:currency, C:currency, D:percent
:::

| Region | Actual | Budget | Variance    |
|--------|--------|--------|-------------|
| APAC   | 120000 | 110000 | =B2/C2-1    |
| Total  | =SUM(B2:B2) | =SUM(C2:C2) | =B3/C3-1 |

# Expenses

## Operating Costs

::: table name=opex, style=striped, autofilter
:::

| Category  | Q3 Actual | Q4 Budget |
|-----------|-----------|-----------|
| Payroll   | 450000    | 470000    |
| Total     | =SUM(B2:B2) | =SUM(C2:C2) |

## Headcount

Snapshot as of the last day of the quarter.

::: anchor F1
:::

| Metric    | Value |
|-----------|-------|
| Headcount | 42    |
`;

describe("renderXlsx — byte reproducibility", () => {
  it("produces byte-identical output for identical input across two runs", async () => {
    const doc = parse(BUDGET_MD);
    const first = await renderXlsx(doc);
    await new Promise((r) => setTimeout(r, 1100));
    const second = await renderXlsx(doc);
    expect(Buffer.compare(first.buffer, second.buffer)).toBe(0);
  });
});

describe("renderXlsx — sheets", () => {
  it("creates one sheet per H1, named from the heading", async () => {
    const { buffer } = await renderXlsx(parse(BUDGET_MD));
    const workbook = await load(buffer);
    expect(workbook.worksheets.map((w) => w.name)).toEqual(["Revenue", "Expenses"]);
  });

  it("uses meta.title (or Sheet1) for content before the first H1", async () => {
    const doc = parse(`---\ntitle: My Budget\n---\n\nIntro.\n\n| A |\n|---|\n| 1 |\n\n# Detail\n\n| B |\n|---|\n| 2 |\n`);
    const { buffer } = await renderXlsx(doc);
    const workbook = await load(buffer);
    expect(workbook.worksheets.map((w) => w.name)).toEqual(["My Budget", "Detail"]);
  });
});

describe("renderXlsx — table layout", () => {
  it("writes a bold header row", async () => {
    const { buffer } = await renderXlsx(parse(BUDGET_MD));
    const workbook = await load(buffer);
    const revenue = workbook.getWorksheet("Revenue")!;
    const header = revenue.getRow(2); // row 1 is the "By Region" range title
    expect(header.getCell(1).value).toBe("Region");
    expect(header.getCell(1).font?.bold).toBe(true);
  });

  it("writes formula cells as real formulas, not literal text", async () => {
    const { buffer } = await renderXlsx(parse(BUDGET_MD));
    const workbook = await load(buffer);
    const revenue = workbook.getWorksheet("Revenue")!;
    const variance = revenue.getRow(3).getCell(4).value as ExcelJS.CellFormulaValue;
    expect(variance.formula).toBe("B2/C2-1");
  });

  it("applies the columns directive's number format to a formula-computed cell", async () => {
    const { buffer } = await renderXlsx(parse(BUDGET_MD));
    const workbook = await load(buffer);
    const revenue = workbook.getWorksheet("Revenue")!;
    expect(revenue.getRow(3).getCell(4).numFmt).toContain("%");
    expect(revenue.getRow(3).getCell(2).numFmt).toContain("$");
  });

  it("places an anchored table's block (range title, note, header, data) starting at its directive cell instead of stacking", async () => {
    const { buffer } = await renderXlsx(parse(BUDGET_MD));
    const workbook = await load(buffer);
    const expenses = workbook.getWorksheet("Expenses")!;
    // F1 = range title ("Headcount"), F2 = note, F3 = header, F4 = data — all anchored at F, not
    // stacked under the "Operating Costs" table in column A.
    expect(expenses.getCell("F1").value).toBe("Headcount");
    expect(expenses.getCell("F3").value).toBe("Metric");
    expect(expenses.getCell("F4").value).toBe("Headcount");
    expect(expenses.getCell("G4").value).toBe(42);
  });

  it("registers a table directive as a named Excel table with autofilter", async () => {
    const { buffer } = await renderXlsx(parse(BUDGET_MD));
    const workbook = await load(buffer);
    const expenses = workbook.getWorksheet("Expenses")!;
    const table = expenses.getTable("opex");
    expect(table.name).toBe("opex");
    // exceljs's Table wrapper only proxies a subset of fields (e.g. `name`) through getters after
    // a round-trip load; columns are readable via the underlying model it wraps.
    const model = (table as unknown as { table: ExcelJS.TableProperties }).table;
    expect(model.columns[0]?.filterButton).toBe(true);
  });
});

describe("renderXlsx — formula safety net", () => {
  it("falls back to literal text and emits a diagnostic for a disallowed formula", async () => {
    const doc = parse(`# Sheet\n\n| A |\n|---|\n| =WEBSERVICE("http://evil.example") |\n`);
    const { buffer, diagnostics } = await renderXlsx(doc);
    const workbook = await load(buffer);
    const cell = workbook.getWorksheet("Sheet")!.getRow(2).getCell(1);
    expect(typeof cell.value).toBe("string");
    expect(diagnostics).toContainEqual(expect.objectContaining({ code: "formula_error", severity: "error" }));
  });
});

describe("renderXlsx — template option", () => {
  it("warns that xlsx template binding isn't supported yet", async () => {
    const doc = parse(`# Sheet\n\n| A |\n|---|\n| 1 |\n`);
    const { diagnostics } = await renderXlsx(doc, { template: Buffer.from("fake") });
    expect(diagnostics).toContainEqual(expect.objectContaining({ code: "xlsx_template_unsupported", severity: "warning" }));
  });
});
