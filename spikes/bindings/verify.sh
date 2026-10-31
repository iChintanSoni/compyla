#!/usr/bin/env bash
# Toolchain check 3 (docs/roadmap.md): one Rust core, built unchanged for the Node, Python and WASM
# hosts, then loaded and called from each host. Every host must return the core's exact output.
# Run from spikes/: bash bindings/verify.sh
set -euo pipefail
cd "$(dirname "$0")/.."
TARGET="${CARGO_TARGET_DIR:-target}"
INPUT='# Hello Compyla

Some *text*.'
EXPECTED='{"blocks":2,"firstHeading":"Hello Compyla"}'
OUT="$TARGET/bindings-check"
rm -rf "$OUT" && mkdir -p "$OUT"
OUT="$(cd "$OUT" && pwd)"

check() {
  local host="$1" got="$2"
  if [[ "$got" != "$EXPECTED" ]]; then
    echo "FAIL $host: got $got, expected $EXPECTED" >&2
    exit 1
  fi
  echo "ok   $host: $got"
}

cargo test -q -p spike-core

# Node (napi-rs): the cdylib is a Node addon once named *.node.
cargo build -q --release -p spike-node
case "$(uname -s)" in
  Darwin) cp "$TARGET"/release/libspike_node.dylib "$OUT/spike.node" ;;
  Linux) cp "$TARGET"/release/libspike_node.so "$OUT/spike.node" ;;
  *) cp "$TARGET"/release/spike_node.dll "$OUT/spike.node" ;;
esac
check node "$(INPUT="$INPUT" node -e 'process.stdout.write(require(process.argv[1]).summarize(process.env.INPUT))' "$OUT/spike.node")"

# Python (PyO3 + maturin): build a wheel, install it into a throwaway environment, import it.
uvx maturin build -q --release -m bindings/python/Cargo.toml -o "$OUT/wheels"
WHEEL="$(ls "$OUT"/wheels/*.whl)"
check python "$(INPUT="$INPUT" uv run -q --no-project --isolated --with "$WHEEL" python -I -c 'import os, sys, spike_python; sys.stdout.write(spike_python.summarize(os.environ["INPUT"]))')"

# WASM (wasm-bindgen): build for wasm32, generate bindings with wasm-pack, load in Node.
npx -y wasm-pack@0.13.1 build --release --target nodejs --out-dir "$OUT/wasm" bindings/wasm >/dev/null 2>&1 \
  || npx -y wasm-pack@0.13.1 build --release --target nodejs --out-dir "$OUT/wasm" bindings/wasm
check wasm "$(INPUT="$INPUT" node -e 'process.stdout.write(require(process.argv[1]).summarize(process.env.INPUT))' "$OUT/wasm/spike_wasm.js")"
ls -l "$OUT/wasm/spike_wasm_bg.wasm" | awk '{print "     wasm size: " $5 " bytes"}'
