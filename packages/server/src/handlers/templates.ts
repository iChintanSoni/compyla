import type { IncomingMessage, ServerResponse } from "node:http";
import { extname } from "node:path";
import { inspectTemplate } from "@compyla/sdk";
import { MalformedRequestError, parseMultipart } from "../multipart.js";

// Same extension->kind mapping as `compyla template inspect` (packages/cli/src/commands/template.ts) —
// each app owns its own I/O mapping per D8; two entries isn't worth a shared package.
const KIND_BY_EXT: Record<string, string> = { ".docx": "docx", ".pptx": "pptx" };

/** `POST /templates` — multipart field: `template` (required file). Stateless: introspects and returns
 *  the report, nothing is persisted or referenced by ID (that's Phase 3's Template manager UI). */
export async function handleTemplates(req: IncomingMessage, res: ServerResponse): Promise<void> {
  const { files } = await parseMultipart(req);
  const template = files.template;
  if (!template) throw new MalformedRequestError('Missing required file field "template".');

  const ext = extname(template.filename).toLowerCase();
  const kind = KIND_BY_EXT[ext];
  if (!kind) {
    throw new MalformedRequestError(
      `Cannot inspect "${template.filename}": unsupported template extension "${ext}". Supported: ${Object.keys(KIND_BY_EXT).join(", ")}.`,
    );
  }

  const report = await inspectTemplate(template.buffer, kind);
  res.writeHead(200, { "Content-Type": "application/json" });
  res.end(JSON.stringify(report));
}
