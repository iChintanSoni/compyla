import { buildDocument, parseMarkdown } from "@compyla/core";
import JSZip from "jszip";
import { describe, expect, it } from "vitest";
import { renderDocx } from "../src/render.js";

// 1x1 red pixel PNG.
const PNG_FIXTURE = Buffer.from(
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=",
  "base64",
);

function parse(source: string) {
  return buildDocument(parseMarkdown(source));
}

async function readDocumentXml(buffer: Buffer): Promise<string> {
  const zip = await JSZip.loadAsync(buffer);
  const file = zip.file("word/document.xml");
  if (!file) throw new Error("word/document.xml missing from rendered docx");
  return file.async("string");
}

describe("renderDocx — byte reproducibility", () => {
  it("produces byte-identical output for identical input across two runs", async () => {
    const doc = parse(`# Quarterly Report\n\n## Revenue\n\nRevenue grew **14%** quarter over quarter.\n`);

    const first = await renderDocx(doc);
    await new Promise((resolve) => setTimeout(resolve, 1100)); // cross a real timestamp boundary
    const second = await renderDocx(doc);

    expect(Buffer.compare(first.buffer, second.buffer)).toBe(0);
  });

  it("is byte-identical even with a hyperlink present (docx assigns its relationship id via unseeded nanoid)", async () => {
    const doc = parse(`See [the board deck](https://example.com/board-deck) for details.\n`);

    const first = await renderDocx(doc);
    const second = await renderDocx(doc);

    expect(Buffer.compare(first.buffer, second.buffer)).toBe(0);
  });
});

describe("renderDocx — structural mapping", () => {
  it("maps headings to Word Heading styles and preserves text", async () => {
    const doc = parse(`# Quarterly Report\n\n## Revenue\n`);
    const { buffer } = await renderDocx(doc);
    const xml = await readDocumentXml(buffer);

    expect(xml).toContain("Quarterly Report");
    expect(xml).toContain('w:val="Heading1"');
    expect(xml).toContain("Revenue");
    expect(xml).toContain('w:val="Heading2"');
  });

  it("maps bold, italic, and hyperlink runs", async () => {
    const doc = parse(`Some **bold** and *italic* and [a link](https://example.com).\n`);
    const { buffer } = await renderDocx(doc);
    const xml = await readDocumentXml(buffer);

    expect(xml).toContain("<w:b/>");
    expect(xml).toContain("<w:i/>");
    expect(xml).toContain("<w:hyperlink ");

    // Word stores hyperlink targets as an external relationship, referenced by r:id — not
    // inline in document.xml.
    const zip = await JSZip.loadAsync(buffer);
    const rels = await zip.file("word/_rels/document.xml.rels")?.async("string");
    expect(rels).toContain("https://example.com");
  });

  it("renders a GFM table as a Word table with a shaded header row", async () => {
    const doc = parse(`| Region | Revenue |\n|---|---|\n| APAC | 120000 |\n`);
    const { buffer } = await renderDocx(doc);
    const xml = await readDocumentXml(buffer);

    expect(xml).toContain("<w:tbl>");
    expect(xml).toContain("Region");
    expect(xml).toContain("APAC");
    expect(xml).toContain("D9E2F3");
  });

  it("renders task list items as checkbox glyphs, not interactive controls", async () => {
    const doc = parse(`- [x] Done\n- [ ] Todo\n`);
    const { buffer } = await renderDocx(doc);
    const xml = await readDocumentXml(buffer);

    expect(xml).toContain("☒");
    expect(xml).toContain("☐");
  });

  it("renders a thematic break as a page break", async () => {
    const doc = parse(`Above\n\n---\n\nBelow\n`);
    const { buffer } = await renderDocx(doc);
    const xml = await readDocumentXml(buffer);
    expect(xml).toContain('w:type="page"');
  });

  it("renders a pagebreak directive the same as a thematic break", async () => {
    const doc = parse(`::: pagebreak\n:::\n`);
    const { buffer } = await renderDocx(doc);
    const xml = await readDocumentXml(buffer);
    expect(xml).toContain('w:type="page"');
  });

  it("renders a fenced code block in a monospace font", async () => {
    const doc = parse("```\nconst x = 1;\n```\n");
    const { buffer } = await renderDocx(doc);
    const xml = await readDocumentXml(buffer);
    expect(xml).toContain("Courier New");
    expect(xml).toContain("const x = 1;");
  });

  it("renders a blockquote indented", async () => {
    const doc = parse(`> A quote\n`);
    const { buffer } = await renderDocx(doc);
    const xml = await readDocumentXml(buffer);
    expect(xml).toContain("A quote");
    expect(xml).toContain("<w:ind ");
  });

  it("renders an unknown directive's children as if the directive were absent", async () => {
    const doc = parse(`::: chart bar\nFallback text.\n:::\n`);
    const { buffer } = await renderDocx(doc);
    const xml = await readDocumentXml(buffer);
    expect(xml).toContain("Fallback text.");
  });

  it("embeds a resolved image sized to the requested width", async () => {
    const doc = parse(`![Logo](logo.png)\n`);
    const { buffer } = await renderDocx(doc, { resolveAsset: () => PNG_FIXTURE });
    const xml = await readDocumentXml(buffer);
    expect(xml).toContain("<w:drawing>");
  });

  it("honors the ::: image width directive as a sizing hint", async () => {
    const doc = parse(`::: image width=50%\n![Logo](logo.png)\n:::\n`);
    const { buffer } = await renderDocx(doc, { resolveAsset: () => PNG_FIXTURE });
    const xml = await readDocumentXml(buffer);
    // 50% of the 624px content width => 312px => 312 * 9525 EMU/px = 2971800
    expect(xml).toContain('cx="2971800"');
  });
});

