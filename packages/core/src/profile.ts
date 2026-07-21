import type { Diagnostic } from "./diagnostics.js";
import type { CompylaDocument } from "./types.js";

export interface ValidateOptions {
  readonly strict?: boolean;
  /** Resolve an image src (e.g. against an assets dir). Defaults to "always resolves" if omitted. */
  readonly resolveAsset?: (src: string) => boolean;
}

export interface Profile {
  readonly name: string;
  readonly knownDirectives: readonly string[];
  validate(doc: CompylaDocument, options: ValidateOptions): Diagnostic[];
}
