import { buildDocument, parseMarkdown } from "@compyla/core";
import { inflateSync } from "node:zlib";
import { describe, expect, it } from "vitest";
import { renderPdf } from "../src/render.js";

function parse(source: string) {
  return buildDocument(parseMarkdown(source));
}

// Dependency-free PDF content-stream reader (no pdf-parse/pdfjs-dist — see the plan's rationale:
// keeps the monorepo's minimal-dependency style, mirrors docx's own `readDocumentXml` test helper but
// for PDF). Content streams are the only FlateDecode-compressed part of a pdfkit PDF; the object/xref/
// trailer structure around them stays plain text, so page counting doesn't need decompression at all.
// pdfkit's standard-14 fonts use WinAnsiEncoding (~ CP1252), which only differs from plain Latin-1 in
// the 0x80-0x9F byte range (bullet, em-dash, smart quotes, etc. live there) — decode that range
// explicitly instead of via Buffer's "latin1" encoding, which maps those bytes to C1 control chars.
const WIN_ANSI_HIGH: Record<number, string> = {
  0x91: "‘",
  0x92: "’",
  0x93: "“",
  0x94: "”",
  0x95: "•",
  0x96: "–",
  0x97: "—",
};

function decodeWinAnsiHex(hex: string): string {
  let out = "";
  for (let i = 0; i < hex.length; i += 2) {
    const byte = parseInt(hex.slice(i, i + 2), 16);
    out += WIN_ANSI_HIGH[byte] ?? String.fromCharCode(byte);
  }
  return out;
}

function pageObjectCount(buffer: Buffer): number {
  const raw = buffer.toString("latin1");
  return (raw.match(/\/Type\s*\/Page(?!s)/g) ?? []).length;
}

function extractShownText(buffer: Buffer): string[] {
  const raw = buffer.toString("latin1");
  const streamRe = /stream\r?\n([\s\S]*?)endstream/g;
  const shown: string[] = [];

  let streamMatch: RegExpExecArray | null;
  while ((streamMatch = streamRe.exec(raw))) {
    let body = streamMatch[1]!;
    if (body.endsWith("\r\n")) body = body.slice(0, -2);
    else if (body.endsWith("\n")) body = body.slice(0, -1);

    let inflated: string;
    try {
      inflated = inflateSync(Buffer.from(body, "latin1")).toString("latin1");
    } catch {
      continue; // not a (compressed) content stream — e.g. an image XObject.
    }

    const showRe = /\[((?:<[0-9a-fA-F]*>|\([^)]*\)|-?\d+(?:\.\d+)?|\s)*)\]\s*TJ|\(([^)]*)\)\s*Tj/g;
    let showMatch: RegExpExecArray | null;
    while ((showMatch = showRe.exec(inflated))) {
      if (showMatch[2] !== undefined) {
        shown.push(showMatch[2]);
        continue;
      }
      const segment = showMatch[1] ?? "";
      let line = "";
      const partRe = /<([0-9a-fA-F]*)>|\(([^)]*)\)/g;
      let partMatch: RegExpExecArray | null;
      while ((partMatch = partRe.exec(segment))) {
        line += partMatch[1] !== undefined ? decodeWinAnsiHex(partMatch[1]) : partMatch[2];
      }
      shown.push(line);
    }
  }
  return shown;
}

function fullText(buffer: Buffer): string {
  return extractShownText(buffer).join(" ");
}

describe("renderPdf — byte reproducibility", () => {
  it("produces byte-identical output for identical input across two runs", async () => {
    const doc = parse(`# Quarterly Report\n\n## Revenue\n\nRevenue grew **14%** quarter over quarter.\n`);

    const first = await renderPdf(doc);
    await new Promise((resolve) => setTimeout(resolve, 1100)); // cross a real timestamp boundary
    const second = await renderPdf(doc);

    expect(Buffer.compare(first.buffer, second.buffer)).toBe(0);
  });

  it("is byte-identical even with a hyperlink present", async () => {
    const doc = parse(`See [the board deck](https://example.com/board-deck) for details.\n`);

    const first = await renderPdf(doc);
    const second = await renderPdf(doc);

    expect(Buffer.compare(first.buffer, second.buffer)).toBe(0);
  });

  it("is byte-identical with title/author frontmatter set", async () => {
    const doc = parse(`---\ntitle: Q3 Review\nauthor: J. Doe\n---\n\n# Q3 Review\n`);

    const first = await renderPdf(doc);
    const second = await renderPdf(doc);

    expect(Buffer.compare(first.buffer, second.buffer)).toBe(0);
  });
});

