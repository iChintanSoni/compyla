import { describe, expect, it } from "vitest";
import type { BodyNode, DirectiveNode, ListNode, ParagraphNode, SectionNode, TableNode } from "@compyla/core";
import { computeHeadingIds, escapeHtml, renderBody, renderInline, slugify, type RenderContext } from "../src/render-html.js";

const pos = { line: 1, column: 1 };

function ctxFor(body: readonly BodyNode[]): RenderContext {
  return {
    resolveLink: (href) => href,
    headingIds: computeHeadingIds(body),
  };
}

describe("slugify", () => {
  it("lowercases, replaces non-alphanumeric runs with hyphens, and trims edges", () => {
    expect(slugify("CLI & SDK Reference!")).toBe("cli-sdk-reference");
  });
});

describe("escapeHtml", () => {
  it("escapes the five reserved characters", () => {
    expect(escapeHtml(`<a href="x">&</a>`)).toBe("&lt;a href=&quot;x&quot;&gt;&amp;&lt;/a&gt;");
  });
});

describe("renderInline", () => {
  it("nests bold, wraps a link outermost, and escapes text", () => {
    const html = renderInline([{ text: "click", bold: true, link: "https://example.com" }], {
      resolveLink: (href) => href,
      headingIds: new Map(),
    });
    expect(html).toBe('<a href="https://example.com"><strong>click</strong></a>');
  });

  it("routes link hrefs through resolveLink", () => {
    const html = renderInline([{ text: "arch", link: "architecture.md" }], {
      resolveLink: () => "../architecture/",
      headingIds: new Map(),
    });
    expect(html).toBe('<a href="../architecture/">arch</a>');
  });
});

describe("renderBody — paragraph", () => {
  it("renders a plain paragraph", () => {
    const node: ParagraphNode = { type: "paragraph", runs: [{ text: "hello" }], position: pos };
    expect(renderBody([node], ctxFor([node]))).toBe("<p>hello</p>");
  });
});

describe("renderBody — table", () => {
  it("renders header and rows with the right cell counts", () => {
    const node: TableNode = {
      type: "table",
      header: [{ runs: [{ text: "A" }], raw: "A" }, { runs: [{ text: "B" }], raw: "B" }],
      align: ["left", null],
      rows: [[{ runs: [{ text: "1" }], raw: "1" }, { runs: [{ text: "2" }], raw: "2" }]],
      position: pos,
    };
    const html = renderBody([node], ctxFor([node]));
    expect(html).toContain('<div class="table-wrap"><table>');
    expect((html.match(/<th[ >]/g) ?? []).length).toBe(2);
    expect((html.match(/<td[ >]/g) ?? []).length).toBe(2);
    expect(html).toContain('style="text-align:left"');
  });
});

describe("renderBody — task lists", () => {
  function taskList(checked: boolean | null): ListNode {
    return {
      type: "list",
      ordered: false,
      items: [{ children: [{ type: "paragraph", runs: [{ text: "item" }], position: pos }], checked, position: pos }],
      position: pos,
    };
  }

  it("emits a checked, disabled checkbox for checked: true", () => {
    const node = taskList(true);
    expect(renderBody([node], ctxFor([node]))).toContain('<li class="task-item"><input type="checkbox" disabled checked>');
  });

  it("emits an unchecked, disabled checkbox for checked: false", () => {
    const node = taskList(false);
    expect(renderBody([node], ctxFor([node]))).toContain('<li class="task-item"><input type="checkbox" disabled>');
  });

  it("emits no checkbox for checked: null (plain list item)", () => {
    const node = taskList(null);
    const html = renderBody([node], ctxFor([node]));
    expect(html).not.toContain("checkbox");
    expect(html).toContain("<li><p>item</p></li>");
  });
});

describe("renderBody — sections", () => {
  it("nests a subsection inside its parent <section>, not as a flattened sibling", () => {
    const inner: SectionNode = { type: "section", level: 2, title: [{ text: "Sub" }], children: [], position: pos };
    const outer: SectionNode = { type: "section", level: 1, title: [{ text: "Top" }], children: [inner], position: pos };
    const html = renderBody([outer], ctxFor([outer]));
    const outerOpen = html.indexOf("<section");
    const innerOpen = html.indexOf("<section", outerOpen + 1);
    const outerClose = html.lastIndexOf("</section>");
    expect(innerOpen).toBeGreaterThan(outerOpen);
    expect(innerOpen).toBeLessThan(outerClose);
  });

  it("de-dupes identical heading text on the same page", () => {
    const a: SectionNode = { type: "section", level: 2, title: [{ text: "Intro" }], children: [], position: pos };
    const b: SectionNode = { type: "section", level: 2, title: [{ text: "Intro" }], children: [], position: pos };
    const html = renderBody([a, b], ctxFor([a, b]));
    expect(html).toContain('id="intro"');
    expect(html).toContain('id="intro-2"');
  });
});

describe("renderBody — directive passthrough", () => {
  it("degrades an unknown directive to its rendered children, dropping the wrapper", () => {
    const node: DirectiveNode = {
      type: "directive",
      name: "toc",
      args: [],
      argsRaw: "",
      children: [{ type: "paragraph", runs: [{ text: "inner" }], position: pos }],
      position: pos,
    };
    expect(renderBody([node], ctxFor([node]))).toBe("<p>inner</p>");
  });
});
