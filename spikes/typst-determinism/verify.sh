#!/usr/bin/env bash
# Toolchain check 2 (docs/roadmap.md): Typst in-process from Rust gives byte-identical PDFs across
# separate runs, inside the sandbox markdown-spec.md §12.2 requires. Run from spikes/.
set -euo pipefail
cd "$(dirname "$0")/.."
TARGET="${CARGO_TARGET_DIR:-target}"
OUT="$TARGET/typst-check"
rm -rf "$OUT" && mkdir -p "$OUT"
OUT="$(cd "$OUT" && pwd)"
cargo build -q --release -p typst-determinism
BIN="$TARGET/release/typst-determinism"
[[ -x "$BIN" ]] || BIN="$BIN.exe"

first="$("$BIN" "$OUT/first.pdf")"
second="$("$BIN" "$OUT/second.pdf")"
if [[ "$first" != "$second" ]] || ! cmp -s "$OUT/first.pdf" "$OUT/second.pdf"; then
  echo "FAIL separate processes produced different PDFs: $first vs $second" >&2
  exit 1
fi
echo "ok   separate processes produced identical PDFs (sha256 $first)"

# The hostile document text must appear literally (it was data, not markup), and the document must
# have real structure: an outline with both headings and the expected metadata.
uv run -q --no-project --isolated --with pypdf==6.1.1 python -I - "$OUT/first.pdf" <<'PY'
import sys
from pypdf import PdfReader
reader = PdfReader(sys.argv[1])
text = " ".join(page.extract_text() for page in reader.pages)
hostile = ']#read("/etc/passwd")#[ and #import "@preview/cetz:0.3.0": *'
assert hostile in text, f"hostile text not rendered literally: {text[:300]!r}"
print("ok   hostile input rendered as literal text")
titles = [item.title for item in reader.outline]
assert titles == ["Heading One", "Heading Two"], titles
print(f"ok   outline: {titles}")
meta = reader.metadata
assert meta.title == "Determinism check" and meta.author == "Compyla", meta
print(f"ok   metadata: title={meta.title!r} author={meta.author!r}; pages={len(reader.pages)}")
PY
