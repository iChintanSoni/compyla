import { buildDocument, escalateStrict, parseMarkdown, validate, type Diagnostic } from "@compyla/core";

export interface LintOptions {
  readonly format: string;
  readonly strict?: boolean;
  readonly resolveAsset?: (src: string) => boolean;
}

export interface LintResult {
  readonly diagnostics: readonly Diagnostic[];
  readonly ok: boolean;
}

export function lint(source: string, options: LintOptions): LintResult {
  const doc = buildDocument(parseMarkdown(source));
  const diagnostics = escalateStrict(
    validate(doc, { format: options.format, resolveAsset: options.resolveAsset }),
    options.strict ?? false,
  );
  return { diagnostics, ok: !diagnostics.some((d) => d.severity === "error") };
}
