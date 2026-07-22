import { describe, expect, it } from "vitest";
import {
  columnIndexFromLetter,
  columnLetterFromIndex,
  inferColumnType,
  parseAnchorDirective,
  parseColumnsDirective,
  parseTableDirective,
} from "../src/xlsx-directives.js";

describe("columnLetterFromIndex / columnIndexFromLetter", () => {
  it("round-trips single and double letters", () => {
    expect(columnLetterFromIndex(0)).toBe("A");
    expect(columnLetterFromIndex(25)).toBe("Z");
    expect(columnLetterFromIndex(26)).toBe("AA");
    expect(columnIndexFromLetter("A")).toBe(0);
    expect(columnIndexFromLetter("AA")).toBe(26);
    expect(columnIndexFromLetter(columnLetterFromIndex(52))).toBe(52);
  });
});

describe("parseColumnsDirective", () => {
  it("parses multiple COLUMN:type entries, including a parameterized type", () => {
    const result = parseColumnsDirective("B:currency, C:percent, D:date(iso)");
    expect(result.ok).toBe(true);
    if (!result.ok) return;
    expect(result.value.get("B")).toEqual({ type: "currency", param: undefined });
    expect(result.value.get("D")).toEqual({ type: "date", param: "iso" });
    expect(result.warnings).toEqual([]);
  });

  it("errors on an empty directive", () => {
    expect(parseColumnsDirective("").ok).toBe(false);
  });

  it("errors on a malformed entry", () => {
    expect(parseColumnsDirective("not-a-column").ok).toBe(false);
  });

  it("warns (not errors) on an unknown column type", () => {
    const result = parseColumnsDirective("B:frobnicate");
    expect(result.ok).toBe(true);
    if (!result.ok) return;
    expect(result.value.has("B")).toBe(false);
    expect(result.warnings[0]).toContain("frobnicate");
  });
});

describe("parseAnchorDirective", () => {
  it("parses a valid cell reference", () => {
    const result = parseAnchorDirective("C5");
    expect(result).toEqual({ ok: true, value: { cell: "C5" }, warnings: [] });
  });

  it("errors on an invalid cell reference", () => {
    expect(parseAnchorDirective("not a cell").ok).toBe(false);
  });
});

describe("parseTableDirective", () => {
  it("parses name/style key-values and the autofilter flag", () => {
    const result = parseTableDirective("name=sales_q3, style=striped, autofilter");
    expect(result.ok).toBe(true);
    if (!result.ok) return;
    expect(result.value).toEqual({ name: "sales_q3", style: "striped", autofilter: true });
  });

  it("defaults autofilter to false and allows a bare name", () => {
    const result = parseTableDirective("name=sales_q3");
    expect(result.ok).toBe(true);
    if (!result.ok) return;
    expect(result.value.autofilter).toBe(false);
  });

  it("warns (not errors) on an unknown option", () => {
    const result = parseTableDirective("color=blue");
    expect(result.ok).toBe(true);
    if (!result.ok) return;
    expect(result.warnings[0]).toContain("color");
  });
});

describe("inferColumnType", () => {
  it("infers number from all-numeric values", () => {
    expect(inferColumnType(["120000", "98000", "42"])).toBe("number");
  });

  it("infers date from ISO date values", () => {
    expect(inferColumnType(["2024-01-01", "2024-02-15"])).toBe("date");
  });

  it("infers boolean from true/false values", () => {
    expect(inferColumnType(["true", "FALSE"])).toBe("boolean");
  });

  it("infers text as the default and the mixed-type fallback", () => {
    expect(inferColumnType(["APAC", "EMEA"])).toBe("text");
    expect(inferColumnType(["120000", "EMEA"])).toBe("mixed");
  });

  it("skips formula cells and blanks when sampling", () => {
    expect(inferColumnType(["120000", "=SUM(B2:B3)", "", "98000"])).toBe("number");
  });

  it("defaults to text when every value is a formula or blank", () => {
    expect(inferColumnType(["=SUM(B2:B3)", ""])).toBe("text");
  });
});
