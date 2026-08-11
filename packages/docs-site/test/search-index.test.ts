import { describe, expect, it } from "vitest";
import type { BodyNode, SectionNode } from "@compyla/core";
import { computeHeadingIds } from "../src/render-html.js";
import { buildSearchIndex } from "../src/search-index.js";

const pos = { line: 1, column: 1 };

function page(slug: string, title: string) {
  const heading1: SectionNode = {
    type: "section",
    level: 2,
    title: [{ text: "Alpha" }],
    children: [{ type: "paragraph", runs: [{ text: "alpha body text" }], position: pos }],
    position: pos,
  };
  const heading2: SectionNode = {
    type: "section",
    level: 2,
    title: [{ text: "Beta" }],
    children: [
      { type: "paragraph", runs: [{ text: "beta body text" }], position: pos },
      {
        type: "section",
        level: 3,
        title: [{ text: "Beta Sub" }],
        children: [{ type: "paragraph", runs: [{ text: "sub body text" }], position: pos }],
        position: pos,
      },
    ],
    position: pos,
  };
  const body: readonly BodyNode[] = [heading1, heading2];
  return { slug, title, body, headingIds: computeHeadingIds(body) };
}

describe("buildSearchIndex", () => {
  it("emits one record per heading, including nested ones", () => {
    const records = buildSearchIndex([page("a", "Page A")]);
    expect(records).toHaveLength(3);
    expect(records.map((r) => r.headingText)).toEqual(["Alpha", "Beta", "Beta Sub"]);
  });

  it("sets pageSlug/pageTitle/anchor correctly", () => {
    const [record] = buildSearchIndex([page("a", "Page A")]);
    expect(record?.pageSlug).toBe("a");
    expect(record?.pageTitle).toBe("Page A");
    expect(record?.anchor).toBe("#alpha");
  });

  it("does not leak a nested subsection's text into its parent's record", () => {
    const records = buildSearchIndex([page("a", "Page A")]);
    const beta = records.find((r) => r.headingText === "Beta");
    expect(beta?.text).toContain("beta body text");
    expect(beta?.text).not.toContain("sub body text");
  });

  it("covers multiple pages", () => {
    const records = buildSearchIndex([page("a", "Page A"), page("b", "Page B")]);
    expect(records).toHaveLength(6);
  });
});
