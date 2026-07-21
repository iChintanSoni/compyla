import { readFile } from "node:fs/promises";
import { isModuleNotFoundError } from "../module-errors.js";

export async function runTemplateInspect(filePath: string): Promise<number> {
  let mod;
  try {
    mod = await import("@compyla/docx");
  } catch (err) {
    if (isModuleNotFoundError(err)) {
      console.error("Template inspection requires @compyla/docx. Install it with:\n       npm i @compyla/docx");
      return 1;
    }
    throw err;
  }

  const buffer = await readFile(filePath);
  const report = await mod.introspectTemplate(buffer);
  console.log(JSON.stringify(report, null, 2));
  return 0;
}
