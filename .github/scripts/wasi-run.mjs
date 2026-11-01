// Runs a wasm32-wasip1 binary, such as a test binary, under Node's WASI, so a crate's tests run on
// wasm32 as they do natively (compyla-fonts: measurement is the same in every host, §13):
//
//   CARGO_TARGET_WASM32_WASIP1_RUNNER="node $PWD/.github/scripts/wasi-run.mjs" \
//     cargo test -p compyla-fonts --target wasm32-wasip1
//
// The binary gets its arguments and environment, and no filesystem.
import { readFile } from "node:fs/promises";
import { argv, env, exit } from "node:process";
import { WASI } from "node:wasi";

const [file, ...args] = argv.slice(2);
const wasi = new WASI({ version: "preview1", args: [file, ...args], env, returnOnExit: true });
const module = await WebAssembly.compile(await readFile(file));
const instance = await WebAssembly.instantiate(module, wasi.getImportObject());
exit(wasi.start(instance));
