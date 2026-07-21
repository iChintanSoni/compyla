import { readOoxmlTheme, type OoxmlTheme } from "@compyla/ooxml";
import JSZip from "jszip";

export interface TemplateIssue {
  readonly code: string;
  readonly message: string;
}

export type TemplateTheme = OoxmlTheme;

export interface TemplateReport {
  readonly kind: "pptx";
  readonly theme: TemplateTheme;
  readonly issues: readonly TemplateIssue[];
}

/**
 * pptxgenjs (the renderer) builds slides from its own house layout definitions rather than
 * reusing an uploaded template's actual slide layouts (see architecture.md D3 — the same
 * simplification docx makes for styles.xml). Template introspection is therefore theme-only for
 * now: fonts and colors, read via the DrawingML schema shared with docx (@compyla/ooxml).
 */
export async function introspectTemplate(buffer: Buffer): Promise<TemplateReport> {
  const zip = await JSZip.loadAsync(buffer);
  const theme = await readOoxmlTheme(zip, "ppt/theme/theme1.xml");
  const issues: TemplateIssue[] = theme
    ? []
    : [{ code: "no_theme", message: "Template has no ppt/theme/theme1.xml; default fonts and colors will be used." }];

  return { kind: "pptx", theme: theme ?? { fonts: [], colors: [] }, issues };
}

export function defaultTemplateReport(): TemplateReport {
  return { kind: "pptx", theme: { fonts: [], colors: [] }, issues: [] };
}
