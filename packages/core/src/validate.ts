import type { Diagnostic } from "./diagnostics.js";
import type { Profile, ValidateOptions } from "./profile.js";
import { docxProfile } from "./profiles/docx.js";
import { pptxProfile } from "./profiles/pptx.js";
import { xlsxProfile } from "./profiles/xlsx.js";
import type { CompylaDocument } from "./types.js";

const PROFILES: Record<string, Profile> = {
  docx: docxProfile,
  pptx: pptxProfile,
  xlsx: xlsxProfile,
};

export interface ValidateCallOptions extends ValidateOptions {
  readonly format: string;
}

export function getProfile(format: string): Profile {
  const profile = PROFILES[format];
  if (!profile) {
    throw new Error(`Unknown format "${format}". Supported: ${Object.keys(PROFILES).join(", ")}.`);
  }
  return profile;
}

export function validate(doc: CompylaDocument, options: ValidateCallOptions): Diagnostic[] {
  return getProfile(options.format).validate(doc, options);
}
