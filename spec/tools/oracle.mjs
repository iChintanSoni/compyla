#!/usr/bin/env node
// Runs the TypeScript reference prototype against every conformance fixture (spec/conformance/README.md).
//   node spec/tools/oracle.mjs           check
//   node spec/tools/oracle.mjs --write   regenerate expected.json for oracle: "prototype" fixtures
// Requires the prototype to be built (`pnpm build`). Exists only until the Rust core passes the suite.

import { readdirSync, readFileSync, writeFileSync, existsSync, mkdtempSync, statSync } from "node:fs";
import { join, dirname } from "node:path";
import { tmpdir } from "node:os";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

const root = join(dirname(fileURLToPath(import.meta.url)), "..", "..");
const conformance = join(root, "spec", "conformance");
const write = process.argv.includes("--write");

const core = await import(join(root, "packages/core/dist/index.js"));
const pptx = await import(join(root, "packages/pptx/dist/index.js"));
const sdk = await import(join(root, "packages/sdk/dist/index.js"));
const renders = mkdtempSync(join(tmpdir(), "compyla-render-"));

// ---------- prototype output → spec shapes ----------

const MARKS = ["bold", "italic", "code", "strike"];

/** Canonical runs (markdown-spec.md §4.4): marks only when set, adjacent identical runs merged, empty runs dropped. */
function toRuns(runs) {
  const out = [];
  for (const r of runs) {
    if (r.text === "") continue;
    const run = { text: r.text };
    for (const m of MARKS) if (r[m]) run[m] = true;
    if (r.link !== undefined) run.link = r.link;
    const prev = out[out.length - 1];
    if (prev && sameMarks(prev, run)) prev.text += run.text;
    else out.push(run);
  }
  return out;
}

function sameMarks(a, b) {
  return MARKS.every((m) => a[m] === b[m]) && a.link === b.link;
}

const pos = (p) => ({ line: p.line, column: p.column });

function toBlock(n) {
  switch (n.type) {
    case "section":
      return { type: "section", level: n.level, title: toRuns(n.title), children: n.children.map(toBlock), position: pos(n.position) };
    case "paragraph":
      return { type: "paragraph", runs: toRuns(n.runs), position: pos(n.position) };
    case "table":
      return {
        type: "table",
        header: n.header.map(toCell),
        align: [...n.align],
        rows: n.rows.map((row) => row.map(toCell)),
        position: pos(n.position),
      };
    case "list":
      return {
        type: "list",
        ordered: n.ordered,
        items: n.items.map((i) => ({ children: i.children.map(toBlock), checked: i.checked, position: pos(i.position) })),
        position: pos(n.position),
      };
    case "media":
      return { type: "media", src: n.src, alt: n.alt, position: pos(n.position) };
    case "code":
      return n.lang === undefined
        ? { type: "code", value: n.value, position: pos(n.position) }
        : { type: "code", lang: n.lang, value: n.value, position: pos(n.position) };
    case "quote":
      return { type: "quote", children: n.children.map(toBlock), position: pos(n.position) };
    case "break":
      return { type: "break", position: pos(n.position) };
    case "directive":
      return { type: "directive", name: n.name, args: n.argsRaw, children: n.children.map(toBlock), position: pos(n.position) };
    default:
      throw new Error(`oracle: unmapped prototype node type ${n.type}`);
  }
}

const toCell = (c) => ({ runs: toRuns(c.runs), raw: c.raw });

function toIr(doc) {
  return { irVersion: "0.1", meta: doc.meta, body: doc.body.map(toBlock) };
}

function toDiagnostics(list) {
  return list
    .map((d) => (d.column === undefined ? { code: d.code, severity: d.severity, line: d.line } : { code: d.code, severity: d.severity, line: d.line, column: d.column }))
    .sort(compareDiagnostics);
}

export function compareDiagnostics(a, b) {
  return (a.line ?? 0) - (b.line ?? 0) || (a.column ?? 0) - (b.column ?? 0) || a.code.localeCompare(b.code);
}

