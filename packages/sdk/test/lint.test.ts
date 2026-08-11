import { describe, expect, it } from "vitest";
import { lint } from "../src/lint.js";

describe("lint", () => {
  it("reports ok for a clean document", () => {
    const result = lint("# Title\n\nSome body text.\n", { format: "docx" });
    expect(result.ok).toBe(true);
    expect(result.diagnostics).toEqual([]);
  });

  it("reports a warning (not an error) for a heading skip, and stays ok", () => {
    const result = lint("# Title\n\n### Skipped to h3\n", { format: "docx" });
    expect(result.diagnostics.some((d) => d.code === "heading_skip" && d.severity === "warning")).toBe(true);
    expect(result.ok).toBe(true);
  });

  it("blocks on an unresolved image (error severity)", () => {
    const result = lint("# Title\n\n![missing](missing.png)\n", { format: "docx", resolveAsset: () => false });
    expect(result.ok).toBe(false);
    expect(result.diagnostics.some((d) => d.code === "image_unresolved")).toBe(true);
  });

  it("escalates strict-promotable warnings (e.g. pptx layout_overflow) to errors under --strict", () => {
    const source = "## Title\n\n::: layout Title and Content\nA paragraph.\n\n| A | B |\n|---|---|\n| 1 | 2 |\n:::\n";
    const loose = lint(source, { format: "pptx" });
    const strict = lint(source, { format: "pptx", strict: true });
    expect(loose.diagnostics.find((d) => d.code === "layout_overflow")?.severity).toBe("warning");
    expect(loose.ok).toBe(true);
    expect(strict.diagnostics.find((d) => d.code === "layout_overflow")?.severity).toBe("error");
    expect(strict.ok).toBe(false);
  });
});
