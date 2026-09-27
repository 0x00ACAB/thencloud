# Contributing

Thanks for helping. Bug reports, reviews of the crypto and pull requests are all welcome.

## The one rule

**No key, password or plaintext may ever reach the server.** All encryption happens in `crates/thencloud-crypto`, which the browser runs as WASM. New ciphertext formats bind their context (node id, version id...) as AEAD associated data, and every new server feature is added to the zero-knowledge scan in `crates/thencloud-server/tests/e2e.rs`. The README explains the design and the threat model.

## Setup

You need a rustup toolchain, `wasm-pack` (`cargo install wasm-pack`) and Node.js 22 or newer.

```sh
./build.sh                  # WASM + web client + server
cargo run -p thencloud-server -- --bind 127.0.0.1:8080 --data-dir ./data
cd web && npm run dev       # hot reload, proxies /api to :8080
```

## Before opening a pull request

```sh
cargo fmt --all
cargo clippy --workspace --all-targets   # zero warnings
cargo test --workspace
cd web && npm run check                  # zero warnings
```

CI runs the same checks. Keep pull requests focused, and describe what the server can see if your change touches the API.

The web UI follows a deliberate style (neutral palette, restrained accent, no CDN, strict CSP); see "Web UI direction" in [CLAUDE.md](CLAUDE.md).

## Roadmap

[MILESTONES.md](MILESTONES.md) is the roadmap. A workflow turns each section into a GitHub milestone and each open item into an issue labelled `roadmap`; ticking an item closes its issue. Comment on an issue before starting something big.

## AI assistance

Parts of thencloud are written with AI help, and commits say so in a `Co-Authored-By` trailer. That's fine for contributions too, as long as you've read and understood what you submit.

## License

By contributing you agree that your work is licensed under AGPL-3.0-or-later.
