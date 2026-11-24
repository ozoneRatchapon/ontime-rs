#!/usr/bin/env bash
# Builds the browser bundle for the timer engine into pkg/.
#
# wasm-pack is not used: the Homebrew/cargo binary is the wrong architecture on
# Apple Silicon here, so this drives cargo + wasm-bindgen + wasm-opt directly.
#
# wasm-bindgen the CLI and wasm-bindgen the crate must be the *exact* same
# version or the schema check fails — keep Cargo.toml in step with:
#   wasm-bindgen --version
set -euo pipefail

cd "$(dirname "$0")"

# wherever cargo puts it: a shared CARGO_TARGET_DIR here, the crate's own target/ on CI
TARGET_DIR="$(cargo metadata --format-version 1 --no-deps | node -e 'let s="";process.stdin.on("data",d=>s+=d).on("end",()=>process.stdout.write(JSON.parse(s).target_directory))')"
WASM="$TARGET_DIR/wasm32-unknown-unknown/release/ontime_wasm.wasm"

echo "==> cargo build"
cargo build --release --target wasm32-unknown-unknown

echo "==> wasm-bindgen"
mkdir -p pkg
wasm-bindgen --target web --out-dir pkg --out-name ontime_timer "$WASM"

if command -v wasm-opt >/dev/null 2>&1; then
  echo "==> wasm-opt -Oz"
  wasm-opt -Oz --enable-bulk-memory --enable-nontrapping-float-to-int \
    -o pkg/ontime_timer_bg.wasm.opt pkg/ontime_timer_bg.wasm
  mv pkg/ontime_timer_bg.wasm.opt pkg/ontime_timer_bg.wasm
else
  echo "==> wasm-opt not found, skipping size pass"
fi

# the client resolves this package through a vite alias, so it needs a manifest
cat > pkg/package.json <<'JSON'
{
  "name": "ontime-timer-engine",
  "version": "0.1.0",
  "type": "module",
  "main": "ontime_timer.js",
  "types": "ontime_timer.d.ts",
  "sideEffects": false
}
JSON

size=$(wc -c < pkg/ontime_timer_bg.wasm | tr -d ' ')
gzipped=$(gzip -c pkg/ontime_timer_bg.wasm | wc -c | tr -d ' ')
echo "==> pkg/ontime_timer_bg.wasm  ${size} bytes (${gzipped} gzipped)"
