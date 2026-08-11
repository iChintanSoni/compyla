import {
  RendererNotInstalledError,
  TemplateIntrospectorNotInstalledError,
  UnsupportedFormatError,
  UnsupportedTemplateKindError,
} from "@compyla/sdk";
import { describe, expect, it } from "vitest";
import { mapError } from "../src/errors.js";
import { MalformedRequestError } from "../src/multipart.js";

describe("mapError", () => {
  it("maps UnsupportedFormatError to 400 (client's mistake — bad input value)", () => {
    const { status, body } = mapError(new UnsupportedFormatError("csv"));
    expect(status).toBe(400);
    expect(body.error).toContain("csv");
  });

  it("maps UnsupportedTemplateKindError to 400", () => {
    const { status, body } = mapError(new UnsupportedTemplateKindError("xlsx"));
    expect(status).toBe(400);
    expect(body.error).toContain("xlsx");
  });

  it("maps MalformedRequestError to 400", () => {
    const { status, body } = mapError(new MalformedRequestError("Missing required field \"format\"."));
    expect(status).toBe(400);
    expect(body.error).toContain("format");
  });

  // These two never fire in the black-box HTTP tests, since the dev/CI environment always has every
  // renderer package installed as a devDependency — mapError is the only place this branch is
  // reachable without actually uninstalling a package.
  it("maps RendererNotInstalledError to 501 (this deployment's capability gap, not the client's)", () => {
    const { status, body } = mapError(new RendererNotInstalledError("pptx", "@compyla/pptx"));
    expect(status).toBe(501);
    expect(body.error).toContain("@compyla/pptx");
  });

  it("maps TemplateIntrospectorNotInstalledError to 501", () => {
    const { status, body } = mapError(new TemplateIntrospectorNotInstalledError("docx", "@compyla/docx"));
    expect(status).toBe(501);
    expect(body.error).toContain("@compyla/docx");
  });

  it("maps core's plain-Error 'Unknown format' message to 400 by matching text, not type", () => {
    const { status, body } = mapError(new Error('Unknown format "csv". Supported: docx, pdf, pptx, xlsx.'));
    expect(status).toBe(400);
    expect(body.error).toContain("csv");
  });

  it("falls back to 500 for an unrelated Error", () => {
    const { status, body } = mapError(new Error("boom"));
    expect(status).toBe(500);
    expect(body.error).toBe("boom");
  });

  it("falls back to 500 for a thrown non-Error value", () => {
    const { status, body } = mapError("not an Error instance");
    expect(status).toBe(500);
    expect(body.error).toBe("not an Error instance");
  });
});
