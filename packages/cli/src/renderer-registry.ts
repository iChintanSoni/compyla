import type { CompylaDocument, Diagnostic } from "@compyla/core";
import { isModuleNotFoundError } from "./module-errors.js";

export interface RenderInvocationOptions {
  readonly template?: Buffer;
  readonly resolveAsset?: (src: string) => Promise<Buffer> | Buffer;
}

export interface RenderOutcome {
  readonly buffer: Buffer;
  readonly diagnostics: readonly Diagnostic[];
}

export interface RendererModule {
  render(doc: CompylaDocument, options: RenderInvocationOptions): Promise<RenderOutcome>;
}

interface RendererEntry {
  readonly pkg: string;
  readonly load: () => Promise<RendererModule>;
}

// Phase 0 ships one renderer. Adding pptx/xlsx/pdf later means one more entry here — each
// renderer package independently opt-in installed, per the core+opt-in packaging model.
const REGISTRY: Record<string, RendererEntry> = {
  docx: {
    pkg: "@compyla/docx",
    load: async () => {
      const mod = await import("@compyla/docx");
      return { render: mod.renderDocx };
    },
  },
};

export class UnsupportedFormatError extends Error {
  constructor(format: string) {
    super(`Unknown format "${format}". Supported: ${Object.keys(REGISTRY).join(", ")}.`);
  }
}

export class RendererNotInstalledError extends Error {
  constructor(format: string, pkg: string) {
    super(`format "${format}" requires ${pkg}. Install it with:\n       npm i ${pkg}`);
  }
}

export async function loadRenderer(format: string): Promise<RendererModule> {
  const entry = REGISTRY[format];
  if (!entry) throw new UnsupportedFormatError(format);
  try {
    return await entry.load();
  } catch (err) {
    if (isModuleNotFoundError(err)) throw new RendererNotInstalledError(format, entry.pkg);
    throw err;
  }
}
