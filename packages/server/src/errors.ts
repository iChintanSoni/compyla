import {
  RendererNotInstalledError,
  TemplateIntrospectorNotInstalledError,
  UnsupportedFormatError,
  UnsupportedTemplateKindError,
} from "@compyla/sdk";
import { MalformedRequestError } from "./multipart.js";

export interface HttpError {
  readonly status: number;
  readonly body: { readonly error: string };
}

// `validate()` rejects an unrecognized format via core's `getProfile()` with a plain `Error` (not
// `UnsupportedFormatError` — that class only guards the SDK's separate renderer registry, reached
// after validation passes), so an unknown-format request never actually surfaces as that class here.
// Matching the message is the pragmatic fix without changing core/sdk (out of scope for this package);
// the message itself is stable and already asserted verbatim by three existing tests upstream.
const UNKNOWN_FORMAT_RE = /^Unknown format "/;

/** An unknown format/kind is the client's mistake (bad input value) -> 400. A known format/kind whose
 *  opt-in package isn't installed on *this* deployment is a server capability gap, matching HTTP's own
 *  definition of 501 ("the server does not support the functionality required") -> 501. */
export function mapError(err: unknown): HttpError {
  if (err instanceof UnsupportedFormatError || err instanceof UnsupportedTemplateKindError || err instanceof MalformedRequestError) {
    return { status: 400, body: { error: err.message } };
  }
  if (err instanceof RendererNotInstalledError || err instanceof TemplateIntrospectorNotInstalledError) {
    return { status: 501, body: { error: err.message } };
  }
  if (err instanceof Error && UNKNOWN_FORMAT_RE.test(err.message)) {
    return { status: 400, body: { error: err.message } };
  }
  return { status: 500, body: { error: err instanceof Error ? err.message : String(err) } };
}
