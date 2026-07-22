import JSZip from "jszip";
import { describe, expect, it } from "vitest";
import { readOoxmlSlideLayouts } from "../src/slide-layouts.js";

const NS =
  'xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"';

function slideLayoutXml(name: string, placeholders: string): string {
  return `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sldLayout ${NS}>
  <p:cSld name="${name}">
    <p:spTree>
      ${placeholders}
    </p:spTree>
  </p:cSld>
</p:sldLayout>`;
}

function placeholderShape(type: string | undefined, idx: number | undefined, x: number, y: number, cx: number, cy: number): string {
  const phAttrs = [type ? `type="${type}"` : undefined, idx !== undefined ? `idx="${idx}"` : undefined].filter(Boolean).join(" ");
  return `<p:sp>
    <p:nvSpPr><p:nvPr><p:ph ${phAttrs}/></p:nvPr></p:nvSpPr>
    <p:spPr><a:xfrm><a:off x="${x}" y="${y}"/><a:ext cx="${cx}" cy="${cy}"/></a:xfrm></p:spPr>
  </p:sp>`;
}

async function buildZip(files: Record<string, string>): Promise<JSZip> {
  const zip = new JSZip();
  for (const [path, content] of Object.entries(files)) zip.file(path, content);
  return zip;
}

describe("readOoxmlSlideLayouts", () => {
  it("reads layout name and placeholder type/geometry, converting EMU to inches", async () => {
    const xml = slideLayoutXml(
      "Title and Content",
      placeholderShape("title", undefined, 457200, 274638, 8229600, 838200) + placeholderShape("body", 1, 457200, 1600200, 8229600, 4525963),
    );
    const zip = await buildZip({ "ppt/slideLayouts/slideLayout1.xml": xml });
    const layouts = await readOoxmlSlideLayouts(zip);

    expect(layouts).toHaveLength(1);
    expect(layouts[0]!.name).toBe("Title and Content");
    expect(layouts[0]!.placeholders).toHaveLength(2);
    expect(layouts[0]!.placeholders[0]).toMatchObject({ type: "title", x: 0.5, y: expect.closeTo(0.3003, 3) });
    expect(layouts[0]!.placeholders[1]).toMatchObject({ type: "body", idx: 1 });
  });

  it("defaults a placeholder's type to body when the type attribute is absent", async () => {
    const xml = slideLayoutXml("Blank Body", placeholderShape(undefined, 1, 0, 0, 100, 100));
    const zip = await buildZip({ "ppt/slideLayouts/slideLayout1.xml": xml });
    const layouts = await readOoxmlSlideLayouts(zip);
    expect(layouts[0]!.placeholders[0]!.type).toBe("body");
  });

  it("skips non-placeholder shapes", async () => {
    const xml = slideLayoutXml(
      "Decorated",
      `<p:sp><p:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="10" cy="10"/></a:xfrm></p:spPr></p:sp>`,
    );
    const zip = await buildZip({ "ppt/slideLayouts/slideLayout1.xml": xml });
    const layouts = await readOoxmlSlideLayouts(zip);
    expect(layouts[0]!.placeholders).toHaveLength(0);
  });

  it("reads multiple slide layouts in numeric order", async () => {
    const zip = await buildZip({
      "ppt/slideLayouts/slideLayout2.xml": slideLayoutXml("Second", ""),
      "ppt/slideLayouts/slideLayout10.xml": slideLayoutXml("Tenth", ""),
      "ppt/slideLayouts/slideLayout1.xml": slideLayoutXml("First", ""),
    });
    const layouts = await readOoxmlSlideLayouts(zip);
    expect(layouts.map((l) => l.name)).toEqual(["First", "Second", "Tenth"]);
  });

  it("returns an empty array when the template has no slide layouts", async () => {
    const zip = await buildZip({});
    expect(await readOoxmlSlideLayouts(zip)).toEqual([]);
  });
});
