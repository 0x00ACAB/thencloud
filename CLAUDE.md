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
- Any new server feature must be added to the zero-knowledge scan in `crates/thencloud-server/tests/e2e.rs`, which checks the DB and blob store for plaintext. New browser flows belong in `web/e2e/`, whose `watchRequests` fails a test if any request carries a name, contents, a password or a key.
- New parsers of untrusted files go in `web/tests/` (browser) or `fuzz/` (Rust) with a fuzz test.
- The one deliberate exception is the video downloader (`src/downloader.rs`, `routes/tools.rs`): opt-in by an admin (off by default), and the UI says the server sees the link and the video. It must stay stream-only: pipes and FIFOs, no files written (a scratch dir, holding only the FIFOs, that is deleted), no caches, nothing logged, site extractors only, private addresses refused. Don't let yt-dlp merge to stdout itself: it then downloads through ffmpeg's HTTP client, which YouTube throttles to about playback speed. Its tests use a fake yt-dlp and ffmpeg.

## Commands

```sh
./build.sh                  # WASM (web/src/wasm) + web client (web/dist) + server; puts ~/.cargo/bin first because the Gentoo rustc lacks the wasm target
cd web && npm run dev       # Vite dev server with hot reload; proxies /api to a server on 127.0.0.1:8080 (override with THENCLOUD_API=http://host:port)
cd web && npm run check     # svelte-check; keep at zero warnings
cd web && npm test          # fuzz tests of the browser's parsers (node --test; FUZZ_RUNS, FUZZ_SEED to replay)
cd web && npx playwright test   # browser tests against a fresh server (see web/e2e/README.md; runs in Docker too)
cd fuzz && cargo +nightly fuzz run <target>   # cargo-fuzz: crypto-decode, sealed-box, webauthn-register, webauthn-assert, api-json, server-input
cargo test --workspace      # crypto unit tests + in-process end-to-end server tests
cargo clippy --workspace --all-targets   # keep at zero warnings
cargo fmt --all
cargo run -p thencloud-server -- --bind 127.0.0.1:8080 --data-dir ./data
```

## Layout

- `crates/thencloud-crypto`: key derivation, key wrapping, sealed boxes, metadata and chunk encryption. `api.rs` holds the JSON wire types shared by the server and clients.
  - Sealed boxes are hybrid X25519 + ML-KEM-768 when the recipient has an ML-KEM key (`KeyPair::sealing_key` is `x25519 || mlkem`); plain X25519 otherwise, and `open_sealed` takes both. Fingerprints and verified-contact pins cover `identity()`: the X25519 key plus the ML-KEM key's hash. In the web client `sk` is the X25519 secret followed by the ML-KEM seed.
