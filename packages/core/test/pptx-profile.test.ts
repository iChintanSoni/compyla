import { describe, expect, it } from "vitest";
import { buildDocument } from "../src/build-tokens.js";
import { escalateStrict } from "../src/diagnostics.js";
import { parseMarkdown } from "../src/parser.js";
import { validate } from "../src/validate.js";

function parse(source: string) {
  return buildDocument(parseMarkdown(source));
}

describe("validate — pptx profile", () => {
  it("warns on H4+ headings inside a slide body", () => {
    const doc = parse(`## Title\n\n#### Too deep\n`);
    const diagnostics = validate(doc, { format: "pptx" });
    expect(diagnostics).toContainEqual(expect.objectContaining({ code: "heading_too_deep", severity: "warning" }));
  });

  it("does not warn on H2/H3 nested content (only the slide title is special)", () => {
    const doc = parse(`## Title\n\n### Still fine\n`);
    const diagnostics = validate(doc, { format: "pptx" });
    expect(diagnostics.some((d) => d.code === "heading_too_deep")).toBe(false);
  });

  it("warns on an unknown directive", () => {
    const doc = parse(`## Title\n\n::: chart bar\n:::\n`);
    const diagnostics = validate(doc, { format: "pptx" });
    expect(diagnostics).toContainEqual(expect.objectContaining({ code: "unknown_directive", severity: "warning" }));
  });

  it("does not warn on known pptx directives (layout, notes, image)", () => {
    const doc = parse(`## Title\n\n::: notes\nSpeak clearly.\n:::\n`);
    const diagnostics = validate(doc, { format: "pptx" });
    expect(diagnostics.some((d) => d.code === "unknown_directive")).toBe(false);
  });

  it("errors when an explicit layout name does not resolve, listing available layouts", () => {
    const doc = parse(`## Title\n\n::: layout not-a-real-layout\nBody\n:::\n`);
    const diagnostics = validate(doc, { format: "pptx" });
    const err = diagnostics.find((d) => d.code === "unknown_layout");
    expect(err?.severity).toBe("error");
    expect(err?.hint).toContain("Title and Content");
  });

  it("resolves a known layout name case-insensitively without error", () => {
    const doc = parse(`## Title\n\n::: layout title and content\nBody\n:::\n`);
    const diagnostics = validate(doc, { format: "pptx" });
    expect(diagnostics.some((d) => d.code === "unknown_layout")).toBe(false);
  });

  it("does not warn overflow when content fits the chosen layout", () => {
    const doc = parse(`## Title\n\n::: layout Title and Content\nOne paragraph of body content.\n:::\n`);
    const diagnostics = validate(doc, { format: "pptx" });
    expect(diagnostics.some((d) => d.code === "layout_overflow")).toBe(false);
  });

  it("does not treat consecutive paragraphs as overflow (one placeholder holds flowing text fine)", () => {
    const doc = parse(`## Title\n\n::: layout Title and Content\nFirst paragraph.\n\nSecond paragraph.\n:::\n`);
    const diagnostics = validate(doc, { format: "pptx" });
    expect(diagnostics.some((d) => d.code === "layout_overflow")).toBe(false);
  });

  it("warns overflow when distinct content blocks (e.g. a paragraph and a table) exceed the layout's placeholders", () => {
    const doc = parse(`## Title\n\n::: layout Title and Content\nA paragraph.\n\n| A | B |\n|---|---|\n| 1 | 2 |\n:::\n`);
    const diagnostics = validate(doc, { format: "pptx" });
    const overflow = diagnostics.find((d) => d.code === "layout_overflow");
    expect(overflow?.severity).toBe("warning");
    expect(overflow?.strictPromotable).toBe(true);
  });

  it("counts <!-- col --> separated content as columns for Two Content overflow checking", () => {
    const fits = parse(`## Title\n\n::: layout Two Content\nLeft.\n<!-- col -->\nRight.\n:::\n`);
    expect(validate(fits, { format: "pptx" }).some((d) => d.code === "layout_overflow")).toBe(false);

    const overflows = parse(`## Title\n\n::: layout Two Content\nLeft.\n<!-- col -->\nMiddle.\n<!-- col -->\nRight.\n:::\n`);
    expect(validate(overflows, { format: "pptx" }).some((d) => d.code === "layout_overflow")).toBe(true);
  });

  it("excludes speaker notes from the overflow count (notes is a sibling block, per the spec's own example)", () => {
    const doc = parse(
      `## Title\n\n::: layout Title and Content\nOne paragraph.\n:::\n\n::: notes\nSpeaker notes here, not visible content.\n:::\n`,
    );
    const diagnostics = validate(doc, { format: "pptx" });
    expect(diagnostics.some((d) => d.code === "layout_overflow")).toBe(false);
  });

  it("escalates layout_overflow to an error under --strict", () => {
    const doc = parse(`## Title\n\n::: layout Title and Content\nA paragraph.\n\n| A | B |\n|---|---|\n| 1 | 2 |\n:::\n`);
    const diagnostics = escalateStrict(validate(doc, { format: "pptx" }), true);
    expect(diagnostics).toContainEqual(expect.objectContaining({ code: "layout_overflow", severity: "error" }));
  });

  it("does not check overflow when no explicit layout directive is given (inference always fits)", () => {
    const doc = parse(`## Title\n\nFirst.\n\nSecond.\n\nThird.\n`);
    const diagnostics = validate(doc, { format: "pptx" });
    expect(diagnostics.some((d) => d.code === "layout_overflow")).toBe(false);
  });
});
