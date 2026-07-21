import JSZip from "jszip";

// Any fixed instant works; the point is that it never changes between renders.
const FIXED_DATE = new Date(Date.UTC(2000, 0, 1));
const FIXED_TIMESTAMP = "2000-01-01T00:00:00Z";

// Every OOXML package (docx/pptx/xlsx alike, per ECMA-376 Part 2 OPC) stamps
// docProps/core.xml with the current time, and some generators (verified: the `docx` npm
// library, for hyperlink relationship IDs specifically) assign relationship IDs via unseeded
// randomness with no override in their public API. Relationship IDs are opaque cross-reference
// tokens by spec — the file format doesn't care what they equal, only that references match —
// so renumbering them canonically in first-appearance order is a safe, meaning-preserving
// normalization, not a content change. This runs against any part's *.rels file, so it applies
// uniformly to word/_rels, ppt/_rels, ppt/slides/_rels, etc.
const RELS_PATH_RE = /(^|\/)_rels\/[^/]+\.rels$/;
const HYPERLINK_RELATIONSHIP_RE = /<Relationship Id="([^"]+)"[^>]*Type="[^"]*\/hyperlink"[^>]*\/>/g;

async function canonicalizeHyperlinkIds(zip: JSZip): Promise<void> {
  const relsFiles = Object.keys(zip.files).filter((name) => RELS_PATH_RE.test(name));

  for (const relsPath of relsFiles) {
    const relsFile = zip.file(relsPath);
    if (!relsFile) continue;
    const relsContent = await relsFile.async("string");

    const idMap = new Map<string, string>();
    let counter = 1;
    for (const match of relsContent.matchAll(HYPERLINK_RELATIONSHIP_RE)) {
      const oldId = match[1]!;
      if (!idMap.has(oldId)) idMap.set(oldId, `rIdHyperlink${counter++}`);
    }
    if (idMap.size === 0) continue;

    let newRelsContent = relsContent;
    for (const [oldId, newId] of idMap) {
      newRelsContent = newRelsContent.split(`Id="${oldId}"`).join(`Id="${newId}"`);
    }
    zip.file(relsPath, newRelsContent);

    // foo/_rels/bar.xml.rels describes foo/bar.xml.
    const partPath = relsPath.replace("_rels/", "").replace(/\.rels$/, "");
    const partFile = zip.file(partPath);
    if (!partFile) continue;
    let partContent = await partFile.async("string");
    for (const [oldId, newId] of idMap) {
      partContent = partContent.split(`r:id="${oldId}"`).join(`r:id="${newId}"`);
    }
    zip.file(partPath, partContent);
  }
}

/**
 * Repackages an OOXML zip (docx/pptx/xlsx) so identical input yields byte-identical output (D6):
 * pins the docProps/core.xml timestamp, canonicalizes hyperlink relationship IDs, and rebuilds
 * every entry (including directory entries, which JSZip's own `folder()` helper does not let you
 * date-pin) in a fixed, sorted order with fixed compression settings.
 */
export async function repackageDeterministically(raw: Buffer): Promise<Buffer> {
  const zip = await JSZip.loadAsync(raw);
  await canonicalizeHyperlinkIds(zip);

  const coreFile = zip.file("docProps/core.xml");
  if (coreFile) {
    const core = await coreFile.async("string");
    const patched = core
      .replace(/<dcterms:created([^>]*)>[^<]*<\/dcterms:created>/, `<dcterms:created$1>${FIXED_TIMESTAMP}</dcterms:created>`)
      .replace(/<dcterms:modified([^>]*)>[^<]*<\/dcterms:modified>/, `<dcterms:modified$1>${FIXED_TIMESTAMP}</dcterms:modified>`);
    zip.file("docProps/core.xml", patched);
  }

  const out = new JSZip();
  const names = Object.keys(zip.files).sort();
  for (const name of names) {
    const entry = zip.files[name]!;
    if (entry.dir) {
      out.file(name, null, { dir: true, date: FIXED_DATE, unixPermissions: "755" });
      continue;
    }
    const content = await entry.async("nodebuffer");
    out.file(name, content, { date: FIXED_DATE, unixPermissions: "644" });
  }

  return out.generateAsync({
    type: "nodebuffer",
    compression: "DEFLATE",
    compressionOptions: { level: 6 },
    platform: "UNIX",
  });
}
