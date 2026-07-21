import type { Diagnostic } from "@compyla/core";

export function formatDiagnostics(filePath: string, diagnostics: readonly Diagnostic[]): string {
  return diagnostics
    .map((d) => {
      const head = `${filePath}:${d.line}: ${d.severity}: ${d.message} [${d.code}]`;
      return d.hint ? `${head}\n  hint: ${d.hint}` : head;
    })
    .join("\n");
}
