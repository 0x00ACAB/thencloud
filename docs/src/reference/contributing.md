# Contributing

Bug reports, reviews of the crypto and pull requests are all welcome on [GitHub](https://github.com/0x00ACAB/thencloud).

## The one rule

**No key, password or plaintext may ever reach the server.** All encryption happens in `crates/thencloud-crypto`, which the browser runs as WASM. New ciphertext formats bind their context (node id, version id) as AEAD associated data, and every new server feature is added to the zero-knowledge scan in `crates/thencloud-server/tests/e2e.rs`.

thencloud is in use with real data, so existing data must keep working: migrations only add, ciphertext formats never change in place, stored JSON keeps reading old shapes, and older clients keep working against a newer server.

## Setup

You need a rustup toolchain, `wasm-pack` (`cargo install wasm-pack`) and Node.js 22 or newer.

```sh
./build.sh                  # WASM + web client + server
cargo run -p thencloud-server -- --bind 127.0.0.1:8080 --data-dir ./data
cd web && npm run dev       # hot reload, proxies /api to :8080
```

## Layout

```
crates/thencloud-crypto   all cryptography and the shared JSON wire types (native and WASM)
crates/thencloud-wasm     wasm-bindgen bindings used by the web client
crates/thencloud-server   axum + SQLite server, local or S3 blob store
crates/thencloud-cli      command-line client, FUSE mount and WebDAV bridge
crates/thencloud-app      Tauri app for Linux, Windows and Android
web/                      browser client: Svelte 5 + Vite + Tailwind CSS
docs/format               ciphertext formats and key derivations, with test vectors
docs/src                  this site
```

## Before opening a pull request

```sh
cargo fmt --all
cargo clippy --workspace --all-targets   # zero warnings
cargo test --workspace
cd web && npm run check && npm test      # zero warnings
```

CI runs the same checks. Keep pull requests focused, and describe what the server can see if your change touches the API.

## Roadmap

[MILESTONES.md](https://github.com/0x00ACAB/thencloud/blob/main/MILESTONES.md) is the roadmap, mirrored as GitHub milestones and issues labelled `roadmap`. Comment on an issue before starting something big.

## This site

These pages are in `docs/src`, built with [mdBook](https://rust-lang.github.io/mdBook/):

```sh
cargo install mdbook
mdbook serve docs
```

## License

AGPL-3.0-or-later. By contributing you agree your work is licensed under it.
