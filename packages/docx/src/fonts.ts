export type FontSource = "user-supplied" | "google-fonts" | "unresolved";

export interface ResolvedFont {
  readonly family: string;
  readonly source: FontSource;
  readonly data?: Buffer;
}

export interface FontResolverOptions {
  /** Font files supplied directly by the caller, keyed by family name. Wins over Google Fonts. */
  readonly userFonts?: ReadonlyMap<string, Buffer>;
  /** Injectable for tests; defaults to the global fetch. */
  readonly fetchImpl?: typeof fetch;
}

const GOOGLE_FONTS_CSS_URL = "https://fonts.googleapis.com/css2";
// Google Fonts format-negotiates by parsing the User-Agent against known browser-version
// regexes: a *recognized* modern browser UA gets woff2 (unusable for OOXML embedding), while an
// unrecognized/generic UA falls back to the universally-compatible format: ttf. So counter-
// intuitively, a bare "Mozilla/5.0" is more robust here than a real Chrome UA string, which
// would need to keep chasing Chrome's version number to keep working.
const GENERIC_UA = "Mozilla/5.0";
const TTF_URL_RE = /url\((https:\/\/fonts\.gstatic\.com\/[^)]+\.ttf)\)/;

/**
 * Fonts are never named in markdown (see markdown-spec.md §3) — the template supplies the family
 * name, and this resolves it to embeddable bytes. Template-embedded font *reuse* (de-obfuscating
 * OOXML .fntdata parts) is out of Phase-0 scope; template introspection only reports whether a
 * font is referenced, it does not re-embed it.
 */
export async function resolveFont(family: string, options: FontResolverOptions = {}): Promise<ResolvedFont> {
  const userFont = options.userFonts?.get(family);
  if (userFont) return { family, source: "user-supplied", data: userFont };

  const fetchImpl = options.fetchImpl ?? fetch;
  try {
    const cssUrl = `${GOOGLE_FONTS_CSS_URL}?family=${encodeURIComponent(family)}:wght@400;700`;
    const cssResponse = await fetchImpl(cssUrl, { headers: { "User-Agent": GENERIC_UA } });
    if (!cssResponse.ok) return { family, source: "unresolved" };

    const css = await cssResponse.text();
    const match = TTF_URL_RE.exec(css);
    if (!match) return { family, source: "unresolved" };

    const fontResponse = await fetchImpl(match[1]!);
    if (!fontResponse.ok) return { family, source: "unresolved" };

    const data = Buffer.from(await fontResponse.arrayBuffer());
    return { family, source: "google-fonts", data };
  } catch {
    return { family, source: "unresolved" };
  }
}

export async function resolveFonts(
  families: readonly string[],
  options: FontResolverOptions = {},
): Promise<Map<string, ResolvedFont>> {
  const unique = [...new Set(families)];
  const resolved = await Promise.all(unique.map((family) => resolveFont(family, options)));
  return new Map(resolved.map((r) => [r.family, r]));
}