describe("renderDocx — error handling", () => {
  it("throws a clear error when an image is present but no asset resolver is given", async () => {
    const doc = parse(`![Missing](missing.png)\n`);
    await expect(renderDocx(doc)).rejects.toThrow(/no asset resolver/);
  });
});

describe("renderDocx — template-driven fonts", () => {
  it("embeds a font resolved via the injected fetch and references it in fontTable.xml", async () => {
    const doc = parse(`# Title\n`);
    const ttfBytes = new Uint8Array([1, 2, 3, 4, 5]);
    const fetchImpl = (async (input: string | URL | Request) => {
      const url = typeof input === "string" ? input : input.toString();
      if (url.startsWith("https://fonts.googleapis.com/css2")) {
        return new Response(`url(https://fonts.gstatic.com/s/x/v1/font.ttf)`);
      }
      return new Response(ttfBytes);
    }) as unknown as typeof fetch;

    // A theme with only a major font resolves to a single family for both heading and body.
    const zip = new JSZip();
    zip.file(
      "word/theme/theme1.xml",
      `<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><a:themeElements><a:fontScheme><a:majorFont><a:latin typeface="Acme Sans"/></a:majorFont></a:fontScheme></a:themeElements></a:theme>`,
    );
    const template = await zip.generateAsync({ type: "nodebuffer" });

    const { buffer, diagnostics } = await renderDocx(doc, { template, fetchImpl });
    const zipOut = await JSZip.loadAsync(buffer);
    const fontTable = await zipOut.file("word/fontTable.xml")?.async("string");

    expect(fontTable).toContain("Acme Sans");
    expect(diagnostics.some((d) => d.code === "font_unresolved")).toBe(false);
  });

  it("emits a strict-promotable warning when a template font cannot be resolved", async () => {
    const doc = parse(`# Title\n`);
    const fetchImpl = (async () => new Response("", { status: 404 })) as unknown as typeof fetch;

    const zip = new JSZip();
    zip.file(
      "word/theme/theme1.xml",
      `<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><a:themeElements><a:fontScheme><a:majorFont><a:latin typeface="Definitely Not A Real Font XYZ"/></a:majorFont></a:fontScheme></a:themeElements></a:theme>`,
    );
    const template = await zip.generateAsync({ type: "nodebuffer" });

    const { diagnostics } = await renderDocx(doc, { template, fetchImpl });
    expect(diagnostics).toContainEqual(
      expect.objectContaining({ code: "font_unresolved", severity: "warning", strictPromotable: true }),
    );
  });
});
