import JSZip from "jszip";
import { describe, expect, it } from "vitest";
import { introspectTemplate } from "../src/template.js";

const STYLES_XML = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:style w:type="paragraph" w:styleId="Heading1">
    <w:name w:val="heading 1"/>
    <w:rPr>
      <w:rFonts w:ascii="Calibri Light" w:hAnsi="Calibri Light"/>
      <w:sz w:val="32"/>
      <w:color w:val="1F4E79"/>
      <w:b/>
    </w:rPr>
  </w:style>
  <w:style w:type="paragraph" w:styleId="Normal">
    <w:name w:val="Normal"/>
    <w:rPr>
      <w:rFonts w:ascii="Calibri" w:hAnsi="Calibri"/>
      <w:sz w:val="22"/>
    </w:rPr>
  </w:style>
</w:styles>`;

const THEME_XML = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" name="Acme">
  <a:themeElements>
    <a:clrScheme name="Acme">
      <a:dk1><a:sysClr val="windowText" lastClr="000000"/></a:dk1>
      <a:lt1><a:sysClr val="window" lastClr="FFFFFF"/></a:lt1>
      <a:dk2><a:srgbClr val="1F4E79"/></a:dk2>
      <a:lt2><a:srgbClr val="EEECE1"/></a:lt2>
      <a:accent1><a:srgbClr val="4472C4"/></a:accent1>
      <a:accent2><a:srgbClr val="ED7D31"/></a:accent2>
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

describe("introspectTemplate", () => {
  it("extracts named styles with font, size, color, and bold", async () => {
    const buffer = await buildFixtureZip({ "word/styles.xml": STYLES_XML, "word/theme/theme1.xml": THEME_XML });
    const report = await introspectTemplate(buffer);

    const heading1 = report.styles.find((s) => s.id === "Heading1");
    expect(heading1).toEqual({
      id: "Heading1",
      name: "heading 1",
      font: "Calibri Light",
      sizeHalfPoints: 32,
      color: "1F4E79",
      bold: true,
    });
  });

  it("extracts theme major/minor fonts and accent colors", async () => {
    const buffer = await buildFixtureZip({ "word/styles.xml": STYLES_XML, "word/theme/theme1.xml": THEME_XML });
    const report = await introspectTemplate(buffer);

    expect(report.theme.fonts).toEqual(["Calibri Light", "Calibri"]);
    expect(report.theme.colors).toEqual(["000000", "FFFFFF", "1F4E79", "EEECE1", "4472C4", "ED7D31"]);
  });

  it("reports an issue and returns empty styles when styles.xml is missing", async () => {
    const buffer = await buildFixtureZip({ "word/theme/theme1.xml": THEME_XML });
    const report = await introspectTemplate(buffer);

    expect(report.styles).toEqual([]);
    expect(report.issues).toContainEqual(expect.objectContaining({ code: "no_styles_xml" }));
  });

  it("reports an issue and returns empty theme when theme1.xml is missing", async () => {
    const buffer = await buildFixtureZip({ "word/styles.xml": STYLES_XML });
    const report = await introspectTemplate(buffer);

    expect(report.theme).toEqual({ fonts: [], colors: [] });
    expect(report.issues).toContainEqual(expect.objectContaining({ code: "no_theme" }));
  });

  it("does not duplicate the theme font when major and minor are the same", async () => {
    const sameFont = THEME_XML.replace('typeface="Calibri"', 'typeface="Calibri Light"');
    const buffer = await buildFixtureZip({ "word/theme/theme1.xml": sameFont });
    const report = await introspectTemplate(buffer);
    expect(report.theme.fonts).toEqual(["Calibri Light"]);
  });
});
