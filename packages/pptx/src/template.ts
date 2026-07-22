import { readOoxmlSlideLayouts, readOoxmlTheme, type OoxmlSlideLayout, type OoxmlTheme } from "@compyla/ooxml";
import JSZip from "jszip";

export interface TemplateIssue {
  readonly code: string;
  readonly message: string;
}

export type TemplateTheme = OoxmlTheme;

export interface TemplateReport {
  readonly kind: "pptx";
  readonly theme: TemplateTheme;
  /** Real slide layouts read from the uploaded template's `ppt/slideLayouts/`, used to bind
   *  Compyla's canonical layout names to the template's own layouts (see ./alias.ts). Empty when
   *  no template was supplied. */
  readonly layouts: readonly OoxmlSlideLayout[];
  readonly issues: readonly TemplateIssue[];
}

/**
 * Template introspection covers the theme (fonts/colors, read via the DrawingML schema shared
 * with docx, @compyla/ooxml) and the template's own slide layouts (names + placeholder geometry).
 * pptxgenjs (the renderer) still builds its slide masters itself rather than literally reusing the
 * template's layout XML — see architecture.md D3 — but ./alias.ts uses these parsed layouts to
 * bind Compyla's house layout names to the template's real placeholder positions.
 */
export async function introspectTemplate(buffer: Buffer): Promise<TemplateReport> {
  const zip = await JSZip.loadAsync(buffer);
  const theme = await readOoxmlTheme(zip, "ppt/theme/theme1.xml");
  const layouts = await readOoxmlSlideLayouts(zip);
  const issues: TemplateIssue[] = theme
    ? []
    : [{ code: "no_theme", message: "Template has no ppt/theme/theme1.xml; default fonts and colors will be used." }];

  return { kind: "pptx", theme: theme ?? { fonts: [], colors: [] }, layouts, issues };
}

export function defaultTemplateReport(): TemplateReport {
  return { kind: "pptx", theme: { fonts: [], colors: [] }, layouts: [], issues: [] };
}
