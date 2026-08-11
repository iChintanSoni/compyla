import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { parse } from "@compyla/core";
import type { BodyNode, SectionNode } from "@compyla/core";
import { PAGES } from "./nav.js";
import { resolveInternalLink } from "./links.js";
import { computeHeadingIds, plainText, renderBody, type RenderContext } from "./render-html.js";
import { renderLayout } from "./layout.js";
import { buildSearchIndex, type SearchRecord } from "./search-index.js";

export interface GeneratedSite {
  /** Path relative to dist/static/ -> file content. */
  readonly files: ReadonlyMap<string, string>;
}

const DESCRIPTION_MAX_LENGTH = 160;

function findFirstSectionTitle(nodes: readonly BodyNode[]): string | undefined {
  const first = nodes.find((node): node is SectionNode => node.type === "section");
  return first ? plainText(first.title) : undefined;
}

function findFirstParagraphText(nodes: readonly BodyNode[]): string | undefined {
  for (const node of nodes) {
    if (node.type === "paragraph") return plainText(node.runs);
    if (node.type === "section" || node.type === "quote" || node.type === "directive") {
      const found = findFirstParagraphText(node.children);
      if (found) return found;
    }
    if (node.type === "list") {
      for (const item of node.items) {
        const found = findFirstParagraphText(item.children);
        if (found) return found;
      }
    }
  }
  return undefined;
}

function truncate(text: string, maxLength: number): string {
  if (text.length <= maxLength) return text;
  return `${text.slice(0, maxLength - 1).trimEnd()}…`;
}

export async function generateSite(): Promise<GeneratedSite> {
  const docsDir = fileURLToPath(new URL("../../../docs", import.meta.url));
  const browserDir = fileURLToPath(new URL("../browser", import.meta.url));
  const files = new Map<string, string>();

  const pageSummaries: { slug: string; navLabel: string; title: string; description: string }[] = [];
  const searchInput: { slug: string; title: string; body: readonly BodyNode[]; headingIds: Map<SectionNode, string> }[] = [];

  for (const page of PAGES) {
    const source = await readFile(`${docsDir}/${page.sourceFile}`, "utf8");
    const document = parse(source);
    const title = findFirstSectionTitle(document.body) ?? page.navLabel;
    const description = truncate(findFirstParagraphText(document.body) ?? "", DESCRIPTION_MAX_LENGTH);
    const headingIds = computeHeadingIds(document.body);

    const ctx: RenderContext = {
      resolveLink: (href) => resolveInternalLink(href, PAGES) ?? href,
      headingIds,
    };
    const bodyHtml = renderBody(document.body, ctx);
    const html = renderLayout(
      {
        title,
        description,
        bodyHtml,
        currentSlug: page.slug,
        assetPrefix: "../../assets/",
        rootPrefix: "../../",
        docsPrefix: "../",
      },
      PAGES,
    );
    files.set(`latest/${page.slug}/index.html`, html);

    pageSummaries.push({ slug: page.slug, navLabel: page.navLabel, title, description });
    searchInput.push({ slug: page.slug, title, body: document.body, headingIds });
  }

  const searchRecords: SearchRecord[] = buildSearchIndex(searchInput);
  files.set("assets/search-index.json", JSON.stringify(searchRecords));

  const homeFragment = await readFile(`${browserDir}/home.html`, "utf8");
  const homeLinks = pageSummaries
    .map(
      (page) =>
        `<li><a href="./latest/${page.slug}/"><strong>${page.navLabel}</strong></a><p>${page.description}</p></li>`,
    )
    .join("\n");
  const homeBodyHtml = homeFragment.replace("<!--PAGES-->", `<ul class="doc-index">\n${homeLinks}\n</ul>`);
  const homeHtml = renderLayout(
    {
      title: "Compyla Docs",
      description: "Compyla converts markdown into docx, pdf, pptx, and xlsx. Documentation, reference, and roadmap.",
      bodyHtml: homeBodyHtml,
      currentSlug: null,
      assetPrefix: "./assets/",
      rootPrefix: "./",
      docsPrefix: "./latest/",
    },
    PAGES,
  );
  files.set("index.html", homeHtml);
  files.set(".nojekyll", "");

  return { files };
}
