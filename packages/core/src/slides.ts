import type { BodyNode, DirectiveNode, InlineRun, Position } from "./types.js";

export interface Slide {
  readonly index: number;
  readonly title?: readonly InlineRun[];
  readonly titlePosition?: Position;
  /** Slide content with the title heading already extracted. Nested headings (e.g. a second
   *  top-level heading within the same slide) remain as Section nodes — profiles/renderers
   *  decide how to treat them (pptx: H4+ warns, everything renders as body content). */
  readonly body: readonly BodyNode[];
  readonly position: Position;
}

interface SlideDraft {
  title?: readonly InlineRun[];
  titlePosition?: Position;
  body: BodyNode[];
  position?: Position;
}

/**
 * Groups a document's body into pptx slides: `---` (a Break node) starts a new slide, and the
 * first heading encountered in a slide (at any nesting depth reached while still looking for a
 * title) becomes that slide's title — matching markdown-spec.md §7.1-7.2.
 *
 * Base tokens nest headings by level regardless of format (see build-tokens.ts): a `---` inside
 * an open Section, or a heading that structurally nests under an earlier one, is exactly what
 * pptx needs to reinterpret — this walk does that reinterpretation without touching the shared
 * token model.
 */
export function groupSlides(body: readonly BodyNode[]): Slide[] {
  const drafts: SlideDraft[] = [{ body: [] }];
  const current = (): SlideDraft => drafts[drafts.length - 1]!;

  function visit(node: BodyNode): void {
    if (node.type === "break") {
      drafts.push({ body: [] });
      return;
    }

    const slide = current();
    if (slide.position === undefined) slide.position = node.position;

    if (node.type === "section" && slide.title === undefined) {
      slide.title = node.title;
      slide.titlePosition = node.position;
      for (const child of node.children) visit(child);
      return;
    }

    slide.body.push(node);
  }

  for (const node of body) visit(node);

  return drafts.map((draft, index) => ({
    index,
    title: draft.title,
    titlePosition: draft.titlePosition,
    body: draft.body,
    position: draft.position ?? draft.titlePosition ?? { line: 1, column: 1 },
  }));
}

export interface ResolvedSlideContent {
  /** The `::: layout <name>` directive, if the slide has one — always as the first non-notes body
   *  block (markdown-spec.md §6.3 priority 1). Its name may not resolve to a known layout; callers
   *  validate/interpret that themselves. */
  readonly layoutDirective?: DirectiveNode;
  /** The `::: notes` directive, if present — anywhere among the slide's top-level body. */
  readonly notesDirective?: DirectiveNode;
  /** The slide's real visible content: notes stripped, and a layout directive's wrapper unwrapped
   *  to its children (that's where the actual content lives, per markdown-spec.md §6.4). Shared
   *  by the pptx validator and renderer so they can never disagree about what "the content" is. */
  readonly content: readonly BodyNode[];
}

export function resolveSlideContent(slide: Slide): ResolvedSlideContent {
  const notesDirective = slide.body.find(
    (n): n is DirectiveNode => n.type === "directive" && n.name === "notes",
  );
  const withoutNotes = notesDirective ? slide.body.filter((n) => n !== notesDirective) : slide.body;

  const first = withoutNotes[0];
  const layoutDirective = first?.type === "directive" && first.name === "layout" ? first : undefined;
  const content = layoutDirective ? layoutDirective.children : withoutNotes;

  return { layoutDirective, notesDirective, content };
}
