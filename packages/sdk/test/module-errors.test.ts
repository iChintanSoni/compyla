import { describe, expect, it } from "vitest";
import { isModuleNotFoundError } from "../src/module-errors.js";

describe("isModuleNotFoundError", () => {
  it("is true for an object with code ERR_MODULE_NOT_FOUND", () => {
    expect(isModuleNotFoundError({ code: "ERR_MODULE_NOT_FOUND" })).toBe(true);
  });

  it("is true for the actual error node throws for a missing dynamic import", async () => {
    try {
      await import("@compyla/this-package-does-not-exist" as string);
      expect.unreachable("expected the import to throw");
    } catch (err) {
      expect(isModuleNotFoundError(err)).toBe(true);
    }
  });

  it("is false for a different error code", () => {
    expect(isModuleNotFoundError({ code: "ERR_SOMETHING_ELSE" })).toBe(false);
  });

  it("is false for a plain Error with no code", () => {
    expect(isModuleNotFoundError(new Error("boom"))).toBe(false);
  });

  it("is false for null, undefined, and non-object values", () => {
    expect(isModuleNotFoundError(null)).toBe(false);
    expect(isModuleNotFoundError(undefined)).toBe(false);
    expect(isModuleNotFoundError("a string")).toBe(false);
    expect(isModuleNotFoundError(42)).toBe(false);
  });
});
