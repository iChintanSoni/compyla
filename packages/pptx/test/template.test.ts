import JSZip from "jszip";
import { describe, expect, it } from "vitest";
import { introspectTemplate } from "../src/template.js";

const THEME_XML = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" name="Acme">
  <a:themeElements>
    <a:clrScheme name="Acme">
      <a:dk1><a:sysClr val="windowText" lastClr="000000"/></a:dk1>
      <a:lt1><a:sysClr val="window" lastClr="FFFFFF"/></a:lt1>
      <a:accent1><a:srgbClr val="4472C4"/></a:accent1>
    </a:clrScheme>
    <a:fontScheme name="Acme">
      <a:majorFont><a:latin typeface="Calibri Light"/></a:majorFont>
      <a:minorFont><a:latin typeface="Calibri"/></a:minorFont>
    </a:fontScheme>
  </a:themeElements>
</a:theme>`;

async function buildFixtureZip(files: Record<string, string>): Promise<Buffer> {
  const zip = new JSZip();
  for (const [path, content] of Object.entries(files)) zip.file(path, content);
  return zip.generateAsync({ type: "nodebuffer" });
}

describe("introspectTemplate (pptx)", () => {
  it("reads theme fonts and colors from ppt/theme/theme1.xml", async () => {
    const buffer = await buildFixtureZip({ "ppt/theme/theme1.xml": THEME_XML });
    const report = await introspectTemplate(buffer);
    expect(report.kind).toBe("pptx");
    expect(report.theme.fonts).toEqual(["Calibri Light", "Calibri"]);
    expect(report.theme.colors).toContain("4472C4");
    expect(report.issues).toEqual([]);
  });

  it("reports an issue and empty theme when theme1.xml is missing", async () => {
    const buffer = await buildFixtureZip({});
    const report = await introspectTemplate(buffer);
    expect(report.theme).toEqual({ fonts: [], colors: [] });
    expect(report.issues).toContainEqual(expect.objectContaining({ code: "no_theme" }));
  });
});
