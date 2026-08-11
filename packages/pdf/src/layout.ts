import type { DocumentMeta } from "@compyla/core";

// Page geometry. `page_size`/`margins` are markdown-spec.md §6's only pdf-specific frontmatter keys.
// `margins` has no numeric spec — these two presets deliberately match Word's own "Normal"/"Narrow"
// margin preset names and point values, since pdf shares docx's authoring surface.
export const PAGE_SIZES = ["A4", "Letter"] as const;
export type PageSizeName = (typeof PAGE_SIZES)[number];

export const MARGIN_PRESETS = ["normal", "narrow"] as const;
export type MarginPresetName = (typeof MARGIN_PRESETS)[number];

const MARGIN_POINTS: Record<MarginPresetName, number> = {
  normal: 72, // 1in
  narrow: 36, // 0.5in
};

// A4/Letter in points (72pt/in) — pdfkit also accepts these size names directly, but we need the
// numeric height/width ourselves for TOC reservation and pagination math.
const PAGE_DIMENSIONS_PT: Record<PageSizeName, { width: number; height: number }> = {
  A4: { width: 595.28, height: 841.89 },
  Letter: { width: 612, height: 792 },
};

export interface PageLayout {
  readonly size: PageSizeName;
  readonly marginPt: number;
  readonly pageWidth: number;
  readonly pageHeight: number;
  readonly contentWidth: number;
  readonly contentHeight: number;
}

export function resolvePageSize(value: unknown): PageSizeName {
  return typeof value === "string" && (PAGE_SIZES as readonly string[]).includes(value) ? (value as PageSizeName) : "A4";
}

export function resolveMargins(value: unknown): MarginPresetName {
  return typeof value === "string" && (MARGIN_PRESETS as readonly string[]).includes(value) ? (value as MarginPresetName) : "normal";
}

export function resolvePageLayout(meta: DocumentMeta): PageLayout {
  const size = resolvePageSize(meta.page_size);
  const marginPreset = resolveMargins(meta.margins);
  const marginPt = MARGIN_POINTS[marginPreset];
  const { width: pageWidth, height: pageHeight } = PAGE_DIMENSIONS_PT[size];

  return {
    size,
    marginPt,
    pageWidth,
    pageHeight,
    contentWidth: pageWidth - marginPt * 2,
    contentHeight: pageHeight - marginPt * 2,
  };
}
