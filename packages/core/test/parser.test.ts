import { describe, expect, it } from "vitest";
import { parseMarkdown } from "../src/parser.js";

describe("parseMarkdown", () => {
  it("extracts YAML frontmatter into meta", () => {
    const result = parseMarkdown(`---\nformat: docx\ntitle: Q3 Review\n---\n\n# Hello\n`);
    expect(result.meta).toEqual({ format: "docx", title: "Q3 Review" });
  });

  it("has no meta when there is no frontmatter", () => {
    const result = parseMarkdown(`# Hello\n`);
    expect(result.meta).toEqual({});
  });

  it("parses a simple directive fence with args", () => {
    const result = parseMarkdown(`::: notes\nMention APAC first.\n:::\n`);
    expect(result.children).toHaveLength(1);
    const directive = result.children[0] as { type: string; name: string; argsRaw: string };
    expect(directive.type).toBe("compylaDirective");
    expect(directive.name).toBe("notes");
    expect(directive.argsRaw).toBe("");
  });

  it("captures directive args as raw text", () => {
    const result = parseMarkdown(`::: image width=50%\n:::\n`);
    const directive = result.children[0] as { name: string; argsRaw: string };
    expect(directive.name).toBe("image");
    expect(directive.argsRaw).toBe("width=50%");
  });

  it("reports the directive's absolute start line", () => {
    const result = parseMarkdown(`# Title\n\nSome text.\n\n::: notes\nSpeaker notes.\n:::\n`);
    const directive = result.children.find((c) => c.type === "compylaDirective") as { position: { start: { line: number } } };
    expect(directive.position.start.line).toBe(5);
  });

  it("preserves absolute line numbers for content after a directive block", () => {
    const result = parseMarkdown(`::: notes\nline a\nline b\n:::\n\n# After\n`);
    const heading = result.children.find((c) => c.type === "heading");
    expect(heading?.position?.start.line).toBe(6);
  });

  it("preserves absolute line numbers for content inside a directive block", () => {
    const result = parseMarkdown(`Intro paragraph.\n\n::: notes\n# Nested heading\n:::\n`);
    const directive = result.children.find((c) => c.type === "compylaDirective") as {
      children: readonly { type: string; position?: { start: { line: number } } }[];
    };
    const nested = directive.children.find((c) => c.type === "heading");
    expect(nested?.position?.start.line).toBe(4);
  });

  it("supports nested directive fences", () => {
    const result = parseMarkdown(`::: layout two-col\nLeft.\n\n::: notes\nInner notes.\n:::\n\nRight.\n:::\n`);
    const outer = result.children[0] as {
      name: string;
      children: readonly { type: string; name?: string }[];
    };
    expect(outer.name).toBe("layout");
    const inner = outer.children.find((c) => c.type === "compylaDirective") as { name: string } | undefined;
    expect(inner?.name).toBe("notes");
  });

  it("does not treat ::: fences inside fenced code blocks as directives", () => {
    const result = parseMarkdown("```\n::: notes\ncode content\n:::\n```\n");
    expect(result.children.some((c) => c.type === "compylaDirective")).toBe(false);
    expect(result.children[0]?.type).toBe("code");
  });

  it("recovers unclosed directive fences without throwing", () => {
    expect(() => parseMarkdown(`::: notes\nUnclosed.\n`)).not.toThrow();
  });

  it("keeps GFM tables and inline markers intact alongside directives", () => {
    const result = parseMarkdown(`| A | B |\n|---|---|\n| 1 | 2 |\n\n<!-- pagebreak -->\n`);
    expect(result.children.some((c) => c.type === "table")).toBe(true);
    expect(result.children.some((c) => c.type === "html")).toBe(true);
  });
});
