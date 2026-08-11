import { buildDocument, escalateStrict, parseMarkdown, validate, type Diagnostic } from "@compyla/core";
import { loadRenderer } from "./renderer-registry.js";

export interface ConvertOptions {
  readonly format: string;
  readonly strict?: boolean;
  readonly template?: Buffer;
  /** Validation-time existence check for referenced assets (e.g. images). */
  readonly resolveAsset?: (src: string) => boolean;
  /** Render-time content read for referenced assets. */
  readonly readAsset?: (src: string) => Promise<Buffer> | Buffer;
}

export interface ConvertResult {
  /** Absent only when a pre-render validation error blocked rendering. */
  readonly buffer?: Buffer;
  readonly diagnostics: readonly Diagnostic[];
  readonly ok: boolean;
}

export async function convert(source: string, options: ConvertOptions): Promise<ConvertResult> {
  const doc = buildDocument(parseMarkdown(source));
  const strict = options.strict ?? false;

  // Validation happens (and can block) before any rendering — errors here mean the document is
  // structurally invalid for the target profile, not just imperfectly styled.
  const validationDiagnostics = validate(doc, { format: options.format, resolveAsset: options.resolveAsset });
  const preRenderErrors = escalateStrict(validationDiagnostics, strict).filter((d) => d.severity === "error");
  if (preRenderErrors.length > 0) {
    return { diagnostics: escalateStrict(validationDiagnostics, strict), ok: false };
  }

  const renderer = await loadRenderer(options.format);
  const { buffer, diagnostics: renderDiagnostics } = await renderer.render(doc, {
    template: options.template,
    resolveAsset: options.readAsset,
  });

  const diagnostics = escalateStrict([...validationDiagnostics, ...renderDiagnostics], strict);
  return { buffer, diagnostics, ok: !diagnostics.some((d) => d.severity === "error") };
}
