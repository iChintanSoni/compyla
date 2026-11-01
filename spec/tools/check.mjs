#!/usr/bin/env node
// Consistency checks across the spec artifacts (spec/README.md):
//   - the diagnostics registry is well-formed and every code is documented in markdown-spec.md
//   - every fixture is well-formed, numbered per its oracle, and uses only registered codes with
//     their registered severity, stage and profiles
//   - every change ID a fixture cites exists, and every fixture the change table cites exists
//   node spec/tools/check.mjs             these checks (no dependencies)
//   node spec/tools/check.mjs --schemas   also validate schemas and instances (runs `uvx check-jsonschema`)

import { readdirSync, readFileSync, existsSync, mkdtempSync, writeFileSync, statSync } from "node:fs";
import { join, dirname } from "node:path";
import { tmpdir } from "node:os";
import { fileURLToPath } from "node:url";
import { execFileSync } from "node:child_process";

const root = join(dirname(fileURLToPath(import.meta.url)), "..", "..");
const specDir = join(root, "spec");
const conformance = join(specDir, "conformance");
const specText = readFileSync(join(root, "docs", "markdown-spec.md"), "utf8");

const problems = [];
const problem = (where, message) => problems.push(`${where}: ${message}`);
const readJson = (path) => JSON.parse(readFileSync(path, "utf8"));

// ---------- registry ----------

const registry = readJson(join(specDir, "diagnostics.json"));
const FORMATS = ["docx", "pdf", "pptx", "xlsx"];
const codes = new Map();
for (const entry of registry.diagnostics) {
  const where = `diagnostics.json ${entry.code}`;
  if (codes.has(entry.code)) problem(where, "duplicate code");
  codes.set(entry.code, entry);
  if (!/^[a-z][a-z0-9_]*$/.test(entry.code)) problem(where, "code must be snake_case");
  if (!["error", "warning"].includes(entry.severity)) problem(where, `bad severity ${entry.severity}`);
  if (!["parse", "validate", "render", "any"].includes(entry.stage)) problem(where, `bad stage ${entry.stage}`);
  if (!["active", "reserved", "retired"].includes(entry.status)) problem(where, `bad status ${entry.status}`);
  if (entry.severity === "error" && entry.strictPromotable) problem(where, "errors can't be strict-promotable");
  if (typeof entry.strictPromotable !== "boolean") problem(where, "strictPromotable must be boolean");
  if (entry.strictPromotable && entry.severity !== "warning") problem(where, "only warnings can be strict-promotable");
  for (const p of entry.profiles) if (p !== "*" && !FORMATS.includes(p)) problem(where, `bad profile ${p}`);
  if (!specText.includes(`\`${entry.code}\``)) problem(where, "not mentioned in docs/markdown-spec.md");
}
const operationErrors = new Set();
for (const entry of registry.operationErrors ?? []) {
  const where = `diagnostics.json operation error ${entry.code}`;
  if (operationErrors.has(entry.code) || codes.has(entry.code)) problem(where, "duplicate code");
  operationErrors.add(entry.code);
  if (!specText.includes(`\`${entry.code}\``)) problem(where, "not mentioned in docs/markdown-spec.md");
}

// ---------- change table ----------

const changeIds = new Set();
for (const match of specText.matchAll(/^\| (C\d+) \|.*\| ([^|]*) \|$/gm)) {
  changeIds.add(match[1]);
  for (const ref of match[2].matchAll(/(parse|lint|compose|render)\/(\d{3})/g)) {
    const dir = join(conformance, ref[1]);
    const found = existsSync(dir) && readdirSync(dir).some((n) => n.startsWith(`${ref[2]}-`));
    if (!found) problem(`markdown-spec.md ${match[1]}`, `cites ${ref[0]}, which doesn't exist`);
  }
}

// ---------- fixtures ----------

const CASE_KEYS = new Set(["description", "spec", "oracle", "format", "strict", "assets", "template"]);
const STAGES_BY_SUITE = { parse: ["parse", "any"], lint: ["parse", "validate", "any"], render: ["parse", "validate", "render", "any"] };
const fixtures = [];

