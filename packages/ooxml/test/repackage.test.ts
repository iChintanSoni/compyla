import JSZip from "jszip";
import { describe, expect, it } from "vitest";
import { repackageDeterministically } from "../src/repackage.js";

function coreXml(created: string): string {
  return `<?xml version="1.0" encoding="UTF-8" standalone="yes"?><cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dcterms="http://purl.org/dc/terms/" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"><dcterms:created xsi:type="dcterms:W3CDTF">${created}</dcterms:created><dcterms:modified xsi:type="dcterms:W3CDTF">${created}</dcterms:modified></cp:coreProperties>`;
}

async function buildZip(files: Record<string, string>, dirs: string[] = []): Promise<Buffer> {
  const zip = new JSZip();
  const now = new Date();
  for (const dir of dirs) zip.file(dir, null, { dir: true, date: now });
  for (const [path, content] of Object.entries(files)) zip.file(path, content, { date: now });
  return zip.generateAsync({ type: "nodebuffer" });
}

describe("repackageDeterministically", () => {
  it("produces identical output regardless of the input's embedded timestamp", async () => {
    const zipA = await buildZip({ "docProps/core.xml": coreXml("2020-01-01T00:00:00Z"), "content.xml": "<a/>" });
    const zipB = await buildZip({ "docProps/core.xml": coreXml("2030-06-15T12:34:56Z"), "content.xml": "<a/>" });

    const outA = await repackageDeterministically(zipA);
    const outB = await repackageDeterministically(zipB);

    expect(Buffer.compare(outA, outB)).toBe(0);
  });

  it("pins directory entry dates too (zip.folder() alone would not)", async () => {
    const zipA = await buildZip({ "content.xml": "<a/>" }, ["word/"]);
    await new Promise((r) => setTimeout(r, 1100));
    const zipB = await buildZip({ "content.xml": "<a/>" }, ["word/"]);

    const outA = await repackageDeterministically(zipA);
    const outB = await repackageDeterministically(zipB);

    expect(Buffer.compare(outA, outB)).toBe(0);
  });

  it("canonicalizes hyperlink relationship ids consistently between a .rels file and its owning part, for any OOXML part path", async () => {
    const relsA = `<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdRANDOM1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="https://example.com" TargetMode="External"/></Relationships>`;
    const partA = `<p:sld xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><a:hlinkClick r:id="rIdRANDOM1"/></p:sld>`;
    const relsB = relsA.replace("rIdRANDOM1", "rIdCOMPLETELYDIFFERENT");
    const partB = partA.replace("rIdRANDOM1", "rIdCOMPLETELYDIFFERENT");

    const zipA = await buildZip({ "ppt/slides/_rels/slide1.xml.rels": relsA, "ppt/slides/slide1.xml": partA });
    const zipB = await buildZip({ "ppt/slides/_rels/slide1.xml.rels": relsB, "ppt/slides/slide1.xml": partB });

    const outA = await repackageDeterministically(zipA);
    const outB = await repackageDeterministically(zipB);

    expect(Buffer.compare(outA, outB)).toBe(0);

    const resultZip = await JSZip.loadAsync(outA);
    const resultPart = await resultZip.file("ppt/slides/slide1.xml")!.async("string");
    expect(resultPart).toContain('r:id="rIdHyperlink1"');
  });

  it("is idempotent — repackaging its own output again is a no-op", async () => {
    const zip = await buildZip({ "docProps/core.xml": coreXml("2020-01-01T00:00:00Z"), "content.xml": "<a/>" });
    const once = await repackageDeterministically(zip);
    const twice = await repackageDeterministically(once);
    expect(Buffer.compare(once, twice)).toBe(0);
  });
});