const plainText = (runs) => runs.map((r) => r.text).join("");

function contentType(n) {
  if (n.type === "directive") return n.name === "col" ? "col" : `directive:${n.name}`;
  if (n.type === "section") return "heading";
  return n.type;
}

function pptxSummary(doc) {
  const slides = core.groupSlides(doc.body);
  return {
    format: "pptx",
    slides: slides.map((slide) => {
      const { layoutDirective, notesDirective, content } = core.resolveSlideContent(slide);
      let layout;
      const resolved = layoutDirective ? core.resolvePptxLayoutName(layoutDirective.argsRaw) : undefined;
      if (resolved) layout = resolved;
      else if (slide.index === 0 && slides.length > 1) layout = "Title Slide";
      else layout = pptx.inferLayout(content, true);
      return {
        title: slide.title ? plainText(slide.title) : null,
        layout,
        notes: notesDirective !== undefined,
        content: content.map(contentType),
      };
    }),
  };
}

// ---------- run one fixture through the prototype ----------

function runPrototype(suite, input, kase) {
  const doc = core.parse(input);
  if (suite === "parse") return { ir: toIr(doc), diagnostics: [] };
  if (suite === "lint") {
    const assets = kase.assets ? new Set(kase.assets) : undefined;
    const diagnostics = core.validate(doc, { format: kase.format, resolveAsset: assets ? (src) => assets.has(src) : undefined });
    return { diagnostics: toDiagnostics(core.escalateStrict(diagnostics, kase.strict ?? false)) };
  }
  if (suite === "compose" && kase.format === "pptx") return pptxSummary(doc);
  return undefined;
}

/** Render suite: renders twice (bytes must match), then checks diagnostics and the projection
 *  with spec/tools/render_check.py. Returns the list of differences from expected.json. */
async function renderDifferences(name, input, kase, fixture, expected) {
  const assets = kase.assets ? new Set(kase.assets) : undefined;
  const options = {
    format: kase.format,
    strict: kase.strict ?? false,
    resolveAsset: assets ? (src) => assets.has(src) : undefined,
    readAsset: (src) => readFileSync(join(fixture, src)),
  };
  if (kase.template) options.template = readFileSync(join(fixture, kase.template));
  const first = await sdk.convert(input, options);
  const second = await sdk.convert(input, options);
  const differences = [];
  if (!first.buffer) return [`no output: ${JSON.stringify(first.diagnostics.map((d) => d.code))}`];
  if (!second.buffer || Buffer.compare(first.buffer, second.buffer) !== 0) differences.push("two renders differ (not reproducible)");
  if (!diagnosticsMatch(toDiagnostics(first.diagnostics), expected.diagnostics)) {
    differences.push(`diagnostics: ${JSON.stringify(toDiagnostics(first.diagnostics))}`);
  }
  const file = join(renders, `${name}.${kase.format}`);
  writeFileSync(file, first.buffer);
  const check = spawnSync(
    "uv",
    ["run", "-q", "--no-project", "--with", "lxml==6.0.2", "--with", "pypdf==6.1.1", "python", "-I", join(root, "spec/tools/render_check.py"),
      "check", kase.format, join(fixture, "expected.json"), file],
    { encoding: "utf8" },
  );
  if (check.status !== 0) differences.push(...(check.stdout + check.stderr).trim().split("\n"));
  return differences;
}

// ---------- order-insensitive JSON comparison ----------

function canonical(value) {
  if (Array.isArray(value)) return value.map(canonical);
  if (value && typeof value === "object") {
    return Object.fromEntries(Object.keys(value).sort().map((k) => [k, canonical(value[k])]));
  }
  return value;
}

const same = (a, b) => JSON.stringify(canonical(a)) === JSON.stringify(canonical(b));

/** markdown-spec.md §11.3: compared in order, keeping duplicates; `line` always compared; `column`
 *  only when the expected diagnostic has one; message and hint never. */
