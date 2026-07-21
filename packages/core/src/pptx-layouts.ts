/**
 * Compyla's house layout set for pptx (markdown-spec.md §6.3). Shared by the pptx validation
 * profile (this package) and the pptx renderer (@compyla/pptx) so they can never recognize a
 * different set of layout names from one another.
 */
export const KNOWN_PPTX_LAYOUTS = [
  "Title Slide",
  "Section Header",
  "Title and Content",
  "Two Content",
  "Picture with Caption",
  "Quote",
] as const;

export type PptxLayoutName = (typeof KNOWN_PPTX_LAYOUTS)[number];

const LOOKUP = new Map<string, PptxLayoutName>(KNOWN_PPTX_LAYOUTS.map((name) => [name.toLowerCase(), name]));

/** Case-insensitive lookup against the known set, per markdown-spec.md §6.3's binding rule. */
export function resolvePptxLayoutName(name: string): PptxLayoutName | undefined {
  return LOOKUP.get(name.trim().toLowerCase());
}
