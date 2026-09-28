# thencloud milestones

## ✅ Milestone 1: Files core

- [x] Zero-knowledge accounts: Argon2id in the client, split into an `auth_key` (the server stores only its Argon2id hash) and a key-encryption key that never leaves the client
- [x] Master key and X25519 keypair generated in the client, stored on the server only in wrapped form
- [x] Encrypted key tree: every file and folder has its own key, wrapped by its parent's key
- [x] Encrypted metadata: name, MIME type, size and mtime
- [x] Client-chosen UUIDs bound into every ciphertext as AEAD associated data, so the server cannot swap or transplant ciphertexts
- [x] Chunked, resumable uploads with 4 MiB XChaCha20-Poly1305 chunks. Chunks are bound to (version, index, is-last), which blocks reordering, truncation and version mixing
- [x] Folders: create, rename, move (the key is re-wrapped under the new parent), and recursive delete
- [x] New file versions replace the old content atomically
- [x] Optimistic concurrency (`if_revision`, 409 on conflict)
- [x] Per-user quotas, enforced per chunk and released on delete
- [x] Sessions with bearer tokens (stored hashed), logout, and password change (re-wraps the master key and signs out other sessions)
- [x] Username enumeration resistance: fake prelogin salts and a dummy hash check on login
- [x] Brute-force throttling on login, password change and link unlock
- [x] Janitor for expired uploads, sessions and links
- [x] Strict CSP, `Referrer-Policy: no-referrer`, and no third-party scripts
- [x] Web client (Svelte + Tailwind): file browser, drag-and-drop uploads, transfer tray, move/rename dialogs, dark mode; all crypto runs in WASM, Argon2 in a Web Worker

## ✅ Milestone 2: Sharing

- [x] Share a file or folder with another user: the node key is sealed to their X25519 public key
- [x] Read and write permissions. Write lets the recipient create, upload, rename, move and delete *inside* the share. Content created there belongs to, and is billed to, the tree owner
- [x] Change permission, revoke (owner), leave (recipient)
- [x] Public key fingerprints shown in the UI for out-of-band verification
- [x] Public links in the form `/s/<token>#<key>`. The key lives only in the URL fragment and is never sent to the server
- [x] Optional link password (a server-side gate, independent of the key) and optional expiry
- [x] Anonymous link viewer: browse folders, decrypt and download

## ✅ Milestone 3: Versions and trash

- [x] Keep previous file versions, up to `--max-versions` per file (default 10). Each version keeps its own encrypted metadata
- [x] List, download, restore and delete old versions. Restoring keeps the newer version in the history, so it can be undone
- [x] Versions count toward the quota; when it's full, the owner's oldest old versions are pruned first (never current files or the trash)
- [x] Trash bin: deleting marks a node trashed in place, hiding its whole subtree from every route, including shares and public links. Keys stay wrapped under the original parent, so a restore needs no re-wrapping
- [x] Restore to the original folder, or into My files (with the key re-wrapped) if that folder is in the trash too; delete permanently, empty trash, auto-purge after `--trash-days` (default 30)
- [x] Deletes inside a shared folder go to the tree owner's trash; the recipient sees a note that the owner can restore it
- [x] Age-based version thinning: all versions from the last hour, then one per hour for a day, one per day for 30 days and one per week after that (`--version-thinning`, on by default), on top of `--max-versions`

## Milestone 4: Admin and devices (mostly done)

- [x] Admin UI and API: list users, set quotas, make or remove admins, disable (signs out everywhere) or delete users, registration open / invite-only / closed at runtime, single-use invite links (only a hash is stored; the link carries the token after `#`)
- [x] Session and device list: see active sessions (device name, last seen; no IPs kept) and sign them out, one at a time or all others. A signed-out browser drops its keys on its next request
- [x] "Keep me signed in on this browser" (opt-in): token and master key saved in IndexedDB under a non-extractable WebCrypto key, removed on sign-out, "Stop keeping signed in", or when the server ends the session
- [x] App passwords for sync clients: made in the browser and shown once, each wrapping its own copy of the master key; full or read-only; revoking one signs out its sessions; never the account password
- [x] Server stats for admins, without revealing anything encrypted: accounts, sessions, storage, file/folder/version counts, shares and links

