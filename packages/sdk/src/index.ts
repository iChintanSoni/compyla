export { convert, type ConvertOptions, type ConvertResult } from "./convert.js";
export { lint, type LintOptions, type LintResult } from "./lint.js";
export { inspectTemplate, UnsupportedTemplateKindError, TemplateIntrospectorNotInstalledError } from "./template.js";
export {
  loadRenderer,
  UnsupportedFormatError,
  RendererNotInstalledError,
  type RendererModule,
  type RenderInvocationOptions,
  type RenderOutcome,
} from "./renderer-registry.js";
export type { Diagnostic } from "@compyla/core";
