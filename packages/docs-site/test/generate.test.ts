import { describe, expect, it } from "vitest";
import { generateSite } from "../src/generate.js";
import { PAGES } from "../src/nav.js";

// Each test regenerates the whole site from docs/*.md, including the ~80 KB markdown spec, which
// takes several seconds on a CI runner: well past vitest's 5 s default.
describe("generateSite", { timeout: 30_000 }, () => {
  it("produces one output file per page at latest/{slug}/index.html", async () => {
    const { files } = await generateSite();
    for (const page of PAGES) {
      expect(files.has(`latest/${page.slug}/index.html`)).toBe(true);
    }
  });

  it("resolves the real [cli.md](cli.md) link in architecture.md to the generated cli page", async () => {
    const { files } = await generateSite();
    expect(files.get("latest/architecture/index.html")).toContain('href="../cli/"');
  });

  it("renders task-list checkboxes on the roadmap page", async () => {
    const { files } = await generateSite();
    const roadmap = files.get("latest/roadmap/index.html") ?? "";
    expect(roadmap).toContain('type="checkbox"');
    expect(roadmap).toContain("checked");
  });

  it("produces a valid, non-empty search index", async () => {
    const { files } = await generateSite();
    const raw = files.get("assets/search-index.json");
    expect(raw).toBeTruthy();
    const records = JSON.parse(raw!) as unknown[];
    expect(records.length).toBeGreaterThan(0);
  });

  it("produces a home page linking to every doc page", async () => {
    const { files } = await generateSite();
    const home = files.get("index.html") ?? "";
    for (const page of PAGES) {
      expect(home).toContain(`href="./latest/${page.slug}/"`);
    }
  });

  it("includes .nojekyll at the artifact root", async () => {
    const { files } = await generateSite();
    expect(files.has(".nojekyll")).toBe(true);
  });
});
