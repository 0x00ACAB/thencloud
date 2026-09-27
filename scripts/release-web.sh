#!/usr/bin/env bash
# Reproducible build of the web client, and its release manifest.
#
#   scripts/release-web.sh [version]
#
# Writes thencloud-web-<version>.json (the SHA-256 of every file the server
# serves) and thencloud-web-<version>.tar.gz (web/dist). Anyone building the
# same commit this way gets the same bytes: the Rust toolchain is pinned,
# wasm-bindgen and the npm packages come from the lockfiles, and checkout
# paths are mapped out of the WASM. Maintainers sign the manifest with
# minisign; `thencloud verify-web` checks a server against it.
set -euo pipefail
cd "$(dirname "$0")/.."

RUST_TOOLCHAIN="${RUST_TOOLCHAIN:-1.94.1}"
version="${1:-$(git describe --tags --always --dirty)}"
cargo_home="${CARGO_HOME:-$HOME/.cargo}"
if [ -d "$cargo_home/bin" ]; then
  export PATH="$cargo_home/bin:$PATH"
fi

rustup toolchain install "$RUST_TOOLCHAIN" --profile minimal --target wasm32-unknown-unknown
export RUSTUP_TOOLCHAIN="$RUST_TOOLCHAIN"
# RUSTFLAGS replaces .cargo/config.toml's, so it repeats the getrandom cfg.
export RUSTFLAGS="--cfg getrandom_backend=\"wasm_js\" --remap-path-prefix=$PWD=/thencloud --remap-path-prefix=$cargo_home=/cargo"
export SOURCE_DATE_EPOCH="$(git log -1 --format=%ct)"

wasm-pack build crates/thencloud-wasm --release --target web --out-dir ../../web/src/wasm --no-typescript --no-pack
(
  cd web
  npm ci --no-audit --no-fund
  npm run build
  THENCLOUD_VERSION="$version" node scripts/manifest.mjs "../thencloud-web-$version.json"
)
tar --sort=name --mtime="@$SOURCE_DATE_EPOCH" --owner=0 --group=0 --numeric-owner -C web -cf - dist |
  gzip -n9 > "thencloud-web-$version.tar.gz"

echo
echo "thencloud-web-$version.json  sha256 $(sha256sum "thencloud-web-$version.json" | cut -d' ' -f1)"
echo "Sign it with: minisign -Sm thencloud-web-$version.json"
