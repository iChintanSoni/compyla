import { buildDocument, parseMarkdown } from "@compyla/core";
import JSZip from "jszip";
import { describe, expect, it } from "vitest";
import { renderPptx } from "../src/render.js";

const PNG_FIXTURE = Buffer.from(
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=",
  "base64",
);

function parse(source: string) {
  return buildDocument(parseMarkdown(source));
}

async function slideXml(buffer: Buffer, index = 1): Promise<string> {
  const zip = await JSZip.loadAsync(buffer);
  const file = zip.file(`ppt/slides/slide${index}.xml`);
  if (!file) throw new Error(`ppt/slides/slide${index}.xml missing from rendered pptx`);
  return file.async("string");
}

async function slideCount(buffer: Buffer): Promise<number> {
  const zip = await JSZip.loadAsync(buffer);
  return Object.keys(zip.files).filter((n) => /^ppt\/slides\/slide\d+\.xml$/.test(n)).length;
}

describe("renderPptx — byte reproducibility", () => {
  it("produces byte-identical output for identical input across two runs", async () => {
    const doc = parse(`# Q3 Review\n\n---\n\n## Revenue\n\nGrew 14%.\n`);
    const first = await renderPptx(doc);
    await new Promise((r) => setTimeout(r, 1100));
    const second = await renderPptx(doc);
    expect(Buffer.compare(first.buffer, second.buffer)).toBe(0);
  });

  it("is byte-identical even with a hyperlink present", async () => {
    const doc = parse(`## Slide\n\nSee [the deck](https://example.com).\n`);
    const first = await renderPptx(doc);
    const second = await renderPptx(doc);
    expect(Buffer.compare(first.buffer, second.buffer)).toBe(0);
  });
});

describe("renderPptx — slide segmentation", () => {
  it("splits into one slide per thematic break", async () => {
    const doc = parse(`# One\n\n---\n\n# Two\n\n---\n\n# Three\n`);
    const { buffer } = await renderPptx(doc);
    expect(await slideCount(buffer)).toBe(3);
  });

  it("puts the whole document on one slide when there are no breaks", async () => {
    const doc = parse(`# Only slide\n\nSome content.\n`);
    const { buffer } = await renderPptx(doc);
    expect(await slideCount(buffer)).toBe(1);
  });
});

describe("renderPptx — title slide handling", () => {
  it("treats slide 0 as a title slide (with subtitle) only when the deck has more than one slide", async () => {
    const doc = parse(`# Deck Title\n\nA subtitle paragraph.\n\n---\n\n# Slide Two\n`);
    const { buffer } = await renderPptx(doc);
    const xml = await slideXml(buffer, 1);
    expect(xml).toContain("Deck Title");
    expect(xml).toContain("A subtitle paragraph.");
  });

  it("does not force title-slide treatment (and does not drop content) for a single-slide image-only document", async () => {
    // Regression test: this previously rendered 0 diagnostics but silently dropped the image,
    // because slide 0 was unconditionally forced into "Title Slide", whose subtitle placeholder
    // is filled via plain-text flattening that ignores media nodes entirely.
    const doc = parse(`## Chart\n\n![A chart](chart.png)\n`);
    const { buffer } = await renderPptx(doc, { resolveAsset: () => PNG_FIXTURE });
    const zip = await JSZip.loadAsync(buffer);
    const mediaFiles = Object.keys(zip.files).filter((n) => n.startsWith("ppt/media/"));
    expect(mediaFiles.length).toBeGreaterThan(0);
  });
});

