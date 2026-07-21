import { XMLParser } from "fast-xml-parser";
import type JSZip from "jszip";

export interface OoxmlTheme {
  readonly fonts: readonly string[];
  readonly colors: readonly string[];
}

const xmlParser = new XMLParser({ ignoreAttributes: false, attributeNamePrefix: "@_" });

interface ThemeLatinFont {
  readonly "@_typeface"?: string;
}
interface ThemeColorNode {
  readonly "a:srgbClr"?: { readonly "@_val"?: string };
  readonly "a:sysClr"?: { readonly "@_lastClr"?: string };
}

/**
 * docx and pptx templates share the identical DrawingML theme schema (`<a:theme>`, verified
 * against real output from both the `docx` and `pptxgenjs` libraries) — one parser for both.
 * `themePath` is typically `word/theme/theme1.xml` (docx) or `ppt/theme/theme1.xml` (pptx).
 */
export async function readOoxmlTheme(zip: JSZip, themePath: string): Promise<OoxmlTheme | undefined> {
  const file = zip.file(themePath);
  if (!file) return undefined;

  const xml = await file.async("string");
  const parsed = xmlParser.parse(xml) as {
    "a:theme"?: {
      "a:themeElements"?: {
        "a:fontScheme"?: {
          "a:majorFont"?: { "a:latin"?: ThemeLatinFont };
          "a:minorFont"?: { "a:latin"?: ThemeLatinFont };
        };
        "a:clrScheme"?: Record<string, ThemeColorNode>;
      };
    };
  };

  const elements = parsed["a:theme"]?.["a:themeElements"];
  const fontScheme = elements?.["a:fontScheme"];
  const clrScheme = elements?.["a:clrScheme"];

  const fonts: string[] = [];
  const majorLatin = fontScheme?.["a:majorFont"]?.["a:latin"]?.["@_typeface"];
  const minorLatin = fontScheme?.["a:minorFont"]?.["a:latin"]?.["@_typeface"];
  if (majorLatin) fonts.push(majorLatin);
  if (minorLatin && minorLatin !== majorLatin) fonts.push(minorLatin);

  const colors: string[] = [];
  const colorKeys = ["a:dk1", "a:lt1", "a:dk2", "a:lt2", "a:accent1", "a:accent2", "a:accent3", "a:accent4", "a:accent5", "a:accent6"];
  for (const key of colorKeys) {
    const node = clrScheme?.[key];
    const val = node?.["a:srgbClr"]?.["@_val"] ?? node?.["a:sysClr"]?.["@_lastClr"];
    if (val) colors.push(val);
  }

  return { fonts, colors };
}
