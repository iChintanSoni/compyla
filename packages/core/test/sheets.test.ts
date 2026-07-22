import { describe, expect, it } from "vitest";
import { buildDocument } from "../src/build-tokens.js";
import { parseMarkdown } from "../src/parser.js";
import { groupSheets } from "../src/sheets.js";

function parse(source: string) {
  return buildDocument(parseMarkdown(source));
}

describe("groupSheets", () => {
  it("returns no sheets for an empty document", () => {
    expect(groupSheets([], "Sheet1")).toEqual([]);
  });

  it("puts content before the first H1 into a default sheet", () => {
    const doc = parse(`Intro paragraph.\n\n| A | B |\n|---|---|\n| 1 | 2 |\n\n# Revenue\n\n| C | D |\n|---|---|\n| 3 | 4 |\n`);
    const sheets = groupSheets(doc.body, "Sheet1");
    expect(sheets.map((s) => s.name)).toEqual(["Sheet1", "Revenue"]);
    expect(sheets[0]?.tables).toHaveLength(1);
    expect(sheets[0]?.tables[0]?.note?.runs[0]?.text).toBe("Intro paragraph.");
  });

  it("creates one sheet per H1, named from the heading text", () => {
    const doc = parse(`# Revenue\n\n| A |\n|---|\n| 1 |\n\n# Expenses\n\n| B |\n|---|\n| 2 |\n`);
    const sheets = groupSheets(doc.body, "Sheet1");
    expect(sheets.map((s) => s.name)).toEqual(["Revenue", "Expenses"]);
  });

  it("attaches an H2 as the range title of the next table", () => {
    const doc = parse(`# Revenue\n\n## By Region\n\n| A |\n|---|\n| 1 |\n`);
    const sheets = groupSheets(doc.body, "Sheet1");
    expect(sheets[0]?.tables[0]?.rangeTitle?.[0]?.text).toBe("By Region");
  });

  it("attaches columns/anchor/table directives to the next table and resets after", () => {
    const doc = parse(
      `# Budget\n\n::: columns B:currency\n:::\n\n::: anchor F1\n:::\n\n| A | B |\n|---|---|\n| x | 1 |\n\n| C |\n|---|\n| 2 |\n`,
    );
    const sheets = groupSheets(doc.body, "Sheet1");
    const [first, second] = sheets[0]!.tables;
    expect(first?.columnsDirective?.name).toBe("columns");
    expect(first?.anchorDirective?.name).toBe("anchor");
    expect(second?.columnsDirective).toBeUndefined();
    expect(second?.anchorDirective).toBeUndefined();
  });

  it("collects the table directive the same way as columns/anchor", () => {
    const doc = parse(`# Budget\n\n::: table name=opex, autofilter\n:::\n\n| A |\n|---|\n| 1 |\n`);
    const sheets = groupSheets(doc.body, "Sheet1");
    expect(sheets[0]?.tables[0]?.tableDirective?.argsRaw).toBe("name=opex, autofilter");
  });

  it("puts quotes, code blocks, and media into the ignored list", () => {
    const doc = parse(`# Notes\n\n> A quote\n\n\`\`\`\ncode\n\`\`\`\n\n![alt](img.png)\n`);
    const sheets = groupSheets(doc.body, "Sheet1");
    expect(sheets[0]?.ignored.map((n) => n.type).sort()).toEqual(["code", "media", "quote"]);
  });

  it("leaves a table with no preceding annotations with all fields undefined", () => {
    const doc = parse(`# Plain\n\n| A |\n|---|\n| 1 |\n`);
    const entry = groupSheets(doc.body, "Sheet1")[0]!.tables[0]!;
    expect(entry.rangeTitle).toBeUndefined();
    expect(entry.note).toBeUndefined();
    expect(entry.columnsDirective).toBeUndefined();
  });
});