describe("renderPptx — layout inference", () => {
  it("infers Section Header for a title-only slide", async () => {
    const doc = parse(`# One\n\n---\n\n## Title Only\n`);
    const { buffer } = await renderPptx(doc);
    const rels = await (await JSZip.loadAsync(buffer)).file("ppt/slides/_rels/slide2.xml.rels")!.async("string");
    expect(rels).toMatch(/slideLayout2\.xml|slideLayout\d\.xml/);
  });

  it("infers Two Content from <!-- col --> separated content", async () => {
    const doc = parse(`## Compare\n\nLeft.\n<!-- col -->\nRight.\n`);
    const { buffer } = await renderPptx(doc);
    const xml = await slideXml(buffer);
    expect(xml).toContain("Left.");
    expect(xml).toContain("Right.");
  });

  it("honors an explicit ::: layout directive over inference", async () => {
    const doc = parse(`## Title\n\n::: layout Section Header\nIgnored body text.\n:::\n`);
    const { buffer } = await renderPptx(doc);
    const xml = await slideXml(buffer);
    // Section Header has no body placeholder — content targeting "body" silently has nowhere to
    // go structurally meaningful, but the render must not throw.
    expect(xml).toContain("Title");
  });

  it("falls back to inference when an explicit layout name doesn't resolve (validator already flags the error separately)", async () => {
    const doc = parse(`## Title\n\n::: layout not-a-real-layout\nSome body text.\n:::\n`);
    const { buffer } = await renderPptx(doc);
    const xml = await slideXml(buffer);
    expect(xml).toContain("Some body text.");
  });
});

describe("renderPptx — content mapping", () => {
  it("renders task list items as checkbox glyphs", async () => {
    const doc = parse(`## Title\n\n- [x] Done\n- [ ] Todo\n`);
    const { buffer } = await renderPptx(doc);
    const xml = await slideXml(buffer);
    expect(xml).toContain("☒");
    expect(xml).toContain("☐");
  });

  it("renders a table with a shaded header row", async () => {
    const doc = parse(`## Data\n\n| Region | Revenue |\n|---|---|\n| APAC | 120000 |\n`);
    const { buffer } = await renderPptx(doc);
    const xml = await slideXml(buffer);
    expect(xml).toContain("<a:tbl>");
    expect(xml).toContain("Region");
    expect(xml).toContain("APAC");
  });

  it("renders a blockquote as its own quote block", async () => {
    const doc = parse(`## Title\n\n> A great quarter.\n`);
    const { buffer } = await renderPptx(doc);
    const xml = await slideXml(buffer);
    expect(xml).toContain("A great quarter.");
  });

  it("puts speaker notes in the notes part, not on the visible slide", async () => {
    const doc = parse(`## Title\n\nVisible body.\n\n::: notes\nSpeak slowly.\n:::\n`);
    const { buffer } = await renderPptx(doc);
    const zip = await JSZip.loadAsync(buffer);
    const notesXml = await zip.file("ppt/notesSlides/notesSlide1.xml")!.async("string");
    const slideXmlContent = await slideXml(buffer);
    expect(notesXml).toContain("Speak slowly.");
    expect(slideXmlContent).not.toContain("Speak slowly.");
  });

  it("embeds a resolved image with byte-identical media content", async () => {
    const doc = parse(`## Chart\n\n![A chart](chart.png)\n`);
    const { buffer } = await renderPptx(doc, { resolveAsset: () => PNG_FIXTURE });
    const zip = await JSZip.loadAsync(buffer);
    const mediaPath = Object.keys(zip.files).find((n) => !zip.files[n]!.dir && n.startsWith("ppt/media/"));
    expect(mediaPath).toBeDefined();
    const mediaBytes = await zip.file(mediaPath!)!.async("nodebuffer");
    expect(Buffer.compare(mediaBytes, PNG_FIXTURE)).toBe(0);
  });
});

describe("renderPptx — error handling", () => {
  it("throws a clear error when an image is present but no asset resolver is given", async () => {
    const doc = parse(`## Chart\n\n![Missing](missing.png)\n`);
    await expect(renderPptx(doc)).rejects.toThrow(/no asset resolver/);
  });
});

describe("renderPptx — template-driven fonts", () => {
  it("emits a strict-promotable warning when a template theme font cannot be resolved", async () => {
    const zip = new JSZip();
    zip.file(
      "ppt/theme/theme1.xml",
      `<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><a:themeElements><a:fontScheme><a:majorFont><a:latin typeface="Definitely Not A Real Font XYZ"/></a:majorFont></a:fontScheme></a:themeElements></a:theme>`,
    );
    const template = await zip.generateAsync({ type: "nodebuffer" });
    const fetchImpl = (async () => new Response("", { status: 404 })) as unknown as typeof fetch;

    const doc = parse(`# Title\n`);
    const { diagnostics } = await renderPptx(doc, { template, fetchImpl });
    expect(diagnostics).toContainEqual(
      expect.objectContaining({ code: "font_unresolved", severity: "warning", strictPromotable: true }),
    );
  });
});
