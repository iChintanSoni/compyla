import { isModuleNotFoundError } from "./module-errors.js";

interface TemplateIntrospector {
  readonly pkg: string;
  readonly load: () => Promise<{ introspectTemplate: (buffer: Buffer) => Promise<unknown> }>;
}

// Kinds mirror the renderer registry's opt-in-install model, keyed by template kind rather than
// output format (docx/pptx templates are introspected; xlsx/pdf have no template introspection yet).
const INTROSPECTORS: Record<string, TemplateIntrospector> = {
  docx: { pkg: "@compyla/docx", load: () => import("@compyla/docx") },
  pptx: { pkg: "@compyla/pptx", load: () => import("@compyla/pptx") },
};

export class UnsupportedTemplateKindError extends Error {
  constructor(kind: string) {
    super(`Cannot inspect template kind "${kind}". Supported: ${Object.keys(INTROSPECTORS).join(", ")}.`);
  }
}

export class TemplateIntrospectorNotInstalledError extends Error {
  constructor(kind: string, pkg: string) {
    super(`Inspecting a "${kind}" template requires ${pkg}. Install it with:\n       npm i ${pkg}`);
  }
}

export async function inspectTemplate(buffer: Buffer, kind: string): Promise<unknown> {
  const introspector = INTROSPECTORS[kind];
  if (!introspector) throw new UnsupportedTemplateKindError(kind);
  let mod;
  try {
    mod = await introspector.load();
  } catch (err) {
    if (isModuleNotFoundError(err)) throw new TemplateIntrospectorNotInstalledError(kind, introspector.pkg);
    throw err;
  }
  return mod.introspectTemplate(buffer);
}
