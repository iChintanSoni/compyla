import { describe, expect, it } from "vitest";
import { resolveInternalLink } from "../src/links.js";
import { PAGES } from "../src/nav.js";

describe("resolveInternalLink", () => {
  it.each([
    ["architecture.md", "../architecture/"],
    ["docs/quickstart.md", "../quickstart/"],
    ["CLI.MD", "../cli/"],
    ["https://pandoc.org", null],
    ["#foo", null],
    ["typo.md", null],
  ])("resolves %s -> %s", (href, expected) => {
    expect(resolveInternalLink(href, PAGES)).toBe(expected);
  });
});
