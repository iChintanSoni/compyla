import type { DocPage } from "./nav.js";

const INTERNAL_MD_LINK = /^(?:docs\/)?([a-z0-9-]+\.md)$/i;

/** Resolves a markdown href to a generated-site path if it points at one of our own docs/*.md
 *  files (bare "foo.md" as used within docs/*.md, or "docs/foo.md" as used from README.md).
 *  Returns null for anything else (external URLs, #anchors, unknown filenames) so callers can
 *  pass the href through unchanged rather than throwing on an unrecognized link. */
export function resolveInternalLink(href: string, pages: readonly DocPage[]): string | null {
  const match = INTERNAL_MD_LINK.exec(href);
  if (!match) return null;
  const filename = match[1]!.toLowerCase();
  const target = pages.find((page) => page.sourceFile.toLowerCase() === filename);
  return target ? `../${target.slug}/` : null;
}
