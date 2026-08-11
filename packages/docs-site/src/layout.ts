import type { DocPage } from "./nav.js";

export interface LayoutOptions {
  /** Page's real H1 text, rendered verbatim in <title> and the page header. */
  title: string;
  /** First paragraph, truncated, for <meta name="description">. */
  description: string;
  bodyHtml: string;
  /** null for the unversioned home page. */
  currentSlug: string | null;
  /** Relative path from this page to dist/static/assets/, e.g. "../../assets/" (doc pages) or "./assets/" (home). */
  assetPrefix: string;
  /** Relative path from this page to the site root, e.g. "../../" (doc pages) or "./" (home). */
  rootPrefix: string;
  /** Relative path from this page to dist/static/latest/, e.g. "../" (doc pages) or "./latest/" (home). */
  docsPrefix: string;
}

function renderSidebar(pages: readonly DocPage[], currentSlug: string | null, docsPrefix: string): string {
  const items = pages
    .map((page) => {
      const active = page.slug === currentSlug;
      const attrs = active ? ` class="active" aria-current="page"` : "";
      return `<li><a href="${docsPrefix}${page.slug}/"${attrs}>${page.navLabel}</a></li>`;
    })
    .join("\n");
  return `<nav id="sidebar" class="sidebar" aria-label="Documentation">\n<ul>\n${items}\n</ul>\n</nav>`;
}

export function renderLayout(opts: LayoutOptions, pages: readonly DocPage[]): string {
  const { title, description, bodyHtml, currentSlug, assetPrefix, rootPrefix, docsPrefix } = opts;
  return `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>${title} — Compyla Docs</title>
<meta name="description" content="${description}">
<meta name="search-index" content="${assetPrefix}search-index.json">
<meta name="docs-prefix" content="${docsPrefix}">
<link rel="stylesheet" href="${assetPrefix}style.css">
</head>
<body>
<a class="skip-link" href="#main">Skip to content</a>
<header class="site-header">
<a class="brand" href="${rootPrefix}">Compyla Docs</a>
<div class="search" role="search">
<input id="search-input" type="search" placeholder="Search docs…" aria-label="Search docs" autocomplete="off">
<ul id="search-results" hidden></ul>
</div>
<button id="nav-toggle" class="nav-toggle" aria-expanded="false" aria-controls="sidebar">Menu</button>
</header>
<div class="layout">
${renderSidebar(pages, currentSlug, docsPrefix)}
<main id="main" class="content"><article>${bodyHtml}</article></main>
</div>
<footer class="site-footer"><p>Compyla is open-core — <a href="https://github.com/iChintanSoni/compyla">source on GitHub</a>.</p></footer>
<script type="module" src="${assetPrefix}app.js"></script>
</body>
</html>
`;
}