for (const suite of ["parse", "lint", "compose", "render", "inspect"]) {
  const dir = join(conformance, suite);
  if (!existsSync(dir)) continue;
  for (const name of readdirSync(dir).sort()) {
    const fixture = join(dir, name);
    if (!statSync(fixture).isDirectory()) continue; // e.g. .DS_Store
    const where = `${suite}/${name}`;
    if (!/^\d{3}-[a-z0-9-]+$/.test(name)) problem(where, "directory must be NNN-kebab-name");
    const files = suite === "inspect" ? ["case.json", "expected.json"] : ["input.md", "case.json", "expected.json"];
    for (const file of files) {
      if (!existsSync(join(fixture, file))) problem(where, `missing ${file}`);
    }
    if (!existsSync(join(fixture, "case.json")) || !existsSync(join(fixture, "expected.json"))) continue;

    const kase = readJson(join(fixture, "case.json"));
    const expected = readJson(join(fixture, "expected.json"));
    fixtures.push({ suite, name, fixture, kase, expected });

    for (const key of Object.keys(kase)) if (!CASE_KEYS.has(key)) problem(where, `unknown case.json key ${key}`);
    if (typeof kase.description !== "string" || kase.description.length === 0) problem(where, "case.json needs a description");
    if (!Array.isArray(kase.spec) || kase.spec.length === 0) problem(where, "case.json needs spec references");
    if (!["prototype", "corrected", "none"].includes(kase.oracle)) problem(where, `bad oracle ${kase.oracle}`);
    if (["lint", "compose", "render"].includes(suite) && !FORMATS.includes(kase.format)) problem(where, "lint, compose and render cases need a format");
    if (suite === "inspect" && !kase.template) problem(where, "inspect cases name their template");
    if (kase.template && !existsSync(join(fixture, kase.template))) problem(where, `template ${kase.template} is missing`);

    const correction = name.startsWith("1");
    if (correction && kase.oracle === "prototype") problem(where, "1xx fixtures encode corrections; oracle can't be prototype");
    if (!correction && kase.oracle !== "prototype") problem(where, "0xx fixtures must agree with the prototype (oracle: prototype)");

    for (const ref of kase.spec ?? []) {
      if (/^C\d+$/.test(ref) && !changeIds.has(ref)) problem(where, `cites change ${ref}, which isn't in the change table`);
      if (/^§/.test(ref) && !specText.includes(`## ${ref.slice(1).split(".")[0]}.`)) problem(where, `cites ${ref}, which doesn't exist`);
    }

    if (suite === "parse" || suite === "lint" || suite === "render" || suite === "inspect") {
      const list = suite === "inspect" ? expected.issues : expected.diagnostics;
      if (!Array.isArray(list)) {
        problem(where, `expected.json needs a${suite === "inspect" ? "n issues" : " diagnostics"} array`);
        continue;
      }
      for (const d of list) {
        const entry = codes.get(d.code);
        if (!entry) {
          problem(where, `unregistered code ${d.code}`);
          continue;
        }
        const severity = kase.strict && entry.strictPromotable ? "error" : entry.severity;
        if (d.severity !== severity) problem(where, `${d.code} must be ${severity} here, not ${d.severity}`);
        if (suite === "inspect") continue;
        // The prototype emits no paths: its expectations can't pin one, and `--write` would drop it.
        if (d.path !== undefined && kase.oracle === "prototype") problem(where, `${d.code} has a path, which the prototype oracle can't check`);
        if (!STAGES_BY_SUITE[suite].includes(entry.stage)) problem(where, `${d.code} is a ${entry.stage}-stage code; it can't appear in the ${suite} suite`);
        if (suite !== "parse" && !entry.profiles.includes("*") && !entry.profiles.includes(kase.format)) {
          problem(where, `${d.code} doesn't apply to ${kase.format}`);
        }
      }
    }
  }
}

