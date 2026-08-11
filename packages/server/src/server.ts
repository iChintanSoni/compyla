import { createServer as createHttpServer, type IncomingMessage, type Server, type ServerResponse } from "node:http";
import { mapError } from "./errors.js";
import { handleConvert } from "./handlers/convert.js";
import { handleTemplates } from "./handlers/templates.js";
import { handleValidate } from "./handlers/validate.js";

type Handler = (req: IncomingMessage, res: ServerResponse) => Promise<void>;

const ROUTES: Record<string, Handler> = {
  "POST /convert": handleConvert,
  "POST /validate": handleValidate,
  "POST /templates": handleTemplates,
};

/** Builds the server without listening — lets tests bind an ephemeral port and lets embedders control
 *  when/where it starts (see bin.ts for the CLI entry that actually calls .listen()). */
export function createServer(): Server {
  return createHttpServer((req, res) => {
    void handleRequest(req, res);
  });
}

async function handleRequest(req: IncomingMessage, res: ServerResponse): Promise<void> {
  const pathname = new URL(req.url ?? "/", "http://localhost").pathname;
  const handler = ROUTES[`${req.method} ${pathname}`];

  if (!handler) {
    res.writeHead(404, { "Content-Type": "application/json" });
    res.end(JSON.stringify({ error: `No route for ${req.method} ${pathname}.` }));
    return;
  }

  try {
    await handler(req, res);
  } catch (err) {
    const { status, body } = mapError(err);
    res.writeHead(status, { "Content-Type": "application/json" });
    res.end(JSON.stringify(body));
  }
}
