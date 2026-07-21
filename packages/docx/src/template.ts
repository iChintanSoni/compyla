import { XMLParser } from "fast-xml-parser";
import JSZip from "jszip";

export interface StyleInfo {
  readonly id: string;
  readonly name?: string;
  readonly font?: string;
  readonly sizeHalfPoints?: number;
  readonly color?: string;
  readonly bold?: boolean;
}

export interface TemplateIssue {
  readonly code: string;
  readonly message: string;
}

export interface TemplateTheme {
  readonly fonts: readonly string[];
  readonly colors: readonly string[];
}

export interface TemplateReport {
  readonly kind: "docx";
  readonly styles: readonly StyleInfo[];
  readonly theme: TemplateTheme;
  readonly issues: readonly TemplateIssue[];
}

const xmlParser = new XMLParser({ ignoreAttributes: false, attributeNamePrefix: "@_" });

export async function introspectTemplate(buffer: Buffer): Promise<TemplateReport> {
  const zip = await JSZip.loadAsync(buffer);
  const issues: TemplateIssue[] = [];

  const styles = await readStyles(zip, issues);
  const theme = await readTheme(zip, issues);

  return { kind: "docx", styles, theme, issues };
}

// Reasonable defaults for callers that pass no template (Compyla ships a house style).
export function defaultTemplateReport(): TemplateReport {
  return { kind: "docx", styles: [], theme: { fonts: [], colors: [] }, issues: [] };
}

interface RawStyle {
  readonly "@_w:styleId"?: string;
  readonly "w:name"?: { readonly "@_w:val"?: string };
  readonly "w:rPr"?: {
    readonly "w:rFonts"?: { readonly "@_w:ascii"?: string };
    readonly "w:sz"?: { readonly "@_w:val"?: string };
    readonly "w:color"?: { readonly "@_w:val"?: string };
    readonly "w:b"?: unknown;
  };
}

async function readStyles(zip: JSZip, issues: TemplateIssue[]): Promise<StyleInfo[]> {
  const file = zip.file("word/styles.xml");
  if (!file) {
    issues.push({
      code: "no_styles_xml",
      message: "Template has no word/styles.xml; the default house style will be used.",
    });
    return [];
  }

  const xml = await file.async("string");
  const parsed = xmlParser.parse(xml) as {
    "w:styles"?: { "w:style"?: RawStyle | RawStyle[] };
  };
  const rawStyles = toArray(parsed["w:styles"]?.["w:style"]);

  const styles: StyleInfo[] = [];
  for (const raw of rawStyles) {
    const id = raw["@_w:styleId"];
    if (!id) continue;
    const rPr = raw["w:rPr"];
    styles.push({
      id,
      name: raw["w:name"]?.["@_w:val"],
      font: rPr?.["w:rFonts"]?.["@_w:ascii"],
      sizeHalfPoints: rPr?.["w:sz"]?.["@_w:val"] !== undefined ? Number(rPr["w:sz"]["@_w:val"]) : undefined,
      color: rPr?.["w:color"]?.["@_w:val"],
      bold: rPr?.["w:b"] !== undefined,
    });
  }
  return styles;
}

interface ThemeLatinFont {
  readonly "@_typeface"?: string;
}
interface ThemeColorNode {
  readonly "a:srgbClr"?: { readonly "@_val"?: string };
  readonly "a:sysClr"?: { readonly "@_lastClr"?: string };
}

async function readTheme(zip: JSZip, issues: TemplateIssue[]): Promise<TemplateTheme> {
  const file = zip.file("word/theme/theme1.xml");
  if (!file) {
    issues.push({
      code: "no_theme",
      message: "Template has no word/theme/theme1.xml; default fonts and colors will be used.",
    });
    return { fonts: [], colors: [] };
  }

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

function toArray<T>(value: T | T[] | undefined): T[] {
  if (value === undefined) return [];
  return Array.isArray(value) ? value : [value];
}
