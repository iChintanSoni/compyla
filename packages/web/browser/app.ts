import { lint } from "@compyla/sdk";
import { formatStatus, renderDiagnosticList, renderEmptyState } from "./diagnostics-view.js";

const markdownEl = document.querySelector<HTMLTextAreaElement>("#markdown")!;
const formatEl = document.querySelector<HTMLSelectElement>("#format")!;
const strictEl = document.querySelector<HTMLInputElement>("#strict")!;
const diagnosticsEl = document.querySelector<HTMLUListElement>("#diagnostics")!;
const statusEl = document.querySelector<HTMLSpanElement>("#status")!;

const LINT_DEBOUNCE_MS = 150;
let debounceHandle: number | undefined;

function scheduleLint(): void {
  window.clearTimeout(debounceHandle);
  debounceHandle = window.setTimeout(runLint, LINT_DEBOUNCE_MS);
}

function runLint(): void {
  const source = markdownEl.value;
  if (source.trim().length === 0) {
    diagnosticsEl.replaceChildren(renderEmptyState());
    statusEl.textContent = "";
    return;
  }

  const { diagnostics, ok } = lint(source, { format: formatEl.value, strict: strictEl.checked });
  diagnosticsEl.replaceChildren(...renderDiagnosticList(diagnostics));
  statusEl.textContent = formatStatus(diagnostics, ok);
}

async function loadDroppedFile(file: File): Promise<void> {
  markdownEl.value = await file.text();
  runLint();
}

/** Tracks nested dragenter/dragleave depth — children of the drop target fire their own
 *  enter/leave pairs, so a naive toggle on every event flickers the overlay off mid-drag. */
function setupDragAndDrop(): void {
  let dragDepth = 0;

  window.addEventListener("dragenter", (e) => {
    e.preventDefault();
    dragDepth += 1;
    document.body.classList.add("dragging");
  });

  window.addEventListener("dragover", (e) => {
    e.preventDefault();
  });

  window.addEventListener("dragleave", (e) => {
    e.preventDefault();
    dragDepth = Math.max(0, dragDepth - 1);
    if (dragDepth === 0) document.body.classList.remove("dragging");
  });

  window.addEventListener("drop", (e) => {
    e.preventDefault();
    dragDepth = 0;
    document.body.classList.remove("dragging");
    const file = e.dataTransfer?.files[0];
    if (file) void loadDroppedFile(file);
  });
}

markdownEl.addEventListener("input", scheduleLint);
formatEl.addEventListener("change", runLint);
strictEl.addEventListener("change", runLint);
setupDragAndDrop();
diagnosticsEl.replaceChildren(renderEmptyState());
