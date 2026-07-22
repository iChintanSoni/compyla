import type { PptxLayoutName } from "@compyla/core";
import type { OoxmlPlaceholder, OoxmlSlideLayout } from "@compyla/ooxml";
import { SLIDE_LAYOUTS, type PlaceholderSpec } from "./layout.js";

export interface LayoutBinding {
  readonly placeholders: readonly PlaceholderSpec[];
  /** "template" when the canonical name resolved to a real layout in the uploaded template
   *  (by explicit alias or by name); "house" when it fell back to Compyla's built-in layout. */
  readonly source: "template" | "house";
}

/**
 * Binds each of Compyla's canonical pptx layout names to a real layout in an uploaded template
 * (markdown-spec.md §7.3): (a) an explicit alias, (b) case-insensitive match against the
 * template's own layout names, else (c) the house layout definition. A matched real layout
 * contributes geometry (position/size) placeholder-by-placeholder on a best-effort basis — a role
 * the real layout doesn't have (e.g. no "pic" placeholder) keeps the house default for that one
 * placeholder rather than discarding the whole match.
 */
export function bindLayouts(
  templateLayouts: readonly OoxmlSlideLayout[],
  explicitAliases: Readonly<Partial<Record<PptxLayoutName, string>>> = {},
): Record<PptxLayoutName, LayoutBinding> {
  const byLowerName = new Map(templateLayouts.map((l) => [l.name.toLowerCase(), l] as const));

  const bindings = {} as Record<PptxLayoutName, LayoutBinding>;
  for (const canonical of Object.keys(SLIDE_LAYOUTS) as PptxLayoutName[]) {
    const alias = explicitAliases[canonical];
    const matched = (alias && byLowerName.get(alias.toLowerCase())) || byLowerName.get(canonical.toLowerCase());

    bindings[canonical] = matched
      ? { placeholders: mergeGeometry(SLIDE_LAYOUTS[canonical], matched), source: "template" }
      : { placeholders: SLIDE_LAYOUTS[canonical], source: "house" };
  }
  return bindings;
}

type PlaceholderRole = PlaceholderSpec["type"];

function classify(type: string): PlaceholderRole {
  if (type === "title" || type === "ctrTitle") return "title";
  if (type === "pic") return "pic";
  return "body"; // subTitle, body, and any other/omitted type all behave as a body role positionally
}

function mergeGeometry(house: readonly PlaceholderSpec[], real: OoxmlSlideLayout): PlaceholderSpec[] {
  const pool: Record<PlaceholderRole, OoxmlPlaceholder[]> = { title: [], body: [], pic: [] };
  for (const ph of [...real.placeholders].sort((a, b) => (a.idx ?? 0) - (b.idx ?? 0))) {
    pool[classify(ph.type)].push(ph);
  }

  return house.map((spec) => {
    const candidate = pool[spec.type].shift();
    if (!candidate || candidate.x === undefined || candidate.y === undefined || candidate.w === undefined || candidate.h === undefined) {
      return spec;
    }
    return { ...spec, x: candidate.x, y: candidate.y, w: candidate.w, h: candidate.h };
  });
}
