#!/usr/bin/env bash
# Build the WASM crypto module for the web client, then the server.
#
#   ./build.sh            # debug server build
#   ./build.sh --release  # release server build
#
# Requires: a rustup toolchain with the wasm32-unknown-unknown target, and
# wasm-pack (`cargo install wasm-pack`).
set -euo pipefail
cd "$(dirname "$0")"

# Prefer rustup's toolchain (which has the wasm target) over a distro rustc.
if [ -d "$HOME/.cargo/bin" ]; then
  export PATH="$HOME/.cargo/bin:$PATH"
fi

if ! rustup target list --installed 2>/dev/null | grep -q wasm32-unknown-unknown; then
  echo "Installing wasm32-unknown-unknown target..."
  rustup target add wasm32-unknown-unknown
fi
if ! command -v wasm-pack >/dev/null; then
  echo "wasm-pack not found: install it with 'cargo install wasm-pack'" >&2
  exit 1
fi

wasm-pack build crates/thencloud-wasm --release --target web --out-dir ../../web/pkg --no-typescript --no-pack
cargo build -p thencloud-server "$@"

echo
echo "Done. Run the server with:"
echo "  cargo run -p thencloud-server $* -- --bind 127.0.0.1:8080"
