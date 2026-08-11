export interface DocPage {
  readonly slug: string;
  readonly sourceFile: string;
  /** Hand-curated short sidebar label — independent of the page's real H1, which is what
   *  the on-page <title>/<h1> render verbatim. markdown-spec.md's real H1 ("Compyla Markdown
   *  Specification") is too long for a narrow sidebar, hence the shorter label here. */
  readonly navLabel: string;
}

/** Single source of truth for site navigation, ordered per README.md's existing "## Documentation"
 *  table — reusing an existing reading-order decision rather than inventing a second one. */
export const PAGES: readonly DocPage[] = [
  { slug: "quickstart", sourceFile: "quickstart.md", navLabel: "Quickstart" },
  { slug: "architecture", sourceFile: "architecture.md", navLabel: "Architecture" },
  { slug: "markdown-spec", sourceFile: "markdown-spec.md", navLabel: "Markdown Spec" },
  { slug: "cli", sourceFile: "cli.md", navLabel: "CLI & SDK Reference" },
  { slug: "when-to-use-pandoc", sourceFile: "when-to-use-pandoc.md", navLabel: "When to Use Pandoc" },
  { slug: "roadmap", sourceFile: "roadmap.md", navLabel: "Roadmap" },
];
