import { existsSync } from "node:fs";
import { readFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { lint } from "@compyla/sdk";
import { formatDiagnostics } from "../format-diagnostics.js";

export interface LintOptions {
  readonly format: string;
  readonly assets?: string;
  readonly strict?: boolean;
}

export async function runLint(inputPath: string, options: LintOptions): Promise<number> {
  const source = await readFile(inputPath, "utf8");
  const assetsDir = options.assets ?? dirname(inputPath);

  const result = lint(source, {
    format: options.format,
    strict: options.strict ?? false,
    resolveAsset: (src) => existsSync(join(assetsDir, src)),
  });

  if (result.diagnostics.length === 0) {
    console.log(`${inputPath}: OK (${options.format})`);
    return 0;
  }

  console.error(formatDiagnostics(inputPath, result.diagnostics));
  return result.ok ? 0 : 1;
}
