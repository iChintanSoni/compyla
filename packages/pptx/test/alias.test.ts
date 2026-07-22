import type { OoxmlSlideLayout } from "@compyla/ooxml";
import { describe, expect, it } from "vitest";
import { bindLayouts } from "../src/alias.js";
import { SLIDE_LAYOUTS } from "../src/layout.js";

function layout(name: string, placeholders: OoxmlSlideLayout["placeholders"]): OoxmlSlideLayout {
  return { name, placeholders };
}

describe("bindLayouts", () => {
  it("falls back to the house layout when the template has no matching layout", () => {
    const bindings = bindLayouts([layout("Some Custom Layout", [])]);
    expect(bindings["Title and Content"]).toEqual({ source: "house", placeholders: SLIDE_LAYOUTS["Title and Content"] });
  });

  it("matches a template layout by case-insensitive name and overrides placeholder geometry", () => {
    const templateLayouts = [
      layout("title and content", [
        { type: "title", idx: 1, x: 2, y: 1, w: 6, h: 3 },
        { type: "body", idx: 2, x: 2, y: 4, w: 6, h: 1.5 },
      ]),
    ];
    const bindings = bindLayouts(templateLayouts);
    const binding = bindings["Title and Content"];
    expect(binding.source).toBe("template");
    expect(binding.placeholders.find((p) => p.name === "title")).toMatchObject({ x: 2, y: 1, w: 6, h: 3 });
    expect(binding.placeholders.find((p) => p.name === "body")).toMatchObject({ x: 2, y: 4, w: 6, h: 1.5 });
  });

  it("prefers an explicit alias over the template's own name match", () => {
    const templateLayouts = [
      layout("Title and Content", [{ type: "title", idx: 1, x: 0, y: 0, w: 1, h: 1 }]),
      layout("Aliased Layout", [{ type: "title", idx: 1, x: 3, y: 3, w: 4, h: 2 }]),
    ];
    const bindings = bindLayouts(templateLayouts, { "Title and Content": "Aliased Layout" });
    expect(bindings["Title and Content"].placeholders.find((p) => p.name === "title")).toMatchObject({ x: 3, y: 3, w: 4, h: 2 });
  });

  it("keeps a per-placeholder house default when the matched layout is missing that role", () => {
    // Matched by name, but the real layout only has a title placeholder — no body.
    const templateLayouts = [layout("Title and Content", [{ type: "title", idx: 1, x: 2, y: 1, w: 6, h: 3 }])];
    const bindings = bindLayouts(templateLayouts);
    const binding = bindings["Title and Content"];
    expect(binding.source).toBe("template");
    expect(binding.placeholders.find((p) => p.name === "title")).toMatchObject({ x: 2, y: 1, w: 6, h: 3 });
    expect(binding.placeholders.find((p) => p.name === "body")).toEqual(
      SLIDE_LAYOUTS["Title and Content"].find((p) => p.name === "body"),
    );
  });

  it("classifies subTitle and untyped placeholders as the body role", () => {
    const templateLayouts = [
      layout("Title Slide", [
        { type: "ctrTitle", idx: 1, x: 1, y: 1, w: 5, h: 1 },
        { type: "subTitle", idx: 2, x: 1, y: 2, w: 5, h: 1 },
      ]),
    ];
    const bindings = bindLayouts(templateLayouts);
    expect(bindings["Title Slide"].placeholders.find((p) => p.name === "title")).toMatchObject({ x: 1, y: 1 });
    expect(bindings["Title Slide"].placeholders.find((p) => p.name === "subtitle")).toMatchObject({ x: 1, y: 2 });
  });
});
