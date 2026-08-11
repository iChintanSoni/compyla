import { readFile } from "node:fs/promises";
import { extname } from "node:path";
import { inspectTemplate } from "@compyla/sdk";

const KIND_BY_EXT: Record<string, string> = { ".docx": "docx", ".pptx": "pptx" };

export async function runTemplateInspect(filePath: string): Promise<number> {
  const ext = extname(filePath).toLowerCase();
  const kind = KIND_BY_EXT[ext];
  if (!kind) {
    console.error(
      `Cannot inspect "${filePath}": unsupported template extension "${ext}". Supported: ${Object.keys(KIND_BY_EXT).join(", ")}.`,
    );
    return 1;
  }

  const buffer = await readFile(filePath);
  let report: unknown;
  try {
    report = await inspectTemplate(buffer, kind);
  } catch (err) {
    console.error(err instanceof Error ? err.message : String(err));
    return 1;
  }

  console.log(JSON.stringify(report, null, 2));
  return 0;
}
