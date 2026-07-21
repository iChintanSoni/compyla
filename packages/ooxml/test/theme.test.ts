import JSZip from "jszip";
import { describe, expect, it } from "vitest";
import { readOoxmlTheme } from "../src/theme.js";

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

async function buildZip(files: Record<string, string>): Promise<JSZip> {
  const zip = new JSZip();
  for (const [path, content] of Object.entries(files)) zip.file(path, content);
  return zip;
}

describe("readOoxmlTheme", () => {
  it("parses fonts and colors from a docx-style theme path", async () => {
    const zip = await buildZip({ "word/theme/theme1.xml": THEME_XML });
    const theme = await readOoxmlTheme(zip, "word/theme/theme1.xml");
    expect(theme?.fonts).toEqual(["Calibri Light", "Calibri"]);
    expect(theme?.colors).toEqual(["000000", "FFFFFF", "4472C4"]);
  });

  it("parses the identical schema from a pptx-style theme path", async () => {
    const zip = await buildZip({ "ppt/theme/theme1.xml": THEME_XML });
    const theme = await readOoxmlTheme(zip, "ppt/theme/theme1.xml");
    expect(theme?.fonts).toEqual(["Calibri Light", "Calibri"]);
  });

  it("returns undefined when the theme path is absent", async () => {
    const zip = await buildZip({});
    const theme = await readOoxmlTheme(zip, "ppt/theme/theme1.xml");
    expect(theme).toBeUndefined();
  });
});
