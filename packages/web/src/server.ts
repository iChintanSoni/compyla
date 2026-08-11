import { readFileSync } from "node:fs";
import { createServer as createHttpServer, type IncomingMessage, type Server, type ServerResponse } from "node:http";
import { fileURLToPath } from "node:url";

type Handler = (res: ServerResponse) => void;

// Resolved relative to the package root (one level up from this file), not this file's own
// directory — that keeps it correct whether this runs compiled (dist/server.js) or, as in tests,
// transpiled in place from src/server.ts. Static assets only ever live in dist/static; the build
// step (esbuild) is what puts them there.
const staticDir = fileURLToPath(new URL("../dist/static", import.meta.url));

function asset(name: string, contentType: string): Handler {
  const body = readFileSync(`${staticDir}/${name}`);
  return (res) => {
    res.writeHead(200, { "Content-Type": contentType });
    res.end(body);
  };
}

/** Builds the server without listening — lets tests bind an ephemeral port and lets embedders control
 *  when/where it starts (see bin.ts for the CLI entry that actually calls .listen()). Reads the three
 *  static assets from dist/static once per call, so a missing build fails fast and clearly. */
export function createServer(): Server {
  const routes: Record<string, Handler> = {
    "GET /": asset("index.html", "text/html; charset=utf-8"),
    "GET /app.js": asset("app.js", "text/javascript; charset=utf-8"),
    "GET /style.css": asset("style.css", "text/css; charset=utf-8"),
  };

  return createHttpServer((req: IncomingMessage, res: ServerResponse) => {
    const pathname = new URL(req.url ?? "/", "http://localhost").pathname;
    const handler = routes[`${req.method} ${pathname}`];

    if (!handler) {
      res.writeHead(404, { "Content-Type": "text/plain" });
      res.end(`No route for ${req.method} ${pathname}.`);
      return;
    }

    handler(res);
  });
}