- `crates/thencloud-wasm`: thin `wasm-bindgen` wrappers. JS does networking only, never crypto.
- `crates/thencloud-server`: axum + SQLite (sqlx, migrations in `migrations/`) and a local blob store.
  - `access.rs` is the single place authorisation is decided (owner, or a share on any ancestor). Every route goes through it, and it hides anything in the trash (a trashed node or any trashed ancestor); only `routes/trash.rs` reaches trashed nodes, checking ownership itself.
  - Two-step sign-in (`routes/two_factor.rs`, `routes/passkeys.rs`, `totp.rs`, `webauthn.rs`): with a TOTP secret or any passkey, `login` returns a ticket (`auth_challenges`, hashed) instead of a session, and `/auth/login/second-factor` trades it for one. WebAuthn is checked by hand (client data, authenticator data, COSE keys via `ciborium`, signatures via `ring`); each passkey keeps the host it was made on as its RP id. A passkey with PRF also stores the master key wrapped under its PRF output (`wrap_master_key_passkey`) and can sign in alone (`/auth/passkey/login`, stateless HMAC challenges, user verification required). App passwords and the recovery key skip the second step.
  - `routes/app_passwords.rs`: per-device credentials. A read-only one is enforced in the `AuthUser` extractor (`auth.rs`), which refuses anything but GET/HEAD and logout.
  - `routes/admin.rs` manages accounts and counts, never content; `settings.rs` holds runtime settings (registration mode) that override the command line. Disabled users are filtered out in the `AuthUser` extractor.
  - File drops (upload-only links): `routes/public.rs` takes the uploads, `routes/drops.rs` lets the owner take them in. A dropped node's `enc_key` is sealed to the owner until then, and `access.rs` hides it like a trashed one.
  - `routes/avatars.rs`: encrypted profile pictures; grants (the avatar key sealed to someone) only between people with a share either way. The client grants automatically on sharing and when it lists incoming shares (`grantAvatar` in `cloud.svelte.js`).
  - Name tags (`name_tag` in the crypto crate): every route that creates, renames, moves, restores or adopts a node takes one, and the unique index `nodes_name_tag` turns a duplicate into 409 `name_taken` (`name_conflict` in `error.rs`). A rename or move without a tag leaves the node untagged.
  - `maintenance.rs`: the `backup DIR` (SQLite `VACUUM INTO` plus the blobs it refers to, hard-linked where possible) and `check` subcommands (`main.rs`).
  - `ClientIp` (`auth.rs`) is only for rate limiting and never stored; with `--trust-proxy` it's the last `X-Forwarded-For` entry.
  - `routes/health.rs`: `/api/health` (no sign-in) and `/api/metrics` (Prometheus, the admin view's counts, only with `--metrics-token`).
  - Public links with `max_opens`: `public::info` counts an open and returns a signed visit token (`X-Link-Token`), which every other link route then requires; it also stands for the password. Shares may have `expires_at`: `access.rs` and the share lists ignore expired ones, and the janitor deletes them.
  - Deleting a node only marks it trashed; `delete_subtree` in `routes/nodes.rs` is the one permanent delete (used by the trash and the janitor).
- `crates/thencloud-cli`: the `thencloud` command-line client (ureq, blocking), signed in with an app password. `verify.rs` is `thencloud verify-web`, which checks a server's web client against a release manifest (made by `web/scripts/manifest.mjs` from `scripts/release-web.sh`'s reproducible build; keep that build reproducible: no timestamps, absolute paths or randomness in `web/dist`). `lib.rs` is the client; `mount.rs` is the FUSE filesystem (`thencloud mount`, Linux only, `fuser`): an inode table over node ids, folder listings cached for a few seconds, reads decrypted a chunk at a time, writes staged in an unlinked temp file and uploaded on close (`flush`) as a new version. Renaming a file over another uploads it as a new version of the target (editors save that way). Its tests (`tests/cli.rs`) run a real server on a TCP port and mount with FUSE when `/dev/fuse` and `fusermount3` exist.
- `web/`: the browser client, Svelte 5 + Vite + Tailwind CSS v4. The server serves the build output in `web/dist`.
  - `src/lib/`: `cloud.svelte.js` holds the session and every server operation (the only place keys are handled), `crypto.js` wraps the WASM module, `kdf.worker.js` runs Argon2 off the main thread, `ui.svelte.js` holds toasts, transfers and the theme.
  - `src/components/`: `Shell.svelte` (logged-in layout), `views/` (one per sidebar section), `dialogs/`, plus shared `Modal`, `Menu`, `Icon`.
  - `src/SharePage.svelte`: the public-link viewer (`share.html`).
  - `src/lib/icons.js` is generated from Lucide by `npm run icons`; add names to `scripts/gen-icons.mjs`.
  - `src/lib/motion.js` wraps Svelte transitions with the app's timings and `prefers-reduced-motion`; use it instead of `svelte/transition` directly. Popovers use its `portal` action.
  - Don't leave a `transform` on an element after an animation (use `animation-fill-mode: backwards`): it traps `position: fixed` descendants.
  - Fonts and the logo live in `web/public/`: Console Sans (OFL; WOFF2 made from the OTFs in `assets/fonts/` with `woff2_compress`) is the UI font, with Geist behind it for characters it lacks (arrows, check marks, Cyrillic), and Geist Mono for monospace.
  - File icons: `components/FileIcon.svelte`, with the pack chosen in Settings (`iconPack` in `ui.svelte.js`). "Minimal" is Lucide; the IDE packs come from `scripts/file-icons-plugin.mjs`, which turns each pack into `virtual:file-icons/<pack>` lookup tables (imported only when chosen) and serves the SVGs from `/file-icons/<pack>/`. Folders use `components/FolderIcon.svelte`: the plain Lucide folder, or with "Folder icons by name" on (`folderIcons` in `ui.svelte.js`) the pack's icon for that name, from the same tables. The packs are Material, Symbols and Documents (file-icon-vectors "vivid", whose `<style>` blocks the plugin turns into attributes, since the CSP blocks styles inside SVGs too).
  - File previews: `components/Preview.svelte` plus `components/preview/` (text, Markdown, PDF). A shared file is untrusted input rendered in the app's origin, so:
    - `lib/preview.js` picks the viewer from the name/MIME, but the decrypted bytes are always wrapped in a Blob with a type from its fixed tables, never the stored MIME type.
    - Markdown only ever reaches the DOM through `lib/markdown.js` (marked + DOMPurify). Don't `{@html}` anything else except highlight.js output, which escapes its input.
    - Heavy viewers (`highlight.js`, `markdown.js`, `pdf.js`) are loaded with `import()` so the file list doesn't pay for them. `lib/languages.js` holds the extension map so it can be used without loading highlight.js.
    - pdf.js's fonts, CMaps and decoders are copied to `/pdfjs/` by a plugin in `vite.config.js`. pdf.js 6 calls `Map.prototype.getOrInsertComputed`, which `lib/upsert.js` polyfills in the page and in its worker (`lib/pdf.worker.js`); without it pages render blank in browsers that lack it. PDF links go through `pageLinks` in `lib/pdf.js`, which only lets http(s), mailto and in-document links through.
    - Never let a file's content make the browser fetch a URL. A relative image path would be requested from our own server (the CSP allows `'self'`), and that request leaks a plaintext name. So Markdown images are never loaded from their `src`, neither in the preview (`lib/markdown.js` drops `src` inside DOMPurify's inert document) nor in the editor (`lib/editor.js` renders the image node as a placeholder). The preview may show a relative one by finding the file in the encrypted tree (`lib/relpath.js`, by decrypted name) and decrypting it into a blob: URL; no request ever carries the path.
  - Streaming (`lib/stream.js`, `public/sw.js`): downloads over 16 MB, video and audio previews and zips go through a service worker at `/_stream/<id>`. The worker is stateless and never sees a key: for each request it asks the open pages who serves that id, and the page decrypts each piece (`openFile` in `crypto.js`) and hands it over a MessageChannel. Pieces are transferred, so don't read a buffer after passing it on. Without a worker (hard reload, some private windows) everything falls back to Blobs. Zips (`lib/zip.js`) are our own store-only writer with ZIP64.
  - Music (`views/MusicView.svelte`, `PlayerBar.svelte`, `lib/music.svelte.js`): a folder picked as the music root is walked with `walkTree` (`cloud.svelte.js`) and grouped into albums by folder. Only the root's node id is kept in `localStorage` (`musicRoot`, with the user id); the library, tags and covers live in memory and `unloadMusic` drops them when the signed-in view goes away. Tracks play through `serveFile` like video previews; `lib/tags.js` reads ID3/FLAC/MP4 tags from the first piece before it is handed on. Tags are untrusted: shown as text only, and a cover is kept only when its bytes are a known image format, typed from those. The queue is `$state.raw` and replaced, not mutated, so tracks compare by identity. The player bar sits above the phone tab bar; `--bottom-bar` in `app.css` is the tab bar plus the player, so other fixed things clear both.
  - Library data (`routes/app_data.rs`, `loadAppData`/`saveAppData` in `cloud.svelte.js`): one JSON blob per name (`music`, `videos`, `files`; the server refuses others), encrypted with `encrypt_private_data` under the master key, labelled with the name, saved with `if_revision` and retried once on 409. Music keeps playlists (node ids) and edited track/album details there; edits are layered over tags and names in `info()`, the files aren't changed. Album covers and video posters are real files (`lib/cover.js`: scaled to JPEG and uploaded as `cover.jpg`, `poster.jpg` or `<video>-poster.jpg` next to the media).
  - Videos (`views/VideosView.svelte`, `VideoPlayer.svelte`, `lib/videos.svelte.js`): like Music, a root folder (`videosRoot` in `localStorage`) walked with `walkTree`. `lib/episodes.js` reads series, season and episode from names and folders (`Show S01E02 Title`, `1x02`, `Show/Season 1/02 Title`, and our own `{Episode Name} S01E02` inside the show's folder); `lib/videotags.js` reads MP4 `tvsh`/`tvsn`/`tves`/`©nam` and the Matroska title (untrusted, bounds-checked, text only). What `details()` shows is your edits, else tags already read (`found`), else the name. Watch progress is kept in the same data. "Rename files" pins the current details as edits before renaming, so nothing depends on the new names parsing back.
  - Converting (`lib/convert.js`, `components/dialogs/ConvertDialog.svelte`): images with canvas encoders, video and audio with ffmpeg.wasm (`lib/ffmpeg.js`, loaded on demand). All in the browser; the result is downloaded or uploaded as a new encrypted file.
  - The build (the `thencloud-optimise` plugin in `vite.config.js`) minifies what Vite copies as is (`public/*.js` with Oxc, SVGs with svgo; the file-icon SVGs in `scripts/file-icons-plugin.mjs`) and writes `.br` and `.gz` copies of static files, which the server sends via `precompressed_br()`/`precompressed_gzip()`, with `Vary: Accept-Encoding`. `Cache-Control` is set per path in `routes/mod.rs`: hashed `/assets/` are immutable, pages `no-cache`, the API `no-store`.
  - Navigation (`Shell.svelte`) keeps the section and folder after the `#` (`#/files/<id>`, `#/music/album/<id>`) so reloads land in place. Only opaque ids go there, never names: history may be synced to a browser vendor.
  - Favourites and Recent (`lib/places.svelte.js`, `views/PlacesView.svelte`): node ids in the `files` app data; names are resolved and decrypted when shown.
  - CSV/TSV previews are text with `table` set in `previewKind`, shown by `preview/TableView.svelte` (`lib/csv.js` parses). Subtitles (`lib/subtitles.js`, `SubtitlePicker.svelte`): `.srt`/`.vtt` named after a video, SRT turned into WebVTT, served as `text/vtt` blob: URLs.
  - Photos (`lib/exif.js`): EXIF/XMP/IPTC found and removed in JPEG, PNG and WebP before upload (`checkPhotos` in `FilesView.svelte`, per the `photoDetails` setting) and offered before sharing one (`PhotoLocationNotice.svelte`); only the orientation is kept. Untrusted input: bounds-checked, and anything that doesn't parse is left alone.
  - "Check your files" in Settings is `verifyTree` in `cloud.svelte.js`: it downloads and decrypts everything, node by node, and reports each failure with its path.
  - The PWA manifest and icons are in `public/` (`manifest.webmanifest`, `icons/`). The icons are PNGs with no metadata chunks, so the build stays reproducible. There is no share target: a POST of the shared files would reach the server whenever the service worker isn't running.
  - Text and code editing: `components/preview/TextEditor.svelte` (a textarea). Drafts (`routes/drafts.rs`, `loadDraft`/`storeDraft` in `cloud.svelte.js`) are encrypted with `encrypt_private_data` under the master key, labelled `draft:<node id>`.
  - Markdown editing: `components/preview/MarkdownEditor.svelte` over `lib/editor.js` (Milkdown, loaded on demand). `Preview` gets a `save` prop only when the viewer can write; `saveText` in `cloud.svelte.js` uploads the text as a new version with `if_revision`, so a concurrent change fails with 409 `conflict` instead of being overwritten.
- `Dockerfile` builds the web client like `scripts/release-web.sh` (same toolchain and path mapping), so a container passes `thencloud verify-web`; keep the two in step. `deploy/` has a compose example with Caddy.
- `assets/`: branding. The logo mark is `thencloud-logo-mark.png`; `branding.txt` has the name, the katakana ゼンクラウド and the accent colour `#3B47F9`.

## Web UI direction

The web client follows this direction; keep it that way when changing it.

**People have recently taken a strong dislike to AI-generated software.** thencloud is built with AI assistance and we are open about that: commits carry a `Co-Authored-By` trailer, and we don't hide or deny it. But the product itself must not *look* AI-generated. The UI should feel like a small team of people with taste designed it by hand. No need to put a watermark on every commit / message too.

**Stack:** Svelte 5 (runes) + Vite + Tailwind CSS v4, all compiled at build time into static files in `web/dist`.
- Never load anything from a CDN (no Tailwind Play script, no Google Fonts). The server's CSP (`routes/mod.rs`) allows only same-origin scripts and stylesheets, and that is also what keeps the `#key` fragment safe from outside code. Don't loosen it.
- No inline `<script>` or `style="..."` attributes in markup: the CSP blocks them. Svelte's `style:prop={...}` directive is fine (it sets styles through the CSSOM).
- Colours come from the semantic tokens in `web/src/app.css` (`bg`, `fg`, `line`, `accent`...), which switch for dark mode. Don't use raw palette colours in components. With "Tint the whole theme" on (`tint` in `ui.svelte.js`, the `tinted` class on `<html>`, set early by `public/theme-init.js`), the neutral tokens are derived from the accent's hue with relative `oklch()` colours; add any new neutral token to both blocks.

**Look:** in the spirit of vercel.com / the Vercel dashboard, Linear and similar developer tools:
- A neutral black/white/zinc palette with a proper dark mode. The brand accent `#3B47F9` is used sparingly: primary buttons, focus rings, links and the selected state.
- Crisp 1px borders, subtle radius (6–8px), and restrained or no shadows.
- A clean sans, Console Sans (self-hosted), with a monospace face (Geist Mono) for key fingerprints and IDs.
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
- Fingerprint display and the fingerprint confirmation before sharing, and the verified-contact pin: a changed key must block sharing until it's re-checked.
- The "no password reset" warning at registration (it may point to the optional recovery key, which only the user holds).
- The explanation that the link key is after `#`.
- Keys held only in memory: no `localStorage` for keys or tokens. The one exception is the opt-in "Keep me signed in on this browser" (`lib/remember.js`): the token and master key go in IndexedDB, encrypted with a non-extractable WebCrypto key, and are deleted on sign-out and whenever the server says the session is gone. Keep it opt-in, keep the plain warning next to the checkbox, and never store anything else there.

Replace native `prompt()`/`confirm()` dialogs with real modals.
