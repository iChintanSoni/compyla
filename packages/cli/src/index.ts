#!/usr/bin/env node
import { Command } from "commander";
import { runConvert } from "./commands/convert.js";
import { runLint } from "./commands/lint.js";
import { runTemplateInspect } from "./commands/template.js";

// Every command runs through this so an unexpected error (e.g. an unsupported --format) prints
// one clean line and exits 1, instead of an uncaught-exception stack trace.
function handle<Args extends unknown[]>(fn: (...args: Args) => Promise<number>): (...args: Args) => Promise<void> {
  return async (...args: Args) => {
    try {
      process.exitCode = await fn(...args);
    } catch (err) {
      console.error(err instanceof Error ? err.message : String(err));
      process.exitCode = 1;
    }
  };
}

const program = new Command();

program.name("compyla").description("Convert Markdown into docx, pdf, pptx, and xlsx.").version("0.1.0");

program
  .command("convert")
  .argument("<file>", "markdown file to convert")
  .description("Convert a markdown file to the target format")
  .requiredOption("--format <format>", "output format (docx | pptx | xlsx)")
  .option("--template <path>", "template file path")
  .option("--out <path>", "output file path")
  .option("--assets <dir>", "directory relative image paths resolve against")
  .option("--strict", "escalate fidelity warnings (e.g. unresolved fonts) to errors", false)
  .action(handle(runConvert));

program
  .command("lint")
  .argument("<file>", "markdown file to validate")
  .description("Validate a markdown file against a format profile, without rendering")
  .requiredOption("--format <format>", "target format profile (docx | pptx | xlsx)")
  .option("--assets <dir>", "directory relative image paths resolve against")
  .option("--strict", "escalate fidelity warnings to errors", false)
  .action(handle(runLint));

const template = program.command("template").description("template utilities");

template
  .command("inspect")
  .argument("<file>", "template file (.docx)")
  .description("report the styles, theme, and fonts Compyla detects in a template")
  .action(handle(runTemplateInspect));

await program.parseAsync(process.argv);
