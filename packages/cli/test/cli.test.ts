import { execFile } from "node:child_process";
import { existsSync, rmSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { afterEach, describe, expect, it } from "vitest";

const CLI_ENTRY = fileURLToPath(new URL("../dist/index.js", import.meta.url));
const GOOD_MD = fileURLToPath(new URL("./fixtures/good.md", import.meta.url));
const BROKEN_MD = fileURLToPath(new URL("./fixtures/broken.md", import.meta.url));
const DECK_MD = fileURLToPath(new URL("./fixtures/deck.md", import.meta.url));

interface CliResult {
  readonly code: number;
  readonly stdout: string;
  readonly stderr: string;
}

function runCli(args: string[]): Promise<CliResult> {
  return new Promise((resolve) => {
    execFile("node", [CLI_ENTRY, ...args], (error, stdout, stderr) => {
      const code = error && "code" in error && typeof error.code === "number" ? error.code : 0;
      resolve({ code, stdout, stderr });
    });
  });
}

const outputsToClean: string[] = [];
afterEach(() => {
  for (const path of outputsToClean.splice(0)) {
    if (existsSync(path)) rmSync(path);
  }
});

describe("compyla lint", () => {
  it("exits 0 and reports OK for a clean document", async () => {
    const result = await runCli(["lint", GOOD_MD, "--format", "docx"]);
    expect(result.code).toBe(0);
    expect(result.stdout).toContain("OK (docx)");
  });

  it("exits 1 and prints line-referenced diagnostics for a broken document", async () => {
    const result = await runCli(["lint", BROKEN_MD, "--format", "docx"]);
    expect(result.code).toBe(1);
    expect(result.stderr).toContain("heading_skip");
    expect(result.stderr).toContain("image_unresolved");
    expect(result.stderr).toContain("unknown_directive");
    expect(result.stderr).toMatch(/broken\.md:\d+:/);
  });

  it("prints a clean error (not a stack trace) for an unsupported format", async () => {
    const result = await runCli(["lint", GOOD_MD, "--format", "epub"]);
    expect(result.code).toBe(1);
    expect(result.stderr).toContain('Unknown format "epub"');
    expect(result.stderr).not.toContain(" at "); // no stack trace frames
  });
});

describe("compyla convert", () => {
  it("writes a docx file and exits 0 for a clean document", async () => {
    const outPath = fileURLToPath(new URL("./fixtures/good.docx", import.meta.url));
    outputsToClean.push(outPath);

    const result = await runCli(["convert", GOOD_MD, "--format", "docx", "--out", outPath]);
    expect(result.code).toBe(0);
    expect(result.stdout).toContain(outPath);
    expect(existsSync(outPath)).toBe(true);
  });

  it("produces byte-identical output across separate process invocations", async () => {
    const outA = fileURLToPath(new URL("./fixtures/good-a.docx", import.meta.url));
    const outB = fileURLToPath(new URL("./fixtures/good-b.docx", import.meta.url));
    outputsToClean.push(outA, outB);

    await runCli(["convert", GOOD_MD, "--format", "docx", "--out", outA]);
    await runCli(["convert", GOOD_MD, "--format", "docx", "--out", outB]);

    const { readFileSync } = await import("node:fs");
    expect(Buffer.compare(readFileSync(outA), readFileSync(outB))).toBe(0);
  });

  it("blocks on validation errors and does not write output", async () => {
    const outPath = fileURLToPath(new URL("./fixtures/broken.docx", import.meta.url));
    outputsToClean.push(outPath);

    const result = await runCli(["convert", BROKEN_MD, "--format", "docx", "--out", outPath]);
    expect(result.code).toBe(1);
    expect(existsSync(outPath)).toBe(false);
  });
});

describe("compyla template inspect", () => {
  it("reports real styles read from a generated docx", async () => {
    const templatePath = fileURLToPath(new URL("./fixtures/good.docx", import.meta.url));
    outputsToClean.push(templatePath);
    await runCli(["convert", GOOD_MD, "--format", "docx", "--out", templatePath]);

    const result = await runCli(["template", "inspect", templatePath]);
    expect(result.code).toBe(0);
    const report = JSON.parse(result.stdout);
    expect(report.kind).toBe("docx");
    expect(report.styles.some((s: { id: string }) => s.id === "Heading1")).toBe(true);
  });

  it("dispatches to the pptx introspector for a .pptx file", async () => {
    const templatePath = fileURLToPath(new URL("./fixtures/deck.pptx", import.meta.url));
    outputsToClean.push(templatePath);
    await runCli(["convert", DECK_MD, "--format", "pptx", "--out", templatePath]);

    const result = await runCli(["template", "inspect", templatePath]);
    expect(result.code).toBe(0);
    const report = JSON.parse(result.stdout);
    expect(report.kind).toBe("pptx");
  });
});

describe("compyla convert — pptx", () => {
  it("writes a pptx file and exits 0 for a clean deck", async () => {
    const outPath = fileURLToPath(new URL("./fixtures/deck.pptx", import.meta.url));
    outputsToClean.push(outPath);

    const result = await runCli(["convert", DECK_MD, "--format", "pptx", "--out", outPath]);
    expect(result.code).toBe(0);
    expect(existsSync(outPath)).toBe(true);
  });

  it("produces byte-identical pptx output across separate process invocations", async () => {
    const outA = fileURLToPath(new URL("./fixtures/deck-a.pptx", import.meta.url));
    const outB = fileURLToPath(new URL("./fixtures/deck-b.pptx", import.meta.url));
    outputsToClean.push(outA, outB);

    await runCli(["convert", DECK_MD, "--format", "pptx", "--out", outA]);
    await runCli(["convert", DECK_MD, "--format", "pptx", "--out", outB]);

    const { readFileSync } = await import("node:fs");
    expect(Buffer.compare(readFileSync(outA), readFileSync(outB))).toBe(0);
  });
});

describe("compyla convert — pdf", () => {
  it("writes a pdf file and exits 0 for a clean document", async () => {
    const outPath = fileURLToPath(new URL("./fixtures/good.pdf", import.meta.url));
    outputsToClean.push(outPath);

    const result = await runCli(["convert", GOOD_MD, "--format", "pdf", "--out", outPath]);
    expect(result.code).toBe(0);
    expect(existsSync(outPath)).toBe(true);
  });

  it("produces byte-identical pdf output across separate process invocations", async () => {
    const outA = fileURLToPath(new URL("./fixtures/good-a.pdf", import.meta.url));
    const outB = fileURLToPath(new URL("./fixtures/good-b.pdf", import.meta.url));
    outputsToClean.push(outA, outB);

    await runCli(["convert", GOOD_MD, "--format", "pdf", "--out", outA]);
    await runCli(["convert", GOOD_MD, "--format", "pdf", "--out", outB]);

    const { readFileSync } = await import("node:fs");
    expect(Buffer.compare(readFileSync(outA), readFileSync(outB))).toBe(0);
  });
});
