export type Severity = "error" | "warning";

export interface Diagnostic {
  readonly severity: Severity;
  readonly code: string;
  readonly line: number;
  readonly column?: number;
  readonly message: string;
  readonly hint?: string;
  /** Escalated from warning to error under --strict fidelity mode. */
  readonly strictPromotable?: boolean;
}

/**
 * Applies --strict fidelity escalation to any diagnostic list — core validation, renderer
 * warnings (e.g. an unresolved font), or the two merged together.
 */
export function escalateStrict(diagnostics: readonly Diagnostic[], strict: boolean): Diagnostic[] {
  if (!strict) return [...diagnostics];
  return diagnostics.map((d) => (d.strictPromotable && d.severity === "warning" ? { ...d, severity: "error" } : d));
}
