import { describe, expect, it } from "vitest";
import { buildDocument } from "../src/build-tokens.js";
import { parseMarkdown } from "../src/parser.js";
import { groupSlides, resolveSlideContent } from "../src/slides.js";
import type { DirectiveNode, SectionNode } from "../src/types.js";

function parse(source: string) {
  return buildDocument(parseMarkdown(source));
}

describe("groupSlides", () => {
  it("treats a document with no breaks as a single slide", () => {
    const doc = parse(`# Title\n\nBody text.\n`);
    const slides = groupSlides(doc.body);
    expect(slides).toHaveLength(1);
    expect(slides[0]?.title?.[0]?.text).toBe("Title");
  });

  it("splits into multiple slides on thematic breaks", () => {
    const doc = parse(`# One\n\n---\n\n# Two\n\n---\n\n# Three\n`);
    const slides = groupSlides(doc.body);
    expect(slides).toHaveLength(3);
    expect(slides.map((s) => s.title?.[0]?.text)).toEqual(["One", "Two", "Three"]);
  });

  it("uses the first heading at any depth as the slide title, and nests its content as body", () => {
    const doc = parse(`# Deck Title\n\nSubtitle paragraph.\n`);
    const slides = groupSlides(doc.body);
    expect(slides[0]?.title?.[0]?.text).toBe("Deck Title");
    expect(slides[0]?.body).toHaveLength(1);
    expect(slides[0]?.body[0]?.type).toBe("paragraph");
  });

  it("demotes a second top-level heading in the same slide to body content instead of a new title", () => {
    const doc = parse(`## Revenue\n\nGrew 14%.\n\n## Extra Heading\n\nMore text.\n`);
    const slides = groupSlides(doc.body);
    expect(slides).toHaveLength(1);
    expect(slides[0]?.title?.[0]?.text).toBe("Revenue");
    const extra = slides[0]?.body.find((n) => n.type === "section") as SectionNode | undefined;
    expect(extra?.title[0]?.text).toBe("Extra Heading");
  });

  it("handles a break that lands inside an open section (docx-style nesting) as a real slide split", () => {
    // Base tokens nest a Break under the currently-open H1 section (format-agnostic); pptx must
    // still treat it as ending the slide.
    const doc = parse(`# One\n\nfirst slide body\n\n---\n\n## Two\n\nsecond slide body\n`);
    const slides = groupSlides(doc.body);
    expect(slides).toHaveLength(2);
    expect(slides[0]?.title?.[0]?.text).toBe("One");
    expect(slides[1]?.title?.[0]?.text).toBe("Two");
  });

  it("keeps a slide's directive content intact (e.g. a layout directive as the first body block)", () => {
    const doc = parse(`## Revenue\n\n::: layout two-col\nLeft.\n<!-- col -->\nRight.\n:::\n`);
    const slides = groupSlides(doc.body);
    const directive = slides[0]?.body[0] as DirectiveNode;
    expect(directive.type).toBe("directive");
    expect(directive.name).toBe("layout");
  });

  it("gives every slide a usable position even without a title", () => {
    const doc = parse(`Just a paragraph, no heading.\n`);
    const slides = groupSlides(doc.body);
    expect(slides[0]?.position.line).toBe(1);
  });
});

describe("resolveSlideContent", () => {
  it("unwraps an explicit layout directive's children as the real content", () => {
    const doc = parse(`## Title\n\n::: layout two-col\nLeft.\n<!-- col -->\nRight.\n:::\n`);
    const [slide] = groupSlides(doc.body);
    const resolved = resolveSlideContent(slide!);
    expect(resolved.layoutDirective?.argsRaw).toBe("two-col");
    expect(resolved.content).toHaveLength(3); // paragraph, col marker, paragraph
  });

  it("finds a notes directive as a sibling block and excludes it from content", () => {
    const doc = parse(`## Title\n\nBody paragraph.\n\n::: notes\nSpeaker notes.\n:::\n`);
    const [slide] = groupSlides(doc.body);
    const resolved = resolveSlideContent(slide!);
    expect(resolved.notesDirective?.name).toBe("notes");
    expect(resolved.content.some((n) => n.type === "directive")).toBe(false);
  });

  it("finds notes alongside an explicit layout directive without treating it as slide content", () => {
    const doc = parse(`## Title\n\n::: layout Title and Content\nBody.\n:::\n\n::: notes\nSpeak up.\n:::\n`);
    const [slide] = groupSlides(doc.body);
    const resolved = resolveSlideContent(slide!);
    expect(resolved.layoutDirective?.argsRaw).toBe("Title and Content");
    expect(resolved.notesDirective?.name).toBe("notes");
    expect(resolved.content).toHaveLength(1); // just the body paragraph
  });

  it("has no layout directive and returns the raw body as content when none is given", () => {
    const doc = parse(`## Title\n\nJust a paragraph.\n`);
    const [slide] = groupSlides(doc.body);
    const resolved = resolveSlideContent(slide!);
    expect(resolved.layoutDirective).toBeUndefined();
    expect(resolved.content).toHaveLength(1);
  });
});