## Tools

- [x] **Convert** (download as another format) (file menu): convert in the browser, then download or save the result as a new encrypted file. The server never sees the file
  - Images (PNG, JPEG, WebP, AVIF, BMP) with the browser's own encoders, with a quality setting
  - Video and audio (MP4, WebM, AVI, MKV, MOV, MP3, M4A, Ogg, Opus, FLAC, WAV, GIF from video) with ffmpeg compiled to WASM: 32 MB (10 MB gzipped), self-hosted, loaded only on first use, single-threaded, with progress and Stop. Container-only changes (MOV/MKV/MP4) copy the streams instead of re-encoding
  - Only for signed-in users, not on public links
  - [x] For several selected files at once (Convert in the selection bar): one format for all, images optionally scaled to a longest side, converted one after another; files already in that format are skipped
  - [x] Crop (free or a fixed shape, dragged on the image) and resize for images; trim for video and audio. The same format can be kept, to just edit
- [x] **Video downloader (yt-dlp)**, the one deliberate exception to "no plaintext on the server", so it's explicit and opt-in:
  - **Off by default**; an admin turns it on in the Admin view, and yt-dlp must be installed on the server. Admins only by default, optionally everyone
  - **Streamed, never stored**: the server runs yt-dlp and pipes the video straight to the browser, which encrypts it and uploads it like any other file (or just downloads it). Nothing is written to the server's disk
  - The UI says plainly that the server sees the link and the video while downloading it, and doesn't keep either
  - Limits: one download at a time per user, a size cap, and public addresses only (no fetching from the server's own network)
  - With ffmpeg installed, separate video and audio (most YouTube videos) are fetched at full speed by two yt-dlp processes into FIFOs and copied by ffmpeg into a fragmented MP4 as they stream, H.264 + AAC preferred up to 1080p; without it, only single-file formats work
  - [x] A quality choice (each distinct resolution the site offers, 1080p by default, "best" beyond it), and playlists: listed flat, picked with checkboxes, fetched one video at a time

## UI

Bigger pieces:
- [x] **Motion**: subtle, fast animations for dialogs, menus, toasts, row insert/remove, view changes and transfers. Respects `prefers-reduced-motion`
- [x] **Custom accent colour**: presets or any colour in Settings; shades derived per theme, black or white text picked for contrast
- [x] **File previews**: images, video, audio, PDF (pdf.js) and text, decrypted in the browser; ← and → step through the folder. Also on public links
- [x] **Syntax highlighting** in text and code previews
- [x] **Markdown preview**: rendered and sanitised, with a Source toggle
- [x] **Markdown editor**: WYSIWYG (Milkdown), toolbar and shortcuts, Ctrl+S, saves each change as a new encrypted version with conflict detection; "New note" creates a file and opens it in the editor
- [x] Editor: task lists (tick them in the editor or the rendered view), a table toolbar, plain-text and code files (a monospace editor), and drafts: unsaved edits are kept on the server as you type, encrypted under your master key, and offered back when you edit the file again
- [x] PDF previews: selectable text (pdf.js's text layer) and links (web and mail links open on click; links within the document jump to the page)
- [x] Images in Markdown previews by relative path (`![](img/photo.png)`): found by decrypted name from the file's folder and shown from a blob: URL; nothing is requested by path, and web images still aren't loaded
- [x] Streamed video and audio previews: decrypted piece by piece as they play (and seek), through the stream service worker, with no size limit
- [x] **Per-file-type icons**: choose Minimal (Lucide), Material, Symbols or Documents (document-shaped icons, like a drive) in Settings; served locally, and only the chosen pack's tables and the icons on screen are downloaded
- [x] Optionally per-name folder icons ("Folder icons by name" in Settings, for Material and Symbols: src, images, docs...)
- [x] **Multi-select** with bulk move, download and delete (checkboxes, shift-click ranges, `x`, select all, a floating action bar; one Undo for a bulk trash)
- [x] **Folder uploads** (drag a whole folder in, or Upload > Folder) and **zip downloads** of folders, selections and public folder links, zipped in the browser
- [x] Streamed zips for very large folders: written piece by piece straight to disk, with ZIP64 so there is no 4 GB limit
- [x] **Search and sort** within a folder (search is client-side over decrypted names; sort by name, size or date)
- [x] **Keyboard shortcuts** (`/` search, `j`/`k` or arrows through rows, Backspace up a folder, `n` new folder, `u` upload, Delete to trash) plus a `?` cheat sheet
- [x] Search across all folders ("Everywhere" next to the search box): names are decrypted in the browser, from an in-memory index built as you browse and filled in by walking the tree when you search; results show where each one is and open in place
- [x] **Music player**: pick a folder as the music root; its tree becomes albums by folder (Artist/Album/01 Track.mp3, CD1/CD2 folded in), with cover.jpg-style images as covers. A bar along the bottom with shuffle, repeat, seek, volume and a queue (play next, add to queue), a full-screen player on phones, and media keys through the Media Session API. Tracks stream decrypted through the service worker, and title, artist and cover art are read from ID3, FLAC and MP4 tags in the first piece as it plays. Audio files in My files can be played or queued from their menu
- [x] Music: **playlists** (make, rename, reorder, add albums or tracks) and **editing** a track's title, artist, album and number, or an album's name, artist and cover. Playlists and edits are kept encrypted under your master key; a new cover is uploaded as cover.jpg into the album's folder
- [x] **Videos**: pick a folder as the videos root; episodes are grouped into series by their names (Show S01E02, 1x02), folders (Show/Season 1) or MP4/Matroska tags, and shown as "{Episode Name} S01E02"; everything else is a movie. Posters (poster.jpg, uploaded from the app too), seasons, a full-screen player that streams decrypted, resumes where you stopped and offers the next episode, continue watching, watched marks, editing an episode's details, and renaming a series' files to that format
- [x] **Profile pictures**: shown in the top bar, share dialogs, shared-with lists and verified contacts. Encrypted in the browser under a per-user avatar key that is sealed to each person you share with, either way round; the server can't see them
- [x] Material-style tint: optionally, backgrounds, borders and grey text take the accent's hue at a low chroma (Settings > Accent colour), in light and dark

Smaller things:
- [x] Inline rename on the row instead of a dialog (menu or F2; Enter or clicking away saves, Esc cancels)
- [x] Remember sort order per device
- [x] Remember the transfer tray's collapsed state per device
- [x] Better empty states with the sticker logo (an empty My files and Shared with me)
- [x] Full timestamps on hover everywhere dates are shown
- [x] Skeleton rows instead of a spinner while a folder loads
- [x] Mobile pass: a bottom tab bar, an add button, long-press to select, larger touch targets
- [x] Minify at build: besides the bundles, `sw.js`, `theme-init.js` and every SVG (svgo)
- [x] Optimize assets at build: brotli copies next to the gzip ones (the server prefers them), and pdf.js's no-WebAssembly decoders left out

## Milestone 5: Trust and accounts

- [x] **Verifiable web client**: the web client builds reproducibly (`scripts/release-web.sh`: pinned Rust toolchain, lockfiles, paths mapped out of the WASM), each release gets a manifest of every file's SHA-256 for maintainers to sign with minisign, and `thencloud verify-web` fetches every file in every encoding the server offers (plus `/`, a share link and the CSP) and compares. A release workflow builds on two runner images and fails unless they match byte for byte
  - [(postponed)] A small browser extension that checks what the browser itself was sent
- [x] **Passkeys**: WebAuthn as a second step after the password, and with the PRF extension a way to sign in without it: the browser wraps a copy of the master key under a key from the passkey's PRF output, and the server hands that copy out only after checking the passkey. Settings shows which passkeys can sign in on their own
- [x] **Two-factor codes (TOTP)** as a server-side gate on login, for people without passkeys. Set up with a QR code; only the TOTP secret is stored, never a key, and each code works once. App passwords and the recovery key skip the second step
- [x] **Post-quantum sealing**: every account has an ML-KEM-768 key next to its X25519 one (older accounts get one on their next sign-in), and keys sealed to other users (shares, profile picture keys, file drops) use both, so recorded shares can't be opened later by a quantum computer. Fingerprints and verified contacts cover both keys
- [x] **Link passwords in the key**: optionally derive part of a public link's key from its password, so the server can't skip the password check. Links with a password carry `#p.<secret>`; the node key is wrapped under a KEK from the secret and the password, and visitors unlock with an auth key derived from the password, which never reaches the server
- [x] **One-time and counted links**: a download limit on public links, and "expires after first open". An open is one visit to the link page, counted by the server, which hands that visit a signed token for the rest of it (browsing, downloads, streaming); once they're used up the link is gone for new visitors, while visits under way can finish
- [x] **Delete my account** from Settings, with the password and a typed confirmation. Not from an app password, and the only admin can't leave others without one
- [x] **Account export**: everything decrypted into a zip in the browser (streamed), plus an encrypted backup the CLI can restore to another server. Settings, "Export your data": My files plus library data and contacts as JSON. `thencloud backup <folder> <file>` writes one file under a new backup key shown once; `thencloud restore <file> <folder>` puts it back on any server, encrypted afresh there

## Milestone 6: Everyday files

- [x] **Encrypted thumbnails**: made in the browser on upload (images, video frames, PDF first pages), encrypted under the file's key and stored as a small side blob; a grid view that uses them. A JPEG of up to 256 px from the uploader's own file (a frame a little way into a video, a PDF's first page), sealed under the node key and bound to its version, kept with the version; My files has a list/grid switch
- [x] **Photos**: a timeline of a chosen folder by date taken (EXIF read in the browser, like music tags), with albums and a lightbox. The date is read at upload into the encrypted metadata (`taken`), so the timeline downloads nothing; albums are folders, tiles are the encrypted thumbnails, and photos open in the preview. A photo cleaned of its details keeps no date either, and sorts by its file time
- [x] **Strip location on share**: offer to remove GPS and camera details from photos before they're uploaded or shared. JPEG, PNG and WebP; the orientation is kept. Uploads ask (or always remove, or keep: Settings), and sharing a photo that has a location offers to remove it, deleting the older versions too
- [x] **Favourites and Recent**: kept as node ids in the encrypted app data, shown in the sidebar. Star from a file's menu; files count as recent when previewed or downloaded
- [ ] **Full-text search**: an encrypted index of text, Markdown and PDF contents built in the browser and saved as app data, so search can look inside files without the server learning the words
- [ ] **Groups**: share with a group whose key is sealed to each member; adding someone doesn't mean re-sharing everything
- [x] **Expiry on user shares**, like links have: an expired share gives no access, and the janitor deletes it (and takes back profile picture keys that depended on it)
- [x] **Comments on files**, encrypted under the node key so everyone with access (and only them) can read them. From a file or folder's menu; bound to the node, the comment and its author, so the server can't move one or change who wrote it. Anyone who can open the item can comment; authors and the owner can delete
- [ ] **Files reports**, report a file, an unencrypted copy gets sent to the admin (with an acknowledgement in the reporting process), the admin then may remove the file or mark it as safe
- [ ] **Activity in shared folders**: who added, changed or deleted what, with names decrypted in the browser. The server already sees these events; it doesn't learn the names
- [x] Drag rows onto a folder (or the breadcrumb) to move them
- [x] Paste to upload (Ctrl+V a screenshot or copied files)

## Milestone 7: Clients and self-hosting

- [x] **Installable app (PWA)**: a manifest and icons. No share target: its POST would reach the server with the plaintext files whenever the service worker isn't running
- [ ] **Local WebDAV bridge** in the CLI (`thencloud serve`): serves your files decrypted on 127.0.0.1 only, so macOS Finder, Windows Explorer and iOS Files apps can use them while the server still sees only ciphertext
- [ ] **Import from Nextcloud**: the CLI reads a Nextcloud account over WebDAV, encrypts locally and uploads, keeping folders and dates
- [x] **Container image and release binaries**: a Dockerfile, a compose example with a reverse proxy, and binaries built in CI for each release. The image builds the web client like a release, so `verify-web` passes; `--trust-proxy` keeps rate limits per client behind the proxy
- [x] **Backup and restore**: a server command that takes a consistent snapshot of SQLite and the blob store, and a documented restore (`thencloud-server backup DIR`, safe while running)
- [x] **Health and metrics**: a health check and Prometheus metrics with the same counts the admin view shows, nothing more (`/api/health`; `/api/metrics` only with `--metrics-token`)
- [x] **Integrity check**: the server checks every blob it expects exists with the right size; the client can verify that everything decrypts and flags what doesn't (`thencloud-server check`, and Settings > Check your files)
- [ ] **Translations**: move UI strings into message files and pick the language from the browser
- [ ] **Accessibility pass**: screen reader labels, focus handling in dialogs and menus, and colour contrast checked in both themes and with the tint on

## Milestone 8: Hardening

- [x] **Format spec**: a written description of every ciphertext format, key derivation and wire type, with test vectors that the Rust tests, the WASM build and any other client check against. `docs/format/README.md` and `vectors.json`: every wrapped key, sealed box, metadata and chunk format with its associated data, the KDFs, name tags, fingerprints, padding and the recovery key encoding, plus ciphertexts that must not open (moved, swapped, cut short)
- [ ] **Format versions**: a version byte on every ciphertext and a tested path for moving old data to a new format
- [x] **Fuzzing**: `cargo-fuzz` targets for the crypto decoders and the server's request parsing, and fuzz tests for the untrusted parsers in the browser (`tags.js`, `videotags.js`, the zip and PDF link handling). Also sealed boxes, WebAuthn, and in the browser photos, CSV, subtitles and episode names; in CI on every push and weekly for longer
- [x] **Browser tests**: Playwright in CI for sign-up, upload, share, public links and previews, including a check that no request carries a name, key or plaintext
- [x] **Dependency checks**: `cargo-deny` (advisories and licences) and `npm audit` in CI, on every push and weekly
- [ ] **Master key rotation**: after a suspected leak, re-wrap every key under a new master key and keypair, and re-seal shares
- [ ] **Independent security audit** of the crypto crate, the web client and the server, with the report published

## Milestone 9: Less metadata

- [x] **Coarse timestamps**: the server records created and changed times rounded to the hour (the exact times stay in the encrypted metadata). Nodes, versions and the trash; the metadata's new `changed` field holds the exact time, and items from before fall back to the server's
- [ ] **Hide file vs folder**: store the node type in the encrypted metadata, so the server sees only nodes that have children or content
- [x] **Onion service**: document and test running thencloud as a Tor onion service, so the server doesn't learn clients' IP addresses. `--limit-by-address false` keeps one visitor's wrong guesses from locking everyone out (all arrive from Tor's address); passkeys work on `http://…onion`; the browser tests run through Tor with `THENCLOUD_E2E_URL` and `THENCLOUD_E2E_PROXY`
- [ ] **Uniform upload sizes**: small files uploaded in batches padded to fixed sizes, so upload timing and count give away less

## Milestone 10: More ways to open files

- [x] **Subtitles**: `.srt` and `.vtt` next to a video are decrypted and shown in the player, with a picker (`Film.en.srt` and the like; SRT is turned into WebVTT in the browser). Renaming a series' files takes its subtitles along
- [ ] **Books**: an EPUB and comic (CBZ) reader, with reading progress kept in the encrypted app data
- [x] **Audiobooks and podcasts**: remember the position per file, chapters from MP4/M4B, and playback speed. Tracks over 20 minutes (and any .m4b) pick up where they were left, on any device (kept in the encrypted music data); chapters come from the Nero `chpl` box, which ffmpeg and most audiobook tools write
- [x] **Tables**: CSV and TSV shown as a sortable table instead of plain text, with a Source toggle and editing as text
- [ ] **Office previews**: DOCX, XLSX, ODT and PPTX rendered in the browser, loaded only when needed and sanitised like Markdown
- [ ] **PDF tools**: merge, split, rotate and reorder pages in the browser, saved as a new encrypted file
- [x] **Notes view**: a folder of Markdown files as a notebook, with a list, search and pinned notes. Saved as you type as new versions, with a choice when a note changed elsewhere; search reads notes' text in the browser; pins are kept in the encrypted app data

## Milestone 11: People and organisations

- [ ] **Share with someone who hasn't signed up yet**: an invite link carrying a one-time key; once they register and their fingerprint is checked, the share is re-sealed to their real key
- [ ] **Team spaces**: folders owned by a group rather than a person, with their own quota, so work doesn't disappear when someone leaves
- [ ] **Single sign-on (OIDC)** as a gate on login for organisations; the encryption password or passkey stays separate, since the identity provider must never hold keys
- [ ] **Federation**: share with `user@other-server`, with public keys fetched and pinned like local ones (and checked against key transparency once that exists)
- [ ] **Per-user limits** on bandwidth and upload rate, set by admins

## Milestone 12: Sync and scale

- [ ] **Change feed**: `GET /api/changes?since=<cursor>` returns what changed in your trees and shares, so sync clients and the mount don't have to walk the whole tree
- [ ] **Live updates**: open views refresh when something changes in a shared folder (Server-Sent Events carrying only node ids)
- [ ] **Large folders**: paginated listings on the server and a virtualised file table, so a folder with 100,000 items stays fast
- [ ] **PostgreSQL** as an alternative to SQLite for bigger installs
- [ ] **Several server instances** behind a load balancer, sharing PostgreSQL and the S3 blob store

## Later

- [x] **Recovery key**: an optional printable key that also wraps the master key; "Forgot your password?" uses it to set a new one without losing data
- [ ] **Key rotation on revocation**: re-key a folder subtree when a share is revoked, so former recipients can't decrypt future content
- [x] **Verified contacts**: keys checked by fingerprint are pinned in an encrypted contact list; a changed key blocks sharing until it's checked again
- [ ] **Key transparency**: a signed or auditable key directory, so even a first share doesn't depend on comparing fingerprints
- [x] **Upload-only "file drop" links**: the link carries the owner's public key after `#`; visitors encrypt each file and seal its key to the owner (bound to the file and folder ids), and see nothing in the folder. Dropped files stay hidden until the owner's client wraps their keys under the folder key
- [x] **Streaming downloads** through a service worker, so large files aren't buffered in memory. The page decrypts; the worker only relays pieces and never sees a key
- [x] **Native CLI** reusing `thencloud-crypto` (`crates/thencloud-cli`): sign in with an app password; `ls`, `get`, `put`, `mkdir`, and one-way `pull`/`push` of folders
- [ ] **Desktop sync client**: two-way sync with conflict handling
- [x] **Encrypted name index**: each node carries a keyed hash of its (lower-cased) name under its folder's key, and the server refuses a second one in the same folder (409 `name_taken`) without learning the names. Uploads keep both as "name (2)", restores and dropped files pick a free name, and older items are tagged the first time their folder is listed
- [x] **Metadata padding**: file contents are padded with zeros to a Padmé bucket (at most about 12% more) before encryption, and encrypted metadata to 128-byte steps, so the server sees only rough sizes. The real size lives in the encrypted metadata
- [ ] **Calendar and contacts**, end-to-end encrypted
- [x] **Linux mount**: `thencloud mount` shows My files (or a folder) as a drive through FUSE, for Dolphin, Nautilus and the shell. Reads are fetched and decrypted a chunk at a time; writes are uploaded as a new version on close; deletes go to the trash; an editor's save-and-rename becomes a new version of the original; conflicting edits are kept as a copy
- [ ] **S3-compatible blob store** behind the existing `BlobStore` interface
- [x] Store the current path after the `#` so that it can persist through reloads
- [ ] **Collaborative editing**: live Markdown editing with others in a shared folder, with every update encrypted under the file's key and the server only relaying them
- [ ] **Mount on macOS and Windows** (FUSE-T, WinFsp), reusing the Linux mount's code
- [ ] **Mobile apps** reusing `thencloud-crypto`, with camera upload
