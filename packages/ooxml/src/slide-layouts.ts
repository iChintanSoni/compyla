import { XMLParser } from "fast-xml-parser";
import type JSZip from "jszip";

export interface OoxmlPlaceholder {
  /** Raw `p:ph/@type` (title, ctrTitle, subTitle, body, pic, ...); absent in the XML means the
   *  schema default of a generic body placeholder. */
  readonly type: string;
  readonly idx?: number;
  readonly x?: number;
  readonly y?: number;
  readonly w?: number;
  readonly h?: number;
}

export interface OoxmlSlideLayout {
  readonly name: string;
  readonly placeholders: readonly OoxmlPlaceholder[];
}

const EMU_PER_INCH = 914400;

const xmlParser = new XMLParser({ ignoreAttributes: false, attributeNamePrefix: "@_" });

interface RawPh {
  readonly "@_type"?: string;
  readonly "@_idx"?: string;
}
interface RawXfrm {
  readonly "a:off"?: { readonly "@_x"?: string; readonly "@_y"?: string };
  readonly "a:ext"?: { readonly "@_cx"?: string; readonly "@_cy"?: string };
}
interface RawSp {
  readonly "p:nvSpPr"?: { readonly "p:nvPr"?: { readonly "p:ph"?: RawPh } };
  readonly "p:spPr"?: { readonly "a:xfrm"?: RawXfrm };
}
interface RawSlideLayout {
  readonly "p:sldLayout"?: {
    readonly "p:cSld"?: {
      readonly "@_name"?: string;
      readonly "p:spTree"?: { readonly "p:sp"?: RawSp | RawSp[] };
    };
  };
}

/**
 * Reads every `ppt/slideLayouts/slideLayoutN.xml` part of an uploaded template: the layout's own
 * name and its placeholder shapes' type + geometry (position/size), in EMU converted to inches.
 * Used to bind Compyla's canonical pptx layout names to a real template's own layouts
 * (markdown-spec.md §7.3), rather than always falling back to the fixed house layout set.
 */
export async function readOoxmlSlideLayouts(zip: JSZip): Promise<OoxmlSlideLayout[]> {
  const paths = Object.keys(zip.files)
    .filter((p) => /^ppt\/slideLayouts\/slideLayout\d+\.xml$/.test(p))
    .sort((a, b) => layoutNumber(a) - layoutNumber(b));

  const layouts: OoxmlSlideLayout[] = [];
  for (const path of paths) {
    const xml = await zip.file(path)!.async("string");
    layouts.push(parseSlideLayout(xml));
  }
  return layouts;
}

function layoutNumber(path: string): number {
  return Number(/slideLayout(\d+)\.xml$/.exec(path)![1]);
}

function parseSlideLayout(xml: string): OoxmlSlideLayout {
  const parsed = xmlParser.parse(xml) as RawSlideLayout;
  const cSld = parsed["p:sldLayout"]?.["p:cSld"];
  const shapes = toArray(cSld?.["p:spTree"]?.["p:sp"]);

  const placeholders: OoxmlPlaceholder[] = [];
  for (const sp of shapes) {
    const ph = sp["p:nvSpPr"]?.["p:nvPr"]?.["p:ph"];
    if (!ph) continue; // a non-placeholder shape (decoration, background, ...)

    const xfrm = sp["p:spPr"]?.["a:xfrm"];
    const off = xfrm?.["a:off"];
    const ext = xfrm?.["a:ext"];
    placeholders.push({
      type: ph["@_type"] ?? "body",
      idx: ph["@_idx"] !== undefined ? Number(ph["@_idx"]) : undefined,
      x: off?.["@_x"] !== undefined ? emuToInches(off["@_x"]) : undefined,
      y: off?.["@_y"] !== undefined ? emuToInches(off["@_y"]) : undefined,
      w: ext?.["@_cx"] !== undefined ? emuToInches(ext["@_cx"]) : undefined,
      h: ext?.["@_cy"] !== undefined ? emuToInches(ext["@_cy"]) : undefined,
    });
  }

  return { name: cSld?.["@_name"] ?? "", placeholders };
}

function emuToInches(value: string): number {
  return Number(value) / EMU_PER_INCH;
}

function toArray<T>(value: T | T[] | undefined): T[] {
  if (value === undefined) return [];
  return Array.isArray(value) ? value : [value];
}
