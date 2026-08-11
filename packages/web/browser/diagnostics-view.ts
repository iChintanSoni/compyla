import type { Diagnostic } from "@compyla/sdk";

export function sortDiagnostics(diagnostics: readonly Diagnostic[]): Diagnostic[] {
  return [...diagnostics].sort((a, b) => a.line - b.line);
}

export function formatStatus(diagnostics: readonly Diagnostic[], ok: boolean): string {
  if (ok) return "no errors";
  return `${diagnostics.length} issue${diagnostics.length === 1 ? "" : "s"}`;
}

export function renderEmptyState(): HTMLLIElement {
  const li = document.createElement("li");
  li.className = "empty";
  li.textContent = "Paste or drop markdown to see diagnostics.";
  return li;
}

export function renderOkState(): HTMLLIElement {
  const li = document.createElement("li");
  li.className = "ok";
  li.textContent = "No issues — looks good.";
  return li;
}

export function renderDiagnostic(d: Diagnostic): HTMLLIElement {
  const li = document.createElement("li");
  li.dataset.severity = d.severity;

  const severity = document.createElement("span");
  severity.className = "diag-severity";
  severity.textContent = d.severity;

  const line = document.createElement("span");
  line.className = "diag-line";
  line.textContent = `line ${d.line}`;

  const code = document.createElement("span");
  code.className = "diag-code";
  code.textContent = d.code;

  const head = document.createElement("div");
  head.className = "diag-head";
  head.append(severity, line, code);

  const message = document.createElement("div");
  message.className = "diag-message";
  message.textContent = d.message;

  li.append(head, message);

  if (d.hint) {
    const hint = document.createElement("div");
    hint.className = "diag-hint";
    hint.textContent = `hint: ${d.hint}`;
    li.append(hint);
  }

  return li;
}

/** Empty-list case renders the "looks good" state, not an empty <ul> — silence would read as "still
 *  loading," not "nothing to report." */
export function renderDiagnosticList(diagnostics: readonly Diagnostic[]): HTMLLIElement[] {
  if (diagnostics.length === 0) return [renderOkState()];
  return sortDiagnostics(diagnostics).map(renderDiagnostic);
}
