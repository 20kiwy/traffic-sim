#!/usr/bin/env bash
# Build the browser engine preview: wasm32 cdylib + wasm-bindgen web glue.
# The glue (egui_app.js / egui_app_bg.wasm) is GENERATED (gitignored) —
# CI regenerates it on every push and deploys the folder to GitHub Pages.
set -euo pipefail
cd "$(dirname "$0")/.."
. "$HOME/.cargo/env" 2>/dev/null || true

rustup target list --installed | grep -q wasm32-unknown-unknown || rustup target add wasm32-unknown-unknown
cargo build --release -p egui-app --target wasm32-unknown-unknown --lib

# wasm-bindgen-cli must match the wasm-bindgen crate version in Cargo.lock
WBV=$(cargo metadata --format-version 1 2>/dev/null | python3 -c \
  "import json,sys; print(next(p['version'] for p in json.load(sys.stdin)['packages'] if p['name']=='wasm-bindgen'))")
WBG="/tmp/wasm-bindgen-${WBV}"
BIN="${WBG}/wasm-bindgen"
if [ ! -x "${BIN}" ]; then
  curl -sL -o /tmp/wbg.tar.gz \
    "https://github.com/wasm-bindgen/wasm-bindgen/releases/download/${WBV}/wasm-bindgen-${WBV}-x86_64-unknown-linux-musl.tar.gz"
  rm -rf "${WBG}" && mkdir -p "${WBG}" && tar -xzf /tmp/wbg.tar.gz -C "${WBG}"
  # the archive wraps the binary in a platform directory
  BIN=$(command find "${WBG}" -type f -name wasm-bindgen | head -n 1)
  chmod +x "${BIN}"
fi
"${BIN}" --target web --out-dir web-preview --out-name egui_app \
  target/wasm32-unknown-unknown/release/egui_app.wasm
echo "web-preview ready (wasm-bindgen ${WBV})"
