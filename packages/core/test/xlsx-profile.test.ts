import { describe, expect, it } from "vitest";
import { buildDocument } from "../src/build-tokens.js";
import { parseMarkdown } from "../src/parser.js";
import { validate } from "../src/validate.js";

function parse(source: string) {
  return buildDocument(parseMarkdown(source));
}

describe("validate — xlsx profile", () => {
  it("produces no diagnostics for a well-formed budget-style document", () => {
    const doc = parse(
      `# Revenue\n\n## By Region\n\n::: columns B:currency, C:currency, D:percent\n:::\n\n` +
        `| Region | Actual | Budget | Variance |\n|---|---|---|---|\n` +
        `| APAC | 120000 | 110000 | =B2/C2-1 |\n| Total | =SUM(B2:B2) | =SUM(C2:C2) | =B3/C3-1 |\n`,
    );
    expect(validate(doc, { format: "xlsx" })).toEqual([]);
  });

  it("warns on an unknown directive", () => {
    const doc = parse(`# Sheet\n\n::: chart bar\n:::\n\n| A |\n|---|\n| 1 |\n`);
    const diagnostics = validate(doc, { format: "xlsx" });
    expect(diagnostics).toContainEqual(expect.objectContaining({ code: "unknown_directive", severity: "warning" }));
  });

  it("errors on a duplicate sheet name", () => {
    const doc = parse(`# Sales\n\n| A |\n|---|\n| 1 |\n\n# Sales\n\n| B |\n|---|\n| 2 |\n`);
    const diagnostics = validate(doc, { format: "xlsx" });
    expect(diagnostics).toContainEqual(expect.objectContaining({ code: "duplicate_sheet_name", severity: "error" }));
  });

  it("errors on a sheet name over 31 characters", () => {
    const doc = parse(`# This Heading Is Deliberately Way Too Long For A Sheet\n\n| A |\n|---|\n| 1 |\n`);
    const diagnostics = validate(doc, { format: "xlsx" });
    expect(diagnostics).toContainEqual(expect.objectContaining({ code: "sheet_name_too_long", severity: "error" }));
  });

  it("errors on a disallowed formula function", () => {
    const doc = parse(`# Sheet\n\n| A |\n|---|\n| =WEBSERVICE("http://evil.example") |\n`);
    const diagnostics = validate(doc, { format: "xlsx" });
    const err = diagnostics.find((d) => d.code === "formula_error");
    expect(err?.severity).toBe("error");
    expect(err?.message).toContain("WEBSERVICE");
  });

  it("warns on a mixed-type column with no columns directive", () => {
    const doc = parse(`# Sheet\n\n| A |\n|---|\n| 120000 |\n| EMEA |\n`);
    const diagnostics = validate(doc, { format: "xlsx" });
    expect(diagnostics).toContainEqual(expect.objectContaining({ code: "mixed_column_type", severity: "warning" }));
  });

  it("does not warn mixed-type for a column covered by a columns directive", () => {
    const doc = parse(`# Sheet\n\n::: columns A:text\n:::\n\n| A |\n|---|\n| 120000 |\n| EMEA |\n`);
    const diagnostics = validate(doc, { format: "xlsx" });
    expect(diagnostics.some((d) => d.code === "mixed_column_type")).toBe(false);
  });

  it("errors on an invalid anchor directive", () => {
    const doc = parse(`# Sheet\n\n::: anchor not-a-cell\n:::\n\n| A |\n|---|\n| 1 |\n`);
    const diagnostics = validate(doc, { format: "xlsx" });
    expect(diagnostics).toContainEqual(expect.objectContaining({ code: "invalid_directive_args", severity: "error" }));
  });

  it("warns on ignored content (a quote block has no xlsx mapping)", () => {
    const doc = parse(`# Sheet\n\n> A quote\n\n| A |\n|---|\n| 1 |\n`);
    const diagnostics = validate(doc, { format: "xlsx" });
    expect(diagnostics).toContainEqual(expect.objectContaining({ code: "xlsx_ignored_content", severity: "warning" }));
  });
});