// ---------- section references ----------

const sections = new Set();
for (const match of specText.matchAll(/^##+ (\d+(?:\.\d+)?)\.? /gm)) sections.add(match[1]);
const referencing = [
  ...readdirSync(specDir).filter((f) => f.endsWith(".json")).map((f) => join(specDir, f)),
  ...readdirSync(join(specDir, "conformance")).filter((f) => f.endsWith(".json")).map((f) => join(specDir, "conformance", f)),
  join(specDir, "conformance", "README.md"),
  ...fixtures.map(({ fixture }) => join(fixture, "case.json")),
];
for (const file of referencing) {
  for (const match of readFileSync(file, "utf8").matchAll(/§(\d+(?:\.\d+)?)/g)) {
    if (!sections.has(match[1])) problem(file.slice(root.length + 1), `cites §${match[1]}, which isn't a section of markdown-spec.md`);
  }
}

// ---------- JSON Schema validation (optional) ----------

if (process.argv.includes("--schemas")) {
  const work = mkdtempSync(join(tmpdir(), "compyla-spec-"));
  const run = (args) => {
    try {
      execFileSync("uvx", ["--from", "check-jsonschema==0.39.0", "check-jsonschema", ...args], { stdio: "pipe" });
    } catch (error) {
      problem("schemas", `check-jsonschema ${args.join(" ")}\n${error.stdout ?? ""}${error.stderr ?? ""}`);
    }
  };
  const schemas = ["ir.schema.json", "diagnostic.schema.json", "diagnostics.schema.json", "template-report.schema.json", "lockfile.schema.json", "conformance/case.schema.json", "conformance/compose.schema.json", "conformance/render.schema.json"];
  run(["--check-metaschema", ...schemas.map((s) => join(specDir, s))]);
  run(["--schemafile", join(specDir, "diagnostics.schema.json"), join(specDir, "diagnostics.json")]);
  run(["--schemafile", join(specDir, "lockfile.schema.json"), join(specDir, "lockfile.example.json")]);

  const instances = { ir: [], compose: [], render: [], inspect: [], case: [] };
  for (const { suite, name, fixture, expected } of fixtures) {
    instances.case.push(join(fixture, "case.json"));
    if (suite === "parse") {
      const file = join(work, `${name}.ir.json`);
      writeFileSync(file, JSON.stringify(expected.ir));
      instances.ir.push(file);
    }
    if (suite === "compose") instances.compose.push(join(fixture, "expected.json"));
    if (suite === "render") instances.render.push(join(fixture, "expected.json"));
    if (suite === "inspect") instances.inspect.push(join(fixture, "expected.json"));
  }
  if (instances.ir.length) run(["--schemafile", join(specDir, "ir.schema.json"), ...instances.ir]);
  if (instances.compose.length) run(["--schemafile", join(specDir, "conformance/compose.schema.json"), ...instances.compose]);
  if (instances.render.length) run(["--schemafile", join(specDir, "conformance/render.schema.json"), ...instances.render]);
  // The report schema refers to the diagnostic schema by $id: embed it, so the reference resolves.
  if (instances.inspect.length) {
    const bundled = join(work, "template-report.bundled.json");
    const report = readJson(join(specDir, "template-report.schema.json"));
    report.$defs = { diagnostic: readJson(join(specDir, "diagnostic.schema.json")) };
    writeFileSync(bundled, JSON.stringify(report)); // the embedded schema keeps its $id, which the $ref names
    run(["--schemafile", bundled, ...instances.inspect]);
  }
  if (instances.case.length) run(["--schemafile", join(specDir, "conformance/case.schema.json"), ...instances.case]);
}

if (problems.length > 0) {
  for (const p of problems) console.log(`FAIL  ${p}`);
  console.log(`\n${problems.length} problem(s)`);
  process.exit(1);
}
console.log(`ok  ${codes.size} codes, ${operationErrors.size} operation errors, ${changeIds.size} changes, ${fixtures.length} fixtures`);
