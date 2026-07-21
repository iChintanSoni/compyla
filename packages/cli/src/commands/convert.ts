import { existsSync } from "node:fs";
import { readFile, writeFile } from "node:fs/promises";
import { basename, dirname, extname, join } from "node:path";
import { buildDocument, escalateStrict, parseMarkdown, validate, type Diagnostic } from "@compyla/core";
import { formatDiagnostics } from "../format-diagnostics.js";
import { loadRenderer } from "../renderer-registry.js";

export interface ConvertOptions {
  readonly format: string;
  readonly template?: string;
  readonly out?: string;
  readonly assets?: string;
  readonly strict?: boolean;
}

export async function runConvert(inputPath: string, options: ConvertOptions): Promise<number> {
  const source = await readFile(inputPath, "utf8");
  const doc = buildDocument(parseMarkdown(source));
  const assetsDir = options.assets ?? dirname(inputPath);
  const strict = options.strict ?? false;

  // Validation happens (and can block) before any rendering — errors here mean the document is
  // structurally invalid for the target profile, not just imperfectly styled.
  const validationDiagnostics = validate(doc, {
    format: options.format,
    resolveAsset: (src) => existsSync(join(assetsDir, src)),
  });
  const preRenderErrors = escalateStrict(validationDiagnostics, strict).filter((d) => d.severity === "error");
  if (preRenderErrors.length > 0) {
    console.error(formatDiagnostics(inputPath, escalateStrict(validationDiagnostics, strict)));
    return 1;
  }

  let renderer;
  try {
    renderer = await loadRenderer(options.format);
  } catch (err) {
    console.error(err instanceof Error ? err.message : String(err));
    return 1;
  }

  const templateBuffer = options.template ? await readFile(options.template) : undefined;
  const { buffer, diagnostics: renderDiagnostics } = await renderer.render(doc, {
    template: templateBuffer,
    resolveAsset: (src: string) => readFile(join(assetsDir, src)),
  });

  const allDiagnostics: readonly Diagnostic[] = escalateStrict([...validationDiagnostics, ...renderDiagnostics], strict);
  if (allDiagnostics.length > 0) {
    console.error(formatDiagnostics(inputPath, allDiagnostics));
  }

  const outPath = options.out ?? defaultOutPath(inputPath, options.format);
  await writeFile(outPath, buffer);
  console.log(`Wrote ${outPath}`);

  return allDiagnostics.some((d) => d.severity === "error") ? 1 : 0;
}

function defaultOutPath(inputPath: string, format: string): string {
  const name = basename(inputPath, extname(inputPath));
  return join(dirname(inputPath), `${name}.${format}`);
}
