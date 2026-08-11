import type { Server } from "node:http";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { createServer } from "../src/server.js";

let server: Server;
let baseUrl: string;

beforeAll(async () => {
  server = createServer();
  await new Promise<void>((resolve) => server.listen(0, resolve));
  const address = server.address();
  if (!address || typeof address === "string") throw new Error("expected a network address");
  baseUrl = `http://127.0.0.1:${address.port}`;
});

afterAll(async () => {
  await new Promise<void>((resolve, reject) => server.close((err) => (err ? reject(err) : resolve())));
});

function form(
  fields: Record<string, string>,
  files?: Record<string, { filename: string; content: Buffer }>,
): FormData {
  const body = new FormData();
  for (const [key, value] of Object.entries(fields)) body.append(key, value);
  if (files) {
    for (const [key, file] of Object.entries(files)) {
      body.append(key, new Blob([file.content]), file.filename);
    }
  }
  return body;
}

describe("POST /convert", () => {
  it("returns a valid docx buffer with the right content-type for a clean document", async () => {
    const res = await fetch(`${baseUrl}/convert`, {
      method: "POST",
      body: form({ markdown: "# Title\n\nSome **bold** text.\n", format: "docx" }),
    });
    expect(res.status).toBe(200);
    expect(res.headers.get("content-type")).toBe(
      "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    );
    expect(res.headers.get("content-disposition")).toContain("output.docx");
    const buffer = Buffer.from(await res.arrayBuffer());
    expect(buffer.subarray(0, 2).toString("latin1")).toBe("PK"); // zip magic
  });

  it("returns a valid pdf buffer for format=pdf", async () => {
    const res = await fetch(`${baseUrl}/convert`, { method: "POST", body: form({ markdown: "# Title\n", format: "pdf" }) });
    expect(res.status).toBe(200);
    expect(res.headers.get("content-type")).toBe("application/pdf");
    const buffer = Buffer.from(await res.arrayBuffer());
    expect(buffer.subarray(0, 5).toString("latin1")).toBe("%PDF-");
  });

  it("applies an uploaded template", async () => {
    const template = Buffer.from(
      await fetch(`${baseUrl}/convert`, { method: "POST", body: form({ markdown: "# Brand\n", format: "docx" }) }).then(
        (r) => r.arrayBuffer(),
      ),
    );

    const res = await fetch(`${baseUrl}/convert`, {
      method: "POST",
      body: form({ markdown: "# Title\n", format: "docx" }, { template: { filename: "brand.docx", content: template } }),
    });
    expect(res.status).toBe(200);
    const buffer = Buffer.from(await res.arrayBuffer());
    expect(buffer.subarray(0, 2).toString("latin1")).toBe("PK");
  });

  it("returns 422 with diagnostics when a document fails to validate (no asset-upload support yet)", async () => {
    const res = await fetch(`${baseUrl}/convert`, {
      method: "POST",
      body: form({ markdown: "![missing](missing.png)\n", format: "docx" }),
    });
    expect(res.status).toBe(422);
    const body = await res.json();
    expect(body.ok).toBe(false);
    expect(body.diagnostics.some((d: { code: string }) => d.code === "image_unresolved")).toBe(true);
  });

  it("returns 400 for an unknown format", async () => {
    const res = await fetch(`${baseUrl}/convert`, { method: "POST", body: form({ markdown: "# Title\n", format: "epub" }) });
    expect(res.status).toBe(400);
    const body = await res.json();
    expect(body.error).toMatch(/Unknown format "epub"/);
  });

  it("returns 400 when the markdown field is missing", async () => {
    const res = await fetch(`${baseUrl}/convert`, { method: "POST", body: form({ format: "docx" }) });
    expect(res.status).toBe(400);
  });

  it("returns 400 when the format field is missing", async () => {
    const res = await fetch(`${baseUrl}/convert`, { method: "POST", body: form({ markdown: "# Title\n" }) });
    expect(res.status).toBe(400);
  });

  it("surfaces non-blocking warnings via X-Compyla-Diagnostics without breaking the binary body", async () => {
    const template = Buffer.from(
      await fetch(`${baseUrl}/convert`, { method: "POST", body: form({ markdown: "# T\n", format: "docx" }) }).then((r) =>
        r.arrayBuffer(),
      ),
    );

    const res = await fetch(`${baseUrl}/convert`, {
      method: "POST",
      body: form({ markdown: "# Title\n", format: "docx" }, { template: { filename: "brand.docx", content: template } }),
    });
    expect(res.status).toBe(200);
    const diagHeader = res.headers.get("x-compyla-diagnostics");
    expect(diagHeader).not.toBeNull();
    expect(JSON.parse(diagHeader!).some((d: { code: string }) => d.code === "no_theme")).toBe(true);
  });
});

describe("POST /validate", () => {
  it("returns 200 with ok:true and no diagnostics for a clean document", async () => {
    const res = await fetch(`${baseUrl}/validate`, { method: "POST", body: form({ markdown: "# Title\n", format: "docx" }) });
    expect(res.status).toBe(200);
    const body = await res.json();
    expect(body).toEqual({ ok: true, diagnostics: [] });
  });

  it("returns 200 with ok:true and warning diagnostics for a non-blocking issue", async () => {
    const res = await fetch(`${baseUrl}/validate`, {
      method: "POST",
      body: form({ markdown: "# One\n\n### Three\n", format: "docx" }),
    });
    expect(res.status).toBe(200);
    const body = await res.json();
    expect(body.ok).toBe(true);
    expect(body.diagnostics.some((d: { code: string }) => d.code === "heading_skip")).toBe(true);
  });

  it("returns 422 with ok:false for a blocking error", async () => {
    const res = await fetch(`${baseUrl}/validate`, {
      method: "POST",
      body: form({ markdown: "![missing](missing.png)\n", format: "docx" }),
    });
    expect(res.status).toBe(422);
    const body = await res.json();
    expect(body.ok).toBe(false);
    expect(body.diagnostics.some((d: { code: string }) => d.code === "image_unresolved")).toBe(true);
  });

  it("returns 400 for an unknown format", async () => {
    const res = await fetch(`${baseUrl}/validate`, { method: "POST", body: form({ markdown: "# Title\n", format: "epub" }) });
    expect(res.status).toBe(400);
  });
});

describe("POST /templates", () => {
  it("returns a real introspection report for an uploaded docx", async () => {
    const template = Buffer.from(
      await fetch(`${baseUrl}/convert`, { method: "POST", body: form({ markdown: "# Title\n", format: "docx" }) }).then((r) =>
        r.arrayBuffer(),
      ),
    );

    const res = await fetch(`${baseUrl}/templates`, {
      method: "POST",
      body: form({}, { template: { filename: "brand.docx", content: template } }),
    });
    expect(res.status).toBe(200);
    const report = await res.json();
    expect(report.kind).toBe("docx");
    expect(Array.isArray(report.styles)).toBe(true);
  });

  it("returns 400 for an unsupported template extension", async () => {
    const res = await fetch(`${baseUrl}/templates`, {
      method: "POST",
      body: form({}, { template: { filename: "brand.xlsx", content: Buffer.from("not a real xlsx") } }),
    });
    expect(res.status).toBe(400);
    const body = await res.json();
    expect(body.error).toContain("unsupported template extension");
  });

  it("returns 400 when the template file field is missing", async () => {
    const res = await fetch(`${baseUrl}/templates`, { method: "POST", body: form({ note: "no file here" }) });
    expect(res.status).toBe(400);
  });
});

describe("unmatched routes", () => {
  it("returns 404 with a helpful message", async () => {
    const res = await fetch(`${baseUrl}/nope`);
    expect(res.status).toBe(404);
    const body = await res.json();
    expect(body.error).toContain("GET /nope");
  });
});
