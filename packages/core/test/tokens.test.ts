import { describe, expect, it } from "vitest";
import { buildDocument } from "../src/build-tokens.js";
import { parseMarkdown } from "../src/parser.js";
import type { CompylaDocument, DirectiveNode, ListNode, MediaNode, QuoteNode, SectionNode, TableNode } from "../src/types.js";

function parse(source: string): CompylaDocument {
  return buildDocument(parseMarkdown(source));
}

describe("buildDocument — sections", () => {
  it("nests H2 under a preceding H1", () => {
    const doc = parse(`# Report\n\nIntro.\n\n## Revenue\n\nGrew 14%.\n`);
    expect(doc.body).toHaveLength(1);
    const h1 = doc.body[0] as SectionNode;
    expect(h1.level).toBe(1);
    expect(h1.children).toHaveLength(2); // intro paragraph + nested H2 section
    const h2 = h1.children[1] as SectionNode;
    expect(h2.type).toBe("section");
    expect(h2.level).toBe(2);
  });

  it("closes a section when a sibling heading of the same or lower level appears", () => {
    const doc = parse(`# One\n\ntext\n\n# Two\n\ntext\n`);
    expect(doc.body).toHaveLength(2);
    expect((doc.body[0] as SectionNode).level).toBe(1);
    expect((doc.body[1] as SectionNode).level).toBe(1);
  });

  it("allows a heading skip structurally (H1 -> H3 nests, validator flags separately)", () => {
    const doc = parse(`# One\n\n### Three\n`);
    const h1 = doc.body[0] as SectionNode;
    expect((h1.children[0] as SectionNode).level).toBe(3);
  });
});

describe("buildDocument — inline runs", () => {
  it("captures bold, italic, code, and link marks", () => {
    const doc = parse(`Some **bold**, *italic*, \`code\`, and [a link](https://example.com).\n`);
    const paragraph = doc.body[0] as { runs: { text: string; bold?: boolean; italic?: boolean; code?: boolean; link?: string }[] };
    const runs = paragraph.runs;
    expect(runs.find((r) => r.text === "bold")?.bold).toBe(true);
    expect(runs.find((r) => r.text === "italic")?.italic).toBe(true);
    expect(runs.find((r) => r.text === "code")?.code).toBe(true);
    expect(runs.find((r) => r.text === "a link")?.link).toBe("https://example.com");
  });

  it("captures strikethrough (GFM)", () => {
    const doc = parse(`~~removed~~\n`);
    const paragraph = doc.body[0] as { runs: { text: string; strike?: boolean }[] };
    expect(paragraph.runs[0]?.strike).toBe(true);
  });
});

describe("buildDocument — tables", () => {
  it("builds header and rows with formula-passthrough raw text preserved", () => {
    const doc = parse(`| Region | Revenue |\n|--------|---------|\n| APAC | 120000 |\n| Total | =SUM(B2:B2) |\n`);
    const table = doc.body[0] as TableNode;
    expect(table.type).toBe("table");
    expect(table.header.map((c) => c.raw)).toEqual(["Region", "Revenue"]);
    expect(table.rows[1]?.[1]?.raw).toBe("=SUM(B2:B2)");
  });
});

describe("buildDocument — lists", () => {
  it("captures task list checked state", () => {
    const doc = parse(`- [x] Done\n- [ ] Todo\n- Plain\n`);
    const list = doc.body[0] as ListNode;
    expect(list.items.map((i) => i.checked)).toEqual([true, false, null]);
  });
});

describe("buildDocument — media, quote, break", () => {
  it("promotes a paragraph containing only an image to a media node", () => {
    const doc = parse(`![Chart](chart.png)\n`);
    const media = doc.body[0] as MediaNode;
    expect(media.type).toBe("media");
    expect(media.src).toBe("chart.png");
    expect(media.alt).toBe("Chart");
  });

  it("builds blockquotes with nested body content", () => {
    const doc = parse(`> A quote\n`);
    const quote = doc.body[0] as QuoteNode;
    expect(quote.type).toBe("quote");
    expect(quote.children).toHaveLength(1);
  });

  it("builds a break node from a thematic break", () => {
    const doc = parse(`Above\n\n---\n\nBelow\n`);
    expect(doc.body[1]?.type).toBe("break");
  });
});

describe("buildDocument — directives", () => {
  it("builds a fenced directive with whitespace-split args", () => {
    const doc = parse(`::: image width=50%\n:::\n`);
    const directive = doc.body[0] as DirectiveNode;
    expect(directive.type).toBe("directive");
    expect(directive.name).toBe("image");
    expect(directive.args).toEqual(["width=50%"]);
  });

  it("builds a leaf directive from an HTML comment marker", () => {
    const doc = parse(`<!-- pagebreak -->\n`);
    const directive = doc.body[0] as DirectiveNode;
    expect(directive.type).toBe("directive");
    expect(directive.name).toBe("pagebreak");
    expect(directive.args).toEqual([]);
  });

  it("does not treat arbitrary HTML comments as directives if they contain punctuation", () => {
    const doc = parse(`<!-- just a regular comment! -->\n`);
    expect(doc.body).toHaveLength(0);
  });
});
