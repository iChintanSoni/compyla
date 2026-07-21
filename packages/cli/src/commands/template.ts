import { readFile } from "node:fs/promises";
import { extname } from "node:path";
import { isModuleNotFoundError } from "../module-errors.js";

interface TemplateIntrospector {
  readonly pkg: string;
  readonly load: () => Promise<{ introspectTemplate: (buffer: Buffer) => Promise<unknown> }>;
}

const INTROSPECTORS: Record<string, TemplateIntrospector> = {
  ".docx": { pkg: "@compyla/docx", load: () => import("@compyla/docx") },
  ".pptx": { pkg: "@compyla/pptx", load: () => import("@compyla/pptx") },
};

export async function runTemplateInspect(filePath: string): Promise<number> {
  const ext = extname(filePath).toLowerCase();
  const introspector = INTROSPECTORS[ext];
  if (!introspector) {
    console.error(`Cannot inspect "${filePath}": unsupported template extension "${ext}". Supported: ${Object.keys(INTROSPECTORS).join(", ")}.`);
    return 1;
  }

  let mod;
  try {
    mod = await introspector.load();
  } catch (err) {
    if (isModuleNotFoundError(err)) {
      console.error(`Template inspection requires ${introspector.pkg}. Install it with:\n       npm i ${introspector.pkg}`);
      return 1;
    }
    throw err;
  }

  const buffer = await readFile(filePath);
  const report = await mod.introspectTemplate(buffer);
  console.log(JSON.stringify(report, null, 2));
  return 0;
}
