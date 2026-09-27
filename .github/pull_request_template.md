## What and why

## Checklist

- [ ] No key, password or plaintext reaches the server (and new server features are covered by the zero-knowledge scan in `crates/thencloud-server/tests/e2e.rs`)
- [ ] `cargo fmt`, `cargo clippy --workspace --all-targets` and `cargo test --workspace` pass
- [ ] `cd web && npm run check` has zero warnings
- [ ] `MILESTONES.md` is updated if this finishes an item
