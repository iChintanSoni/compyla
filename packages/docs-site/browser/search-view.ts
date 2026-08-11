export interface SearchRecord {
  readonly pageSlug: string;
  readonly pageTitle: string;
  readonly headingText: string;
  readonly anchor: string;
  readonly text: string;
}

interface ScoredRecord {
  readonly record: SearchRecord;
  readonly score: number;
}

const SNIPPET_RADIUS = 50;

export function matchRecords(records: readonly SearchRecord[], query: string, limit = 8): SearchRecord[] {
  const q = query.trim().toLowerCase();
  if (q.length === 0) return [];

  const scored: ScoredRecord[] = [];
  for (const record of records) {
    let score = 0;
    if (record.headingText.toLowerCase().includes(q)) score += 2;
    if (record.pageTitle.toLowerCase().includes(q)) score += 2;
    if (record.text.toLowerCase().includes(q)) score += 1;
    if (score > 0) scored.push({ record, score });
  }

  scored.sort((a, b) => b.score - a.score);
  return scored.slice(0, limit).map((s) => s.record);
}

function buildSnippet(text: string, query: string): string {
  const lower = text.toLowerCase();
  const idx = lower.indexOf(query.toLowerCase());
  if (idx === -1) return text.slice(0, SNIPPET_RADIUS * 2);
  const start = Math.max(0, idx - SNIPPET_RADIUS);
  const end = Math.min(text.length, idx + query.length + SNIPPET_RADIUS);
  return `${start > 0 ? "…" : ""}${text.slice(start, end)}${end < text.length ? "…" : ""}`;
}

/** Appends `text` to `target`, wrapping every case-insensitive occurrence of `query` in <mark>.
 *  Built via DOM nodes rather than innerHTML since the matched substring's casing is
 *  attacker-uncontrolled but there's no reason to introduce an innerHTML habit here either. */
function appendHighlighted(target: HTMLElement, text: string, query: string): void {
  const q = query.trim().toLowerCase();
  if (q.length === 0) {
    target.append(text);
    return;
  }

  const lower = text.toLowerCase();
  let cursor = 0;
  let idx = lower.indexOf(q, cursor);
  while (idx !== -1) {
    if (idx > cursor) target.append(text.slice(cursor, idx));
    const mark = document.createElement("mark");
    mark.textContent = text.slice(idx, idx + q.length);
    target.append(mark);
    cursor = idx + q.length;
    idx = lower.indexOf(q, cursor);
  }
  if (cursor < text.length) target.append(text.slice(cursor));
}

export function renderSearchResult(record: SearchRecord, query: string, docsPrefix: string): HTMLLIElement {
  const li = document.createElement("li");
  const a = document.createElement("a");
  a.href = `${docsPrefix}${record.pageSlug}/${record.anchor}`;

  const title = document.createElement("span");
  title.className = "result-title";
  const titleText =
    record.pageTitle === record.headingText ? record.headingText : `${record.pageTitle} › ${record.headingText}`;
  appendHighlighted(title, titleText, query);

  const snippet = document.createElement("span");
  snippet.className = "snippet";
  appendHighlighted(snippet, buildSnippet(record.text, query), query);

  a.append(title, snippet);
  li.append(a);
  return li;
}
