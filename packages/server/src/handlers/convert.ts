import type { IncomingMessage, ServerResponse } from "node:http";
import { convert } from "@compyla/sdk";
import { MalformedRequestError, parseMultipart } from "../multipart.js";

const CONTENT_TYPES: Record<string, string> = {
  docx: "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
  pdf: "application/pdf",
  pptx: "application/vnd.openxmlformats-officedocument.presentationml.presentation",
  xlsx: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
};

/** `POST /convert` — multipart fields: `markdown` (required text), `format` (required text), `strict`
 *  (optional text, "true"/"1"), `template` (optional file). Success returns the converted file's raw
 *  bytes directly (no signed URL — this is the stateless, no-storage self-hosted server; see the plan).
 *  Non-blocking warnings ride along in `X-Compyla-Diagnostics` rather than breaking the binary body. */
export async function handleConvert(req: IncomingMessage, res: ServerResponse): Promise<void> {
  const { fields, files } = await parseMultipart(req);
  const markdown = fields.markdown;
  const format = fields.format;
  if (!markdown) throw new MalformedRequestError('Missing required field "markdown".');
  if (!format) throw new MalformedRequestError('Missing required field "format".');
  const strict = fields.strict === "true" || fields.strict === "1";

  const result = await convert(markdown, {
    format,
    strict,
    template: files.template?.buffer,
    // No asset-upload support yet: reporting every image as unresolved fails validation cleanly
    // (image_unresolved, 422) rather than crashing at render time with no readAsset to call.
    resolveAsset: () => false,
  });

  if (!result.ok || !result.buffer) {
    res.writeHead(422, { "Content-Type": "application/json" });
    res.end(JSON.stringify({ ok: false, diagnostics: result.diagnostics }));
    return;
  }

  const headers: Record<string, string> = {
    "Content-Type": CONTENT_TYPES[format] ?? "application/octet-stream",
    "Content-Disposition": `attachment; filename="output.${format}"`,
  };
  if (result.diagnostics.length > 0) {
    headers["X-Compyla-Diagnostics"] = JSON.stringify(result.diagnostics);
  }
  res.writeHead(200, headers);
  res.end(result.buffer);
}
