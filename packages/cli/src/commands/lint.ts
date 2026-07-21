import { existsSync } from "node:fs";
import { readFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { buildDocument, escalateStrict, parseMarkdown, validate } from "@compyla/core";
import { formatDiagnostics } from "../format-diagnostics.js";

export interface LintOptions {
  readonly format: string;
  readonly assets?: string;
  readonly strict?: boolean;
}

export async function runLint(inputPath: string, options: LintOptions): Promise<number> {
  const source = await readFile(inputPath, "utf8");
  const doc = buildDocument(parseMarkdown(source));
  const assetsDir = options.assets ?? dirname(inputPath);

  const diagnostics = escalateStrict(
    validate(doc, { format: options.format, resolveAsset: (src) => existsSync(join(assetsDir, src)) }),
    options.strict ?? false,
  );

  if (diagnostics.length === 0) {
    console.log(`${inputPath}: OK (${options.format})`);
    return 0;
  }

  console.error(formatDiagnostics(inputPath, diagnostics));
  return diagnostics.some((d) => d.severity === "error") ? 1 : 0;
}
