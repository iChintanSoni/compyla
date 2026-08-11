import { describe, expect, it } from "vitest";
import { inspectTemplate, UnsupportedTemplateKindError } from "../src/template.js";
import { convert } from "../src/convert.js";

describe("inspectTemplate", () => {
  it("rejects an unsupported template kind", async () => {
    await expect(inspectTemplate(Buffer.from(""), "xlsx")).rejects.toBeInstanceOf(UnsupportedTemplateKindError);
  });

  it("reports styles read from a generated docx", async () => {
    const { buffer } = await convert("# Title\n\nSome body text.\n", { format: "docx" });
    const report = (await inspectTemplate(buffer!, "docx")) as { kind: string; styles: { id: string }[] };
    expect(report.kind).toBe("docx");
    expect(report.styles.some((s) => s.id === "Heading1")).toBe(true);
  });
});
