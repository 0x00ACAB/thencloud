# thencloud

An open-source, **end-to-end encrypted** alternative to Nextcloud.

Files, folder names and keys are encrypted and decrypted **in your browser**. The server stores ciphertext and wrapped keys. It never receives your password, your keys, your file names or your file contents.

> **Status:** early. The backend is a complete file cloud: accounts, folders, chunked uploads, quotas, sharing and public links, with a web client for all of it. See [MILESTONES.md](MILESTONES.md).

## Quick start

Requirements:
- A [rustup](https://rustup.rs) toolchain with the `wasm32-unknown-unknown` target. `build.sh` adds the target if it's missing.
- `wasm-pack`, installed with `cargo install wasm-pack`.
- Node.js with npm, for building the web client.

```sh
./build.sh --release
./target/release/thencloud-server --bind 127.0.0.1:8080
# open http://127.0.0.1:8080
```

The first account to register becomes the admin.

### Server options

Every flag can also be set as an environment variable.

| Flag | Env | Default |
|---|---|---|
| `--bind` | `THENCLOUD_BIND` | `127.0.0.1:8080` |
| `--data-dir` | `THENCLOUD_DATA_DIR` | `./data` (SQLite DB + encrypted blobs) |
| `--web-dir` | `THENCLOUD_WEB_DIR` | `./web/dist` (the built web client) |
| `--allow-registration` | `THENCLOUD_ALLOW_REGISTRATION` | `true` (the first user can always register) |
| `--default-quota` | `THENCLOUD_DEFAULT_QUOTA` | 10 GiB |
| `--session-days` | `THENCLOUD_SESSION_DAYS` | `30` |
| `--max-versions` | `THENCLOUD_MAX_VERSIONS` | `10` (versions kept per file, including the current one) |
| `--trash-days` | `THENCLOUD_TRASH_DAYS` | `30` (days before trashed items are purged) |
| `--upload-ttl-hours` | `THENCLOUD_UPLOAD_TTL_HOURS` | `24` |

**Serve thencloud over HTTPS in production** (for example, behind a reverse proxy). The crypto protects data at rest on the server, but the page and its WASM must reach the browser intact.

## Layout

```
crates/thencloud-crypto   All cryptography + shared JSON wire types (native & WASM)
crates/thencloud-wasm     wasm-bindgen bindings used by the web client
crates/thencloud-server   axum + SQLite server, local blob store
web/                      browser client: Svelte 5 + Vite + Tailwind CSS
```

## How the encryption works

```
password ──Argon2id──► root ──HKDF──┬─► auth_key  → server (stored only as Argon2id(auth_key))
                                    └─► kek       (never leaves the client)
kek        ──wraps──► master key
master key ──wraps──► X25519 private key, root folder key
folder key ──wraps──► child node keys
node key   ──seals──► metadata {name, mime, size, mtime}
file key   ──wraps──► per-version content key ──seals──► 4 MiB chunks
```

Primitives:
- **AEAD:** XChaCha20-Poly1305.
- **KDF:** Argon2id (64 MiB, t=3) and HKDF-SHA256.
- **Sharing:** X25519 sealed boxes.

Every ciphertext carries associated data that binds it to its context:
- Node keys and metadata are bound to the node id.
- Content keys are bound to the node id and version id.
- Chunks are bound to the version id, chunk index and an is-last flag.

As a result, a malicious server cannot swap files, move ciphertexts between nodes, reorder or truncate chunks, or serve an old version's chunks under a new one. Decryption fails if it tries.

**Sharing with a user:** the owner fetches the recipient's public key, checks its fingerprint (shown in both users' UIs), and seals the folder or file key to it. The recipient opens the sealed key with their private key and can then unwrap the whole subtree.

**Public links** look like `https://host/s/<token>#<key>`. Browsers never send the part after `#` in any HTTP request, so the server only ever sees `<token>`. The share page reads the key from `location.hash` and decrypts locally. Other defences:
- `Referrer-Policy: no-referrer` and a strict same-origin CSP keep the URL from leaking to third parties.
- The optional link password is a separate server-side gate. It is not derived from the key.

### Threat model

**The server can see:**
- Usernames and public keys.
- The shape of the folder tree: which node is inside which, and file vs folder.
- Ciphertext sizes and chunk counts, which approximate file sizes.
- Timestamps.
- Who shares with whom, with what permission, and which nodes have public links.
- IP addresses and access patterns.

**The server cannot see:**
- Passwords, master keys or private keys.
- File and folder keys.
- File and folder names, MIME types, plaintext sizes and modification times.
- File contents.

**Known limitations**, most of them tracked in [MILESTONES.md](MILESTONES.md):
- **The web client is served by the server.** A malicious or compromised server could serve modified JavaScript. This is inherent to every browser-based E2EE app. A native client (planned) avoids it.
- **Public keys are trust-on-first-use.** Compare fingerprints out of band, or a malicious server could substitute its own key when you share.
- **Revoking a share** stops the server from serving the data, but it does not re-key. A former recipient who kept the key could decrypt ciphertext they get from elsewhere.
- **Anyone who has a full public link** (including the `#` part, for example from chat history) can decrypt what it points to.
- **There is no password recovery.** A forgotten password means the data is lost.
- **Previews render files other people shared with you** inside the app, where your keys live. Decrypted bytes are always re-typed to a fixed, known-safe type (never the stored MIME type), Markdown goes through DOMPurify with images and scripts removed, and PDFs are drawn to canvas by pdf.js without running any PDF JavaScript. The CSP is the second line of defence.

## Development

```sh
cargo test --workspace          # crypto unit tests + end-to-end server tests
cd web && npm run dev           # hot-reloading client; proxies /api to a server on :8080 (or $THENCLOUD_API)
cd web && npm run check         # svelte-check
```

`crates/thencloud-server/tests/e2e.rs` drives a real server in-process with a native client built on `thencloud-crypto`. It covers register, upload, move, share, public link, revoke, version history and the trash. It then scans the SQLite database and blob store to check that no plaintext names, contents, passwords or keys were stored.

## AI assistance

Parts of thencloud are written with the help of AI (commits say so in a `Co-Authored-By` trailer). The code is public so anyone can check what it does.

## License

AGPL-3.0-or-later. See [LICENSE](LICENSE).

The web client bundles [Geist](https://vercel.com/font) (OFL), [Lucide](https://lucide.dev) icons (ISC), [highlight.js](https://highlightjs.org) (BSD-3-Clause), [marked](https://marked.js.org) (MIT), [DOMPurify](https://github.com/cure53/DOMPurify) (Apache-2.0 or MPL-2.0) [pdf.js](https://mozilla.github.io/pdf.js/) (Apache-2.0), and the file icon packs [Material Icon Theme](https://github.com/material-extensions/vscode-material-icon-theme) (MIT), [vscode-icons](https://github.com/vscode-icons/vscode-icons) (MIT) and [Seti UI](https://github.com/jesseweed/seti-ui) (MIT). All of it is served from your own server; nothing loads from a CDN.
