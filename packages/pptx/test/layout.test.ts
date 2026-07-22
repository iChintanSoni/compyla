import { buildDocument, groupSlides, parseMarkdown, resolveSlideContent } from "@compyla/core";
import { describe, expect, it } from "vitest";
import { inferLayout } from "../src/layout.js";

function slideContent(source: string) {
  const doc = buildDocument(parseMarkdown(source));
  const [slide] = groupSlides(doc.body);
  return resolveSlideContent(slide!).content;
}

describe("inferLayout — Quote gating (markdown-spec.md §7.3)", () => {
  it("infers Quote for a lone blockquote when a Quote layout is available", () => {
    const content = slideContent(`## Title\n\n> A great quarter.\n`);
    expect(inferLayout(content, true)).toBe("Quote");
  });

  it("falls back to Title and Content for a lone blockquote when no Quote layout is available", () => {
    const content = slideContent(`## Title\n\n> A great quarter.\n`);
    expect(inferLayout(content, false)).toBe("Title and Content");
  });

  it("defaults hasQuoteLayout to true when the argument is omitted", () => {
    const content = slideContent(`## Title\n\n> A great quarter.\n`);
    expect(inferLayout(content)).toBe("Quote");
  });
});
