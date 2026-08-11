import { readdir } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { PAGES } from "../src/nav.js";

describe("PAGES", () => {
  it("has 6 entries with unique slugs", () => {
    expect(PAGES).toHaveLength(6);
    expect(new Set(PAGES.map((p) => p.slug)).size).toBe(6);
  });

  it("registers every markdown file that actually exists under docs/ — guards against a forgotten new doc", async () => {
    const docsDir = fileURLToPath(new URL("../../../docs", import.meta.url));
    const realFiles = (await readdir(docsDir)).filter((f) => f.endsWith(".md")).sort();
    const registeredFiles = PAGES.map((p) => p.sourceFile).sort();
    expect(registeredFiles).toEqual(realFiles);
  });
});
