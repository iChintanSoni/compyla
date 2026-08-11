// @vitest-environment jsdom
import type { Diagnostic } from "@compyla/sdk";
import { describe, expect, it } from "vitest";
import {
  formatStatus,
  renderDiagnostic,
  renderDiagnosticList,
  renderEmptyState,
  renderOkState,
  sortDiagnostics,
} from "../browser/diagnostics-view.js";

function diag(overrides: Partial<Diagnostic> = {}): Diagnostic {
  return {
    severity: "error",
    code: "test_code",
    line: 1,
    message: "a message",
    ...overrides,
  };
}

describe("sortDiagnostics", () => {
  it("orders by line ascending without mutating the input", () => {
    const input = [diag({ line: 5 }), diag({ line: 1 }), diag({ line: 3 })];
    const sorted = sortDiagnostics(input);
    expect(sorted.map((d) => d.line)).toEqual([1, 3, 5]);
    expect(input.map((d) => d.line)).toEqual([5, 1, 3]);
  });
});

describe("formatStatus", () => {
  it("reports 'no errors' when ok, regardless of leftover warnings passed in", () => {
    expect(formatStatus([diag({ severity: "warning" })], true)).toBe("no errors");
  });

  it("pluralizes for more than one issue", () => {
    expect(formatStatus([diag(), diag()], false)).toBe("2 issues");
  });

  it("stays singular for exactly one issue", () => {
    expect(formatStatus([diag()], false)).toBe("1 issue");
  });
});

describe("renderDiagnostic", () => {
  it("renders severity, line, code, and message", () => {
    const li = renderDiagnostic(diag({ severity: "warning", line: 42, code: "heading_skip", message: "Skips a level" }));
    expect(li.dataset.severity).toBe("warning");
    expect(li.querySelector(".diag-severity")?.textContent).toBe("warning");
    expect(li.querySelector(".diag-line")?.textContent).toBe("line 42");
    expect(li.querySelector(".diag-code")?.textContent).toBe("heading_skip");
    expect(li.querySelector(".diag-message")?.textContent).toBe("Skips a level");
  });

  it("omits the hint element when there is no hint", () => {
    const li = renderDiagnostic(diag());
    expect(li.querySelector(".diag-hint")).toBeNull();
  });

  it("renders a hint element when a hint is present", () => {
    const li = renderDiagnostic(diag({ hint: "add the missing cell" }));
    expect(li.querySelector(".diag-hint")?.textContent).toBe("hint: add the missing cell");
  });
});

describe("renderDiagnosticList", () => {
  it("renders the ok state for an empty list", () => {
    const items = renderDiagnosticList([]);
    expect(items).toHaveLength(1);
    expect(items[0]?.className).toBe("ok");
  });

  it("renders one <li> per diagnostic, sorted by line", () => {
    const items = renderDiagnosticList([diag({ line: 9 }), diag({ line: 2 })]);
    expect(items.map((li) => li.querySelector(".diag-line")?.textContent)).toEqual(["line 2", "line 9"]);
  });
});

describe("renderEmptyState / renderOkState", () => {
  it("produce distinct, non-empty placeholder items", () => {
    expect(renderEmptyState().className).toBe("empty");
    expect(renderEmptyState().textContent).not.toBe("");
    expect(renderOkState().className).toBe("ok");
    expect(renderOkState().textContent).not.toBe("");
  });
});
