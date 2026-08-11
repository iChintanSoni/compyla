import { describe, expect, it } from "vitest";
import { convert } from "../src/convert.js";
import { loadRenderer, UnsupportedFormatError } from "../src/renderer-registry.js";

describe("convert", () => {
  it("renders a clean document to a docx buffer", async () => {
    const result = await convert("# Title\n\nSome body text.\n", { format: "docx" });
    expect(result.ok).toBe(true);
    expect(result.buffer).toBeInstanceOf(Buffer);
    expect(result.buffer!.length).toBeGreaterThan(0);
  });

  it("blocks on pre-render validation errors without invoking a renderer", async () => {
    const result = await convert("# Title\n\n![missing](missing.png)\n", {
      format: "docx",
      resolveAsset: () => false,
    });
    expect(result.ok).toBe(false);
    expect(result.buffer).toBeUndefined();
    expect(result.diagnostics.some((d) => d.code === "image_unresolved")).toBe(true);
  });

  it("rejects for a format with no validation profile (e.g. pdf, not yet implemented) before ever loading a renderer", async () => {
    // validate() rejects the format before convert() reaches loadRenderer — this is a plain Error
    // from core's profile lookup, not the renderer registry's own UnsupportedFormatError below.
    await expect(convert("# Title\n", { format: "pdf" })).rejects.toThrow(/Unknown format "pdf"/);
  });

  it("loadRenderer itself throws UnsupportedFormatError for a format with no registry entry", async () => {
    await expect(loadRenderer("epub")).rejects.toBeInstanceOf(UnsupportedFormatError);
  });
});
