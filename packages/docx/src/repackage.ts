import JSZip from "jszip";

// Any fixed instant works; the point is that it never changes between renders.
const FIXED_DATE = new Date(Date.UTC(2000, 0, 1));
const FIXED_TIMESTAMP = "2000-01-01T00:00:00Z";

// The `docx` library assigns hyperlink relationship IDs via nanoid (unseeded Math.random()),
// with no override in its public API — verified the only other non-deterministic part of its
// output (headings, tables, images, TOC, and sequential relationship IDs like fontTable are all
// already stable across runs). Relationship IDs are opaque cross-reference tokens by spec — Word
// doesn't care what they equal, only that references match — so renumbering them canonically in
// first-appearance order is a safe, meaning-preserving normalization, not a content change.
const HYPERLINK_RELATIONSHIP_RE = /<Relationship Id="([^"]+)"[^>]*Type="[^"]*\/hyperlink"[^>]*\/>/g;

async function canonicalizeHyperlinkIds(zip: JSZip): Promise<void> {
  const relsFiles = Object.keys(zip.files).filter((name) => name.startsWith("word/_rels/") && name.endsWith(".rels"));

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

    // word/_rels/foo.xml.rels describes word/foo.xml.
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
 * The `docx` library stamps docProps/core.xml with `new Date()` on every Packer.toBuffer() call
 * and does not expose an override for it. Combined with hyperlink relationship IDs (above),
 * these are the only non-deterministic bytes in an otherwise deterministic zip. This repackages
 * the zip with both pinned and every entry rebuilt in a fixed, sorted order so identical input
 * yields byte-identical output (D6).
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
      // zip.folder() does not accept a date option and defaults directory entries to "now" —
      // use file() with dir:true directly so folder entries are pinned too.
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
