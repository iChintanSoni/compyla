import { walkBody } from "@compyla/core";
import type { BodyNode, InlineRun, SectionNode } from "@compyla/core";

export interface RenderContext {
  /** Rewrites an internal docs/*.md link to its generated-site path; returns the href unchanged
   *  for anything else (external URLs, #anchors). */
  resolveLink(href: string): string;
  /** Precomputed via computeHeadingIds() over the same body — shared with the search index so
   *  rendered anchor ids and search-result anchors always agree by construction. */
  headingIds: ReadonlyMap<SectionNode, string>;
}

/** Assigns a slugified, per-page-unique id to every heading in DFS order (matching renderBody's
 *  own traversal, since both walk sections/directives/quotes/list-items via the same shape as
 *  walkBody). Computed once per page and shared between the HTML renderer and the search index. */
export function computeHeadingIds(nodes: readonly BodyNode[]): Map<SectionNode, string> {
  const ids = new Map<SectionNode, string>();
  const seen = new Map<string, number>();
  walkBody(nodes, 0, {
    onSection: (node) => {
      const base = slugify(plainText(node.title));
      const count = seen.get(base) ?? 0;
      seen.set(base, count + 1);
      ids.set(node, count === 0 ? base : `${base}-${count + 1}`);
    },
  });
  return ids;
}

export function slugify(text: string): string {
  return text
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
}

export function escapeHtml(text: string): string {
  return text
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

export function plainText(runs: readonly InlineRun[]): string {
  return runs.map((run) => run.text).join("");
}

export function renderInline(runs: readonly InlineRun[], ctx: RenderContext): string {
  return runs.map((run) => renderRun(run, ctx)).join("");
}

function renderRun(run: InlineRun, ctx: RenderContext): string {
  let html = escapeHtml(run.text);
  if (run.code) html = `<code>${html}</code>`;
  if (run.bold) html = `<strong>${html}</strong>`;
  if (run.italic) html = `<em>${html}</em>`;
  if (run.strike) html = `<del>${html}</del>`;
  if (run.link) html = `<a href="${escapeHtml(ctx.resolveLink(run.link))}">${html}</a>`;
  return html;
}

export function renderBody(nodes: readonly BodyNode[], ctx: RenderContext): string {
  return nodes.map((node) => renderNode(node, ctx)).join("\n");
}

function renderNode(node: BodyNode, ctx: RenderContext): string {
  switch (node.type) {
    case "section": {
      const id = ctx.headingIds.get(node)!;
      const heading = `<h${node.level} id="${id}">${renderInline(node.title, ctx)}` +
        `<a class="anchor" href="#${id}" aria-label="Link to this section">#</a></h${node.level}>`;
      return `<section aria-labelledby="${id}">\n${heading}\n${renderBody(node.children, ctx)}\n</section>`;
    }
    case "paragraph":
      return `<p>${renderInline(node.runs, ctx)}</p>`;
    case "table": {
      const alignStyle = (align: (typeof node.align)[number]) =>
        align ? ` style="text-align:${align}"` : "";
      const headerCells = node.header
        .map((cell, i) => `<th${alignStyle(node.align[i] ?? null)}>${renderInline(cell.runs, ctx)}</th>`)
        .join("");
      const bodyRows = node.rows
        .map(
          (row) =>
            `<tr>${row
              .map((cell, i) => `<td${alignStyle(node.align[i] ?? null)}>${renderInline(cell.runs, ctx)}</td>`)
              .join("")}</tr>`,
        )
        .join("\n");
      return `<div class="table-wrap"><table>\n<thead><tr>${headerCells}</tr></thead>\n<tbody>\n${bodyRows}\n</tbody>\n</table></div>`;
    }
    case "list": {
      const tag = node.ordered ? "ol" : "ul";
      const items = node.items
        .map((item) => {
          const checkbox =
            item.checked === null
              ? ""
              : `<input type="checkbox" disabled${item.checked ? " checked" : ""}> `;
          const cls = item.checked === null ? "" : ` class="task-item"`;
          return `<li${cls}>${checkbox}${renderBody(item.children, ctx)}</li>`;
        })
        .join("\n");
      return `<${tag}>\n${items}\n</${tag}>`;
    }
    case "media":
      return `<img src="${escapeHtml(ctx.resolveLink(node.src))}" alt="${escapeHtml(node.alt)}"${
        node.widthPercent ? ` style="width:${node.widthPercent}%"` : ""
      }>`;
    case "code":
      return `<pre><code class="language-${escapeHtml(node.lang ?? "text")}">${escapeHtml(node.value)}</code></pre>`;
    case "quote":
      return `<blockquote>\n${renderBody(node.children, ctx)}\n</blockquote>`;
    case "break":
      return "<hr>";
    case "directive":
      // No live directive fences exist in docs/*.md today (see D2: directives refine, never
      // gatekeep) — degrade to plain content rather than throwing if one is ever introduced.
      return renderBody(node.children, ctx);
    default:
      return "";
  }
}
