# CLAUDE.md

Guidance for Claude Code (and other AI assistants) working in this repo.

## What this is

thencloud is an open-source, end-to-end encrypted alternative to Nextcloud. Read `README.md` for the crypto design and threat model, and `MILESTONES.md` for what's done and what's next.

## The one rule that must never break

**No key, password or plaintext may ever reach the server.**
- All encryption and decryption happens in `crates/thencloud-crypto`, which runs natively and in the browser via WASM (`crates/thencloud-wasm`).
- The server only stores and serves ciphertext, wrapped keys and public keys.
- Public links are `/s/<token>#<key>`. The key lives only in the URL fragment and must never be put into a path, query string, header, request body or log.
- New ciphertext formats must bind their context (node id, version id, etc.) as AEAD associated data, like the existing ones in `thencloud-crypto/src/lib.rs`.
- Any new server feature must be added to the zero-knowledge scan in `crates/thencloud-server/tests/e2e.rs`, which checks the DB and blob store for plaintext.

## Commands

```sh
./build.sh                  # WASM (web/pkg) + server; puts ~/.cargo/bin first because the Gentoo rustc lacks the wasm target
cargo test --workspace      # crypto unit tests + in-process end-to-end server tests
cargo clippy --workspace --all-targets   # keep at zero warnings
cargo fmt --all
cargo run -p thencloud-server -- --bind 127.0.0.1:8080 --data-dir ./data
```

## Layout

- `crates/thencloud-crypto`: key derivation, key wrapping, sealed boxes, metadata and chunk encryption. `api.rs` holds the JSON wire types shared by the server and clients.
- `crates/thencloud-wasm`: thin `wasm-bindgen` wrappers. JS does networking only, never crypto.
- `crates/thencloud-server`: axum + SQLite (sqlx, migrations in `migrations/`) and a local blob store.
  - `access.rs` is the single place authorisation is decided (owner, or a share on any ancestor). Every route goes through it.
- `web/`: the browser client. `common.js` has shared helpers, `app.js` is the logged-in app, `share.js` is the public-link viewer.
- `assets/`: branding. The logo mark is `thencloud-logo-mark.png`; `branding.txt` has the name, the katakana ゼンクラウド and the accent colour `#3B47F9`.

## Web UI direction

The current `web/` client is a deliberately unstyled proof of concept. The next step is a real UI, and there's an important constraint on how it should feel.

**People have recently taken a strong dislike to AI-generated software.** thencloud is built with AI assistance and we are open about that: commits carry a `Co-Authored-By` trailer, and we don't hide or deny it. But the product itself must not *look* AI-generated. The UI should feel like a small team of people with taste designed it by hand.

**Stack:** Tailwind CSS, compiled at build time with the standalone Tailwind CLI (or npm), into a static CSS file served from `web/`.
- Do **not** use the Tailwind Play CDN script. The server's CSP (`routes/mod.rs`) forbids third-party scripts, and it's also what keeps the `#key` fragment safe from outside code.
- When adding the stylesheet, change `style-src 'none'` to `style-src 'self'` in the CSP. Don't loosen anything else.
- Self-host fonts too; no Google Fonts or other CDNs.

**Look:** in the spirit of vercel.com / the Vercel dashboard, Linear and similar developer tools:
- A neutral black/white/zinc palette with a proper dark mode. The brand accent `#3B47F9` is used sparingly: primary buttons, focus rings, links and the selected state.
- Crisp 1px borders, subtle radius (6–8px), and restrained or no shadows.
- A clean sans (Geist or Inter, self-hosted) with a monospace face for key fingerprints and IDs.
- Dense but breathable file tables, a quiet top bar, clear empty states and keyboard-friendly interactions.

**Avoid the tell-tale "AI template" look:**
- Purple-to-blue gradients, glassmorphism and glowing blobs.
- Emoji used as icons; use a real icon set such as Lucide, as inline SVG.
- Oversized hero sections with vague marketing copy ("Unleash the power of…").
- Everything centred in cards, and uniform `rounded-2xl shadow-xl` on every element.
- Generic stock-illustration vibes and filler text.

**Copy:** short, specific and human. Say what's encrypted and what the server can see in plain words, not buzzwords.

**Brand:** the logo is a playful sticker-style mark. Use it for brand moments (login screen, favicon, empty states, public share page header). Keep the working app chrome restrained around it.

**Security UX must survive the redesign.** Keep:
- Fingerprint display and the fingerprint confirmation before sharing.
- The "no password recovery" warning at registration.
- The explanation that the link key is after `#`.
- Keys held only in memory: no `localStorage` for keys or tokens.

Replace native `prompt()`/`confirm()` dialogs with real modals.
