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
./build.sh                  # WASM (web/src/wasm) + web client (web/dist) + server; puts ~/.cargo/bin first because the Gentoo rustc lacks the wasm target
cd web && npm run dev       # Vite dev server with hot reload; proxies /api to a server on 127.0.0.1:8080
cd web && npm run check     # svelte-check; keep at zero warnings
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
- `web/`: the browser client, Svelte 5 + Vite + Tailwind CSS v4. The server serves the build output in `web/dist`.
  - `src/lib/`: `cloud.svelte.js` holds the session and every server operation (the only place keys are handled), `crypto.js` wraps the WASM module, `kdf.worker.js` runs Argon2 off the main thread, `ui.svelte.js` holds toasts, transfers and the theme.
  - `src/components/`: `Shell.svelte` (logged-in layout), `views/` (one per sidebar section), `dialogs/`, plus shared `Modal`, `Menu`, `Icon`.
  - `src/SharePage.svelte`: the public-link viewer (`share.html`).
  - `src/lib/icons.js` is generated from Lucide by `npm run icons`; add names to `scripts/gen-icons.mjs`.
  - Fonts (Geist, OFL) and the logo live in `web/public/`.
- `assets/`: branding. The logo mark is `thencloud-logo-mark.png`; `branding.txt` has the name, the katakana ゼンクラウド and the accent colour `#3B47F9`.

## Web UI direction

The web client follows this direction; keep it that way when changing it.

**People have recently taken a strong dislike to AI-generated software.** thencloud is built with AI assistance and we are open about that: commits carry a `Co-Authored-By` trailer, and we don't hide or deny it. But the product itself must not *look* AI-generated. The UI should feel like a small team of people with taste designed it by hand.

**Stack:** Svelte 5 (runes) + Vite + Tailwind CSS v4, all compiled at build time into static files in `web/dist`.
- Never load anything from a CDN (no Tailwind Play script, no Google Fonts). The server's CSP (`routes/mod.rs`) allows only same-origin scripts and stylesheets, and that is also what keeps the `#key` fragment safe from outside code. Don't loosen it.
- No inline `<script>` or `style="..."` attributes in markup: the CSP blocks them. Svelte's `style:prop={...}` directive is fine (it sets styles through the CSSOM).
- Colours come from the semantic tokens in `web/src/app.css` (`bg`, `fg`, `line`, `accent`...), which switch for dark mode. Don't use raw palette colours in components.

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
- Unicode characters not often used in websites, `—`, `…`, `½`, `⅓`, etc. A middle dot (`·`) is fine.

**Copy:** short, specific and human. Say what's encrypted and what the server can see in plain words, not buzzwords.

**Brand:** the logo is a playful sticker-style mark. Use it for brand moments (login screen, favicon, empty states, public share page header). Keep the working app chrome restrained around it.

**Security UX must survive the redesign.** Keep:
- Fingerprint display and the fingerprint confirmation before sharing.
- The "no password recovery" warning at registration.
- The explanation that the link key is after `#`.
- Keys held only in memory: no `localStorage` for keys or tokens.

Replace native `prompt()`/`confirm()` dialogs with real modals.