describe("renderPdf — structural mapping", () => {
  it("produces a valid PDF with pages", async () => {
    const doc = parse(`# Title\n\nBody text.\n`);
    const { buffer } = await renderPdf(doc);

    expect(buffer.subarray(0, 5).toString("latin1")).toBe("%PDF-");
    expect(buffer.subarray(-6).toString("latin1").trim()).toBe("%%EOF");
    expect(pageObjectCount(buffer)).toBeGreaterThanOrEqual(1);
  });

  it("renders headings and paragraph text", async () => {
    const doc = parse(`# Quarterly Report\n\n## Revenue\n\nRevenue grew steadily.\n`);
    const { buffer } = await renderPdf(doc);
    const text = fullText(buffer);

    expect(text).toContain("Quarterly Report");
    expect(text).toContain("Revenue");
    expect(text).toContain("Revenue grew steadily.");
  });

  it("preserves bold, italic, and link run text", async () => {
    const doc = parse(`Some **bold** and *italic* and [a link](https://example.com).\n`);
    const { buffer } = await renderPdf(doc);
    const text = fullText(buffer);

    expect(text).toContain("bold");
    expect(text).toContain("italic");
    expect(text).toContain("a link");
  });

  it("renders ordered, unordered, and task-list markers", async () => {
    const doc = parse(`- Plain bullet\n\n1. First\n2. Second\n\n- [x] Done\n- [ ] Not done\n`);
    const { buffer } = await renderPdf(doc);
    const text = fullText(buffer);

    expect(text).toContain("•");
    expect(text).toContain("1.");
    expect(text).toContain("2.");
    expect(text).toContain("[x]");
    expect(text).toContain("[ ]");
  });

  it("renders a GFM table's header and cell text", async () => {
    const doc = parse(`| Phase | Weeks |\n|---|---|\n| Discovery | 2 |\n| Migration | 6 |\n`);
    const { buffer } = await renderPdf(doc);
    const text = fullText(buffer);

    expect(text).toContain("Phase");
    expect(text).toContain("Weeks");
    expect(text).toContain("Discovery");
    expect(text).toContain("Migration");
  });

  it("renders blockquote content", async () => {
    const doc = parse(`> A quoted line.\n> — Attribution\n`);
    const { buffer } = await renderPdf(doc);
    const text = fullText(buffer);

    expect(text).toContain("A quoted line.");
  });

  it("renders fenced code block content", async () => {
    const doc = parse("```\nYear 1: $120,000\n```\n");
    const { buffer } = await renderPdf(doc);
    const text = fullText(buffer);

    expect(text).toContain("Year 1: $120,000");
  });

  it("`---` forces a page break (docx/pdf share this convention)", async () => {
    const doc = parse(`# Page One\n\nFirst page content.\n\n---\n\n# Page Two\n\nSecond page content.\n`);
    const { buffer } = await renderPdf(doc);

    expect(pageObjectCount(buffer)).toBe(2);
  });

  it("`::: pagebreak` forces a page break", async () => {
    const doc = parse(`# Page One\n\n::: pagebreak\n:::\n\n# Page Two\n`);
    const { buffer } = await renderPdf(doc);

    expect(pageObjectCount(buffer)).toBe(2);
  });

  it("respects page_size and margins frontmatter without erroring", async () => {
    const doc = parse(`---\npage_size: Letter\nmargins: narrow\n---\n\n# Title\n`);
    const { buffer, diagnostics } = await renderPdf(doc);

    expect(buffer.subarray(0, 5).toString("latin1")).toBe("%PDF-");
    expect(diagnostics).toEqual([]);
  });
});

describe("renderPdf — end to end (examples/proposal.md's feature set)", () => {
  const PROPOSAL_MD = `---
title: Vendor Proposal — Data Platform Migration
author: Platform Team
format: pdf
page_size: A4
margins: narrow
---

# Vendor Proposal

::: toc
:::

This proposal covers migrating our analytics pipeline to a **managed** data platform, reducing
on-call load and *materially* improving query latency. Full context lives in the
[engineering RFC](https://example.com/rfc/data-platform).

## Scope

- Cut infra ops load
- Improve query latency
- Consolidate vendors

## Timeline

| Phase | Owner | Weeks |
|---|---|---|
| Discovery | Platform Team | 2 |
| Migration | Platform Team | 6 |
| Cutover | Platform Team | 1 |

### Milestones

- [x] Discovery complete
- [x] Vendor selected
- [ ] Migration started
- [ ] Cutover complete

> "The switch paid for itself within a quarter."
> — VP Engineering

::: pagebreak
:::

## Cost Model

\`\`\`
Year 1: $120,000
Year 2: $80,000
\`\`\`

## Risks

Some risk content here.

## Appendix

Some appendix content here.
`;

  it("converts without error and produces a multi-page, byte-reproducible PDF", async () => {
    const doc = parse(PROPOSAL_MD);
    const first = await renderPdf(doc);
    const second = await renderPdf(doc);

    expect(Buffer.compare(first.buffer, second.buffer)).toBe(0);
    expect(pageObjectCount(first.buffer)).toBeGreaterThan(1);
  });

  it("bakes real, correct page numbers into the computed table of contents", async () => {
    const doc = parse(PROPOSAL_MD);
    const { buffer } = await renderPdf(doc);
    const text = fullText(buffer);
    const pageCount = pageObjectCount(buffer);

    expect(text).toContain("Table of Contents");

    // Every TOC entry line ("Title ..... N") must cite a page number within the document's real
    // range, and the forced `::: pagebreak` must land Cost Model on a strictly later page than Scope.
    const tocLine = (label: string) => {
      const m = new RegExp(`${label}[.\\s]*\\.*\\s*(\\d+)`).exec(text);
      expect(m, `expected a TOC entry for "${label}"`).not.toBeNull();
      return Number(m![1]);
    };

    const scopePage = tocLine("Scope");
    const costModelPage = tocLine("Cost Model");

    expect(scopePage).toBeGreaterThanOrEqual(1);
    expect(costModelPage).toBeGreaterThan(scopePage);
    expect(costModelPage).toBeLessThanOrEqual(pageCount);
  });

  it("renders every section's content", async () => {
    const doc = parse(PROPOSAL_MD);
    const { buffer } = await renderPdf(doc);
    const text = fullText(buffer);

    for (const expected of [
      "Vendor Proposal",
      "managed",
      "engineering RFC",
      "Cut infra ops load",
      "Discovery",
      "[x]",
      "[ ]",
      "The switch paid for itself",
      "Year 1: $120,000",
      "Some risk content here.",
      "Some appendix content here.",
    ]) {
      expect(text).toContain(expected);
    }
  });
});
