import { buildDocument } from "./build-tokens.js";
import { parseMarkdown } from "./parser.js";
import type { CompylaDocument } from "./types.js";

export { parseMarkdown } from "./parser.js";
export type { AugmentedContent, CompylaDirectiveMdastNode, ParseResult } from "./parser.js";
export { buildDocument } from "./build-tokens.js";
export { validate, getProfile } from "./validate.js";
export type { ValidateCallOptions } from "./validate.js";
export { escalateStrict } from "./diagnostics.js";
export type { Diagnostic, Severity } from "./diagnostics.js";
export type { Profile, ValidateOptions } from "./profile.js";
export { walkBody } from "./walk.js";
export type { WalkVisitor } from "./walk.js";
export * from "./types.js";

/** Parse markdown straight to a Compyla document — the common entry point. */
export function parse(source: string): CompylaDocument {
  return buildDocument(parseMarkdown(source));
}
