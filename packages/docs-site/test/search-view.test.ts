// @vitest-environment jsdom
import { describe, expect, it } from "vitest";
import { matchRecords, renderSearchResult, type SearchRecord } from "../browser/search-view.js";

function record(overrides: Partial<SearchRecord> = {}): SearchRecord {
  return {
    pageSlug: "quickstart",
    pageTitle: "Quickstart",
    headingText: "Install",
    anchor: "#install",
    text: "run npm install to get started with the cli",
    ...overrides,
  };
}

describe("matchRecords", () => {
  it("returns no results for an empty query", () => {
    expect(matchRecords([record()], "")).toEqual([]);
  });

  it("ranks a heading-text match above a body-text-only match", () => {
    const headingMatch = record({ pageSlug: "a", headingText: "Install" });
    const bodyOnlyMatch = record({ pageSlug: "b", headingText: "Other", text: "you can install compyla here" });
    const results = matchRecords([bodyOnlyMatch, headingMatch], "install");
    expect(results[0]?.pageSlug).toBe("a");
  });

  it("caps results at the given limit", () => {
    const many = Array.from({ length: 20 }, (_, i) => record({ pageSlug: `p${i}` }));
    expect(matchRecords(many, "install", 5)).toHaveLength(5);
  });
});

describe("renderSearchResult", () => {
  it("builds an href from docsPrefix, pageSlug, and anchor", () => {
    const li = renderSearchResult(record(), "install", "../");
    const a = li.querySelector("a");
    expect(a?.getAttribute("href")).toBe("../quickstart/#install");
  });

  it("wraps the matched substring in <mark>", () => {
    const li = renderSearchResult(record(), "install", "../");
    expect(li.querySelector("mark")?.textContent?.toLowerCase()).toBe("install");
  });
});
