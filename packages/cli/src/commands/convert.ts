import { existsSync } from "node:fs";
import { readFile, writeFile } from "node:fs/promises";
import { basename, dirname, extname, join } from "node:path";
import { convert } from "@compyla/sdk";
import { formatDiagnostics } from "../format-diagnostics.js";

export interface ConvertOptions {
  readonly format: string;
  readonly template?: string;
  readonly out?: string;
  readonly assets?: string;
  readonly strict?: boolean;
}

export async function runConvert(inputPath: string, options: ConvertOptions): Promise<number> {
  const source = await readFile(inputPath, "utf8");
  const assetsDir = options.assets ?? dirname(inputPath);
  const templateBuffer = options.template ? await readFile(options.template) : undefined;

  let result;
  try {
    result = await convert(source, {
      format: options.format,
      strict: options.strict ?? false,
      template: templateBuffer,
      resolveAsset: (src) => existsSync(join(assetsDir, src)),
      readAsset: (src) => readFile(join(assetsDir, src)),
    });
  } catch (err) {
    // Renderer load failures (unsupported/uninstalled format) throw — everything else about a
    // valid, blockable, or successful conversion is expressed in the returned result instead.
    console.error(err instanceof Error ? err.message : String(err));
    return 1;
  }

  if (result.diagnostics.length > 0) {
    console.error(formatDiagnostics(inputPath, result.diagnostics));
  }

  if (!result.buffer) {
    return 1; // blocked at pre-render validation — nothing to write
  }

  const outPath = options.out ?? defaultOutPath(inputPath, options.format);
  await writeFile(outPath, result.buffer);
  console.log(`Wrote ${outPath}`);

  return result.ok ? 0 : 1;
}

function defaultOutPath(inputPath: string, format: string): string {
  const name = basename(inputPath, extname(inputPath));
  return join(dirname(inputPath), `${name}.${format}`);
}
