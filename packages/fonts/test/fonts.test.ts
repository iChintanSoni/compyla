import { describe, expect, it } from "vitest";
import { resolveFont, resolveFonts } from "../src/index.js";

function fakeFetch(handlers: Record<string, () => Response>): typeof fetch {
  return (async (input: string | URL | Request) => {
    const url = typeof input === "string" ? input : input.toString();
    const key = Object.keys(handlers).find((k) => url.startsWith(k));
    if (!key) throw new Error(`Unhandled fetch in test: ${url}`);
    return handlers[key]!();
  }) as typeof fetch;
}

describe("resolveFont", () => {
  it("prefers a user-supplied font over any network lookup", async () => {
    const userFonts = new Map([["Corporate Sans", Buffer.from("fake-ttf-bytes")]]);
    const result = await resolveFont("Corporate Sans", {
      userFonts,
      fetchImpl: (() => {
        throw new Error("should not be called");
      }) as unknown as typeof fetch,
    });
    expect(result).toEqual({ family: "Corporate Sans", source: "user-supplied", data: Buffer.from("fake-ttf-bytes") });
  });

  it("resolves via Google Fonts when the CSS response contains a ttf url", async () => {
    const ttfBytes = new Uint8Array([1, 2, 3, 4]);
    const fetchImpl = fakeFetch({
      "https://fonts.googleapis.com/css2": () =>
        new Response(`@font-face { src: url(https://fonts.gstatic.com/s/inter/v20/font.ttf) format('truetype'); }`),
      "https://fonts.gstatic.com/": () => new Response(ttfBytes),
    });

    const result = await resolveFont("Inter", { fetchImpl });
    expect(result.source).toBe("google-fonts");
    expect(result.data).toEqual(Buffer.from(ttfBytes));
  });

  it("marks a font unresolved when Google Fonts has no match", async () => {
    const fetchImpl = fakeFetch({
      "https://fonts.googleapis.com/css2": () => new Response("", { status: 404 }),
    });
    const result = await resolveFont("Not A Real Font", { fetchImpl });
    expect(result.source).toBe("unresolved");
    expect(result.data).toBeUndefined();
  });

  it("marks a font unresolved when the CSS response has no ttf url", async () => {
    const fetchImpl = fakeFetch({
      "https://fonts.googleapis.com/css2": () => new Response("@font-face { src: url(https://fonts.gstatic.com/s/x/v1/font.woff2); }"),
    });
    const result = await resolveFont("Weird Font", { fetchImpl });
    expect(result.source).toBe("unresolved");
  });

  it("marks a font unresolved rather than throwing when fetch itself fails", async () => {
    const fetchImpl = (async () => {
      throw new Error("network down");
    }) as unknown as typeof fetch;
    const result = await resolveFont("Inter", { fetchImpl });
    expect(result.source).toBe("unresolved");
  });
});

describe("resolveFonts", () => {
  it("dedupes families and resolves each once", async () => {
    let cssCalls = 0;
    const fetchImpl = fakeFetch({
      "https://fonts.googleapis.com/css2": () => {
        cssCalls++;
        return new Response(`url(https://fonts.gstatic.com/s/x/v1/font.ttf)`);
      },
      "https://fonts.gstatic.com/": () => new Response(new Uint8Array([9])),
    });
    const result = await resolveFonts(["Inter", "Inter", "Roboto"], { fetchImpl });
    expect(cssCalls).toBe(2);
    expect(result.size).toBe(2);
  });
});

describe("resolveFont — live Google Fonts integration", () => {
  // Skipped in CI: a real network call to fonts.googleapis.com, which would make CI flake on
  // Google's availability rather than this repo's own code. Still runs locally.
  it.skipIf(process.env.CI)("downloads a real ttf for a well-known family", async () => {
    const result = await resolveFont("Inter");
    expect(result.source).toBe("google-fonts");
    expect(result.data && result.data.length).toBeGreaterThan(1000);
  }, 15000);
});
