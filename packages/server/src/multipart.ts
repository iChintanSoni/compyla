import type { IncomingMessage } from "node:http";
import Busboy from "busboy";

export interface MultipartFile {
  readonly filename: string;
  readonly buffer: Buffer;
}

export interface MultipartResult {
  readonly fields: Record<string, string>;
  readonly files: Record<string, MultipartFile>;
}

export class MalformedRequestError extends Error {}

/** Thin `busboy` wrapper: buffers every file part fully in memory (fine for markdown/office-template
 *  sized uploads) and resolves once the whole request body has been consumed. */
export function parseMultipart(req: IncomingMessage): Promise<MultipartResult> {
  return new Promise((resolve, reject) => {
    const contentType = req.headers["content-type"];
    if (!contentType || !contentType.startsWith("multipart/form-data")) {
      reject(new MalformedRequestError("Expected a multipart/form-data request body."));
      return;
    }

    const bb = Busboy({ headers: req.headers });
    const fields: Record<string, string> = {};
    const files: Record<string, MultipartFile> = {};
    const filesDone: Promise<void>[] = [];
    let settled = false;

    const fail = (err: unknown) => {
      if (settled) return;
      settled = true;
      reject(err instanceof Error ? err : new MalformedRequestError(String(err)));
    };

    bb.on("field", (name, value) => {
      fields[name] = value;
    });

    bb.on("file", (name, stream, info) => {
      const chunks: Buffer[] = [];
      filesDone.push(
        new Promise((resolveFile, rejectFile) => {
          stream.on("data", (chunk: Buffer) => chunks.push(chunk));
          stream.on("end", () => {
            files[name] = { filename: info.filename, buffer: Buffer.concat(chunks) };
            resolveFile();
          });
          stream.on("error", rejectFile);
        }),
      );
    });

    bb.on("error", fail);
    bb.on("close", () => {
      if (settled) return;
      Promise.all(filesDone)
        .then(() => {
          settled = true;
          resolve({ fields, files });
        })
        .catch(fail);
    });

    req.on("error", fail);
    req.pipe(bb);
  });
}
