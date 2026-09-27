#!/usr/bin/env bash
# Build the web client (Rust crypto -> WASM, then Svelte + Tailwind via Vite)
# and the server.
#
#   ./build.sh            # debug server build
#   ./build.sh --release  # release server build
#
# Requires: a rustup toolchain with the wasm32-unknown-unknown target,
# wasm-pack (`cargo install wasm-pack`) and Node.js with npm.
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
for tool in wasm-pack npm; do
  if ! command -v "$tool" >/dev/null; then
    echo "$tool not found (wasm-pack: 'cargo install wasm-pack'; npm: install Node.js)" >&2
    exit 1
  fi
done

wasm-pack build crates/thencloud-wasm --release --target web --out-dir ../../web/src/wasm --no-typescript --no-pack

(
  cd web
  # Reinstall when the lockfile changed since the last install (or there was none).
  [ package-lock.json -nt node_modules/.package-lock.json ] && npm ci --no-audit --no-fund
  npm run build
)

cargo build -p thencloud-server "$@"

echo
echo "Done. Run the server with:"
echo "  cargo run -p thencloud-server $* -- --bind 127.0.0.1:8080"
