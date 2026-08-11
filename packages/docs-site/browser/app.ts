import { matchRecords, renderSearchResult, type SearchRecord } from "./search-view.js";

const searchInputEl = document.querySelector<HTMLInputElement>("#search-input")!;
const searchResultsEl = document.querySelector<HTMLUListElement>("#search-results")!;
const navToggleEl = document.querySelector<HTMLButtonElement>("#nav-toggle")!;
const sidebarEl = document.querySelector<HTMLElement>("#sidebar")!;

const searchIndexUrl = document.querySelector('meta[name="search-index"]')?.getAttribute("content") ?? "";
const docsPrefix = document.querySelector('meta[name="docs-prefix"]')?.getAttribute("content") ?? "../";

const SEARCH_DEBOUNCE_MS = 120;
let debounceHandle: number | undefined;
let records: SearchRecord[] = [];

async function loadIndex(): Promise<void> {
  if (!searchIndexUrl) return;
  const response = await fetch(searchIndexUrl);
  if (!response.ok) return;
  records = (await response.json()) as SearchRecord[];
}

function runSearch(): void {
  const query = searchInputEl.value;
  const matches = matchRecords(records, query);
  if (matches.length === 0) {
    searchResultsEl.hidden = true;
    searchResultsEl.replaceChildren();
    return;
  }
  searchResultsEl.replaceChildren(...matches.map((record) => renderSearchResult(record, query, docsPrefix)));
  searchResultsEl.hidden = false;
}

function scheduleSearch(): void {
  window.clearTimeout(debounceHandle);
  debounceHandle = window.setTimeout(runSearch, SEARCH_DEBOUNCE_MS);
}

function toggleNav(): void {
  const open = sidebarEl.classList.toggle("open");
  navToggleEl.setAttribute("aria-expanded", String(open));
}

searchInputEl.addEventListener("input", scheduleSearch);
document.addEventListener("click", (e) => {
  if (!(e.target instanceof Node)) return;
  if (searchInputEl.contains(e.target) || searchResultsEl.contains(e.target)) return;
  searchResultsEl.hidden = true;
});
navToggleEl.addEventListener("click", toggleNav);

void loadIndex();
