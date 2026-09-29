#!/usr/bin/env bash
# Build the thencloud app (crates/thencloud-app) with the web client bundled.
#
#   scripts/build-app.sh            # this desktop's installers (Linux or Windows)
#   scripts/build-app.sh android    # APK and AAB
#   scripts/build-app.sh dev        # run it, against `npm run dev:app`
#
# Extra arguments go to `tauri build` / `tauri android build`.
# Requires what build.sh does, plus the Tauri CLI
# (`cargo install tauri-cli --version "^2" --locked`) and, for Android, the
# Android SDK and NDK (ANDROID_HOME, NDK_HOME). TAURI overrides how the CLI
# is run (CI's desktop builds use `npx @tauri-apps/cli@2`). Not for Android:
# `android init` writes that command into the Gradle project, and there
# `npm run tauri` needs a package.json.
set -euo pipefail
cd "$(dirname "$0")/.."

if [ -d "$HOME/.cargo/bin" ]; then
  export PATH="$HOME/.cargo/bin:$PATH"
fi

target="${1:-desktop}"
[ $# -gt 0 ] && shift

if [ "$target" != dev ]; then
  wasm-pack build crates/thencloud-wasm --release --target web --out-dir ../../web/src/wasm --no-typescript --no-pack
  (
    cd web
    [ package-lock.json -nt node_modules/.package-lock.json ] && npm ci --no-audit --no-fund
    npm run build:app
  )
fi

tauri() { ${TAURI:-cargo tauri} "$@"; }

cd crates/thencloud-app
case "$target" in
  desktop) tauri build "$@" ;;
  dev) tauri dev "$@" ;;
  android)
    if [ ! -d gen/android ]; then
      tauri android init
      tauri icon ../../web/public/icons/icon-512.png
    fi
    tauri android build "$@"
    ;;
  *) echo "usage: $0 [desktop|android|dev] [args...]" >&2; exit 1 ;;
esac