function diagnosticsMatch(actual, expected) {
  return (
    actual.length === expected.length &&
    actual.every(
      (a, i) =>
        a.code === expected[i].code &&
        a.severity === expected[i].severity &&
        a.line === expected[i].line &&
        (expected[i].column === undefined || a.column === expected[i].column),
    )
  );
}

/** Whether the prototype's output matches a fixture's expectation, per suite. */
function matches(suite, actual, expected) {
  if (actual === undefined || actual.prototypeThrew) return false;
  if (suite === "parse") return same(actual.ir, expected.ir) && diagnosticsMatch(actual.diagnostics, expected.diagnostics);
  if (suite === "lint") return diagnosticsMatch(actual.diagnostics, expected.diagnostics);
  return same(actual, expected);
}

// ---------- main ----------

let failures = 0;
for (const suite of ["parse", "lint", "compose", "render"]) {
  const dir = join(conformance, suite);
  if (!existsSync(dir)) continue;
  for (const name of readdirSync(dir).sort()) {
    const fixture = join(dir, name);
    if (!statSync(fixture).isDirectory()) continue; // e.g. .DS_Store
    const kase = JSON.parse(readFileSync(join(fixture, "case.json"), "utf8"));
    const input = readFileSync(join(fixture, "input.md"), "utf8");
    const expectedPath = join(fixture, "expected.json");
    const label = `${suite}/${name}`;

    if (kase.oracle === "none") {
      console.log(`skip     ${label}`);
      continue;
    }

    if (suite === "render") {
      // Render expectations are always hand-written: a projection is a chosen subset of the output.
      if (write) {
        console.log(`skip     ${label} (render expectations are hand-written)`);
        continue;
      }
      const differences = await renderDifferences(name, input, kase, fixture, JSON.parse(readFileSync(expectedPath, "utf8")));
      if (kase.oracle === "prototype" && differences.length > 0) {
        failures++;
        console.log(`FAIL     ${label}: prototype output doesn't satisfy expected.json`);
        for (const d of differences) console.log(`         ${d}`);
      } else if (kase.oracle === "corrected" && differences.length === 0) {
        failures++;
        console.log(`FAIL     ${label}: marked "corrected" but the prototype already satisfies expected.json`);
      } else {
        console.log(`ok       ${label}${kase.oracle === "corrected" ? ` (corrects the prototype: ${differences[0]})` : ""}`);
      }
      continue;
    }

    let actual;
    try {
      actual = runPrototype(suite, input, kase);
    } catch (error) {
      actual = { prototypeThrew: String(error) };
    }

    if (kase.oracle === "prototype") {
      if (write) {
        if (actual === undefined || actual.prototypeThrew) {
          failures++;
          console.log(`FAIL     ${label}: the prototype can't produce this (${actual?.prototypeThrew ?? "unsupported suite/format"}); not written`);
          continue;
        }
        writeFileSync(expectedPath, `${JSON.stringify(actual, null, 2)}\n`);
        console.log(`wrote    ${label}`);
        continue;
      }
      const expected = JSON.parse(readFileSync(expectedPath, "utf8"));
      if (matches(suite, actual, expected)) console.log(`ok       ${label}`);
      else {
        failures++;
        console.log(`FAIL     ${label}: prototype output differs from expected.json`);
        console.log(`         prototype: ${JSON.stringify(actual)}`);
      }
      continue;
    }

    // corrected: the spec must genuinely differ from the prototype
    const expected = JSON.parse(readFileSync(expectedPath, "utf8"));
    if (matches(suite, actual, expected)) {
      failures++;
      console.log(`FAIL     ${label}: marked "corrected" but the prototype already produces expected.json`);
    } else {
      console.log(`ok       ${label} (corrects the prototype)`);
    }
  }
}

if (failures > 0) {
  console.log(`\n${failures} failure(s)`);
  process.exit(1);
}
