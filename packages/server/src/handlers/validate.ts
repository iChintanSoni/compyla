import type { IncomingMessage, ServerResponse } from "node:http";
import { lint } from "@compyla/sdk";
import { MalformedRequestError, parseMultipart } from "../multipart.js";

/** `POST /validate` — multipart fields: `markdown` (required text), `format` (required text), `strict`
 *  (optional text, "true"/"1"). Mirrors `compyla lint`: offline, no renderer ever loaded. `200` when
 *  clean or warning-only, `422` when a blocking error is present — matching the CLI's exit-code rule. */
export async function handleValidate(req: IncomingMessage, res: ServerResponse): Promise<void> {
  const { fields } = await parseMultipart(req);
  const markdown = fields.markdown;
  const format = fields.format;
  if (!markdown) throw new MalformedRequestError('Missing required field "markdown".');
  if (!format) throw new MalformedRequestError('Missing required field "format".');
  const strict = fields.strict === "true" || fields.strict === "1";

  const result = lint(markdown, { format, strict, resolveAsset: () => false });

  res.writeHead(result.ok ? 200 : 422, { "Content-Type": "application/json" });
  res.end(JSON.stringify({ ok: result.ok, diagnostics: result.diagnostics }));
}
