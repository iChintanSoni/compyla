import { describe, expect, it } from "vitest";
import { buildDocument } from "../src/build-tokens.js";
import { escalateStrict } from "../src/diagnostics.js";
import { parseMarkdown } from "../src/parser.js";
import { validate } from "../src/validate.js";

function parse(source: string) {
  return buildDocument(parseMarkdown(source));
}

describe("validate — docx profile", () => {
  it("warns on a heading level skip", () => {
    const doc = parse(`# One\n\n### Three\n`);
    const diagnostics = validate(doc, { format: "docx" });
    expect(diagnostics).toContainEqual(
      expect.objectContaining({ code: "heading_skip", severity: "warning", line: 3 }),
    );
  });

  it("does not warn when headings step one level at a time", () => {
    const doc = parse(`# One\n\n## Two\n\n### Three\n`);
    const diagnostics = validate(doc, { format: "docx" });
    expect(diagnostics.some((d) => d.code === "heading_skip")).toBe(false);
  });

  it("errors on an unresolved image", () => {
    const doc = parse(`![Missing](missing.png)\n`);
    const diagnostics = validate(doc, { format: "docx", resolveAsset: () => false });
    expect(diagnostics).toContainEqual(
      expect.objectContaining({ code: "image_unresolved", severity: "error" }),
    );
  });

  it("does not error when the asset resolver confirms the image", () => {
    const doc = parse(`![Present](present.png)\n`);
    const diagnostics = validate(doc, { format: "docx", resolveAsset: () => true });
    expect(diagnostics.some((d) => d.code === "image_unresolved")).toBe(false);
  });

  it("warns on an unknown directive", () => {
    const doc = parse(`::: chart bar\n:::\n`);
    const diagnostics = validate(doc, { format: "docx" });
    expect(diagnostics).toContainEqual(expect.objectContaining({ code: "unknown_directive", severity: "warning" }));
  });

  it("does not warn on a known docx directive", () => {
    const doc = parse(`::: toc\n:::\n`);
    const diagnostics = validate(doc, { format: "docx" });
    expect(diagnostics.some((d) => d.code === "unknown_directive")).toBe(false);
  });

  it("carries line numbers through to diagnostics for CI line-referencing", () => {
    const doc = parse(`# One\n\nSome text.\n\n### Skipped\n`);
    const diagnostics = validate(doc, { format: "docx" });
    const skip = diagnostics.find((d) => d.code === "heading_skip");
    expect(skip?.line).toBe(5);
  });

  it("throws a clear error for an unsupported format", () => {
    const doc = parse(`# One\n`);
    expect(() => validate(doc, { format: "pptx" })).toThrow(/Unknown format "pptx"/);
  });
});

describe("escalateStrict", () => {
  it("promotes strictPromotable warnings to errors only when strict is on", () => {
    const diagnostics = [
      { severity: "warning" as const, code: "font_unresolved", line: 1, message: "x", strictPromotable: true },
      { severity: "warning" as const, code: "heading_skip", line: 2, message: "y" },
    ];
    expect(escalateStrict(diagnostics, false).map((d) => d.severity)).toEqual(["warning", "warning"]);
    expect(escalateStrict(diagnostics, true).map((d) => d.severity)).toEqual(["error", "warning"]);
  });
});
