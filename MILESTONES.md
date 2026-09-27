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
- [ ] Previews: streamed video (today the whole file is decrypted into memory first, up to 256 MB)
- [x] **Per-file-type icons**: choose Minimal (Lucide), Material, Symbols or Documents (document-shaped icons, like a drive) in Settings; served locally, and only the chosen pack's tables and the icons on screen are downloaded
- [x] Optionally per-name folder icons ("Folder icons by name" in Settings, for Material and Symbols: src, images, docs...)
- [x] **Multi-select** with bulk move, download and delete (checkboxes, shift-click ranges, `x`, select all, a floating action bar; one Undo for a bulk trash)
- [x] **Folder uploads** (drag a whole folder in, or Upload > Folder) and **zip downloads** of folders, selections and public folder links, zipped in the browser
- [ ] Streamed zips for very large folders (today a zip is built in memory, up to 4 GB)
- [x] **Search and sort** within a folder (search is client-side over decrypted names; sort by name, size or date)
- [x] **Keyboard shortcuts** (`/` search, `j`/`k` or arrows through rows, Backspace up a folder, `n` new folder, `u` upload, Delete to trash) plus a `?` cheat sheet
- [x] Search across all folders ("Everywhere" next to the search box): names are decrypted in the browser, from an in-memory index built as you browse and filled in by walking the tree when you search; results show where each one is and open in place

- [ ] **Profile pictures**: shown in the top bar, share dialogs and shared-with lists. Decide who can see them: encrypted to people you share with (server can't see) vs. plain on the server (simpler, but visible to it)

Smaller things:
- [x] Inline rename on the row instead of a dialog (menu or F2; Enter or clicking away saves, Esc cancels)
- [x] Remember sort order per device
- [x] Remember the transfer tray's collapsed state per device
- [x] Better empty states with the sticker logo (an empty My files and Shared with me)
- [x] Full timestamps on hover everywhere dates are shown
- [x] Skeleton rows instead of a spinner while a folder loads
- [ ] Mobile pass: bottom action bar, larger touch targets

## Later

- [x] **Recovery key**: an optional printable key that also wraps the master key; "Forgot your password?" uses it to set a new one without losing data
- [ ] **Key rotation on revocation**: re-key a folder subtree when a share is revoked, so former recipients can't decrypt future content
- [x] **Verified contacts**: keys checked by fingerprint are pinned in an encrypted contact list; a changed key blocks sharing until it's checked again
- [ ] **Key transparency**: a signed or auditable key directory, so even a first share doesn't depend on comparing fingerprints
- [x] **Upload-only "file drop" links**: the link carries the owner's public key after `#`; visitors encrypt each file and seal its key to the owner (bound to the file and folder ids), and see nothing in the folder. Dropped files stay hidden until the owner's client wraps their keys under the folder key
- [ ] **Streaming downloads** through a service worker, so large files aren't buffered in memory
- [ ] **Native CLI and desktop sync client** reusing `thencloud-crypto`
- [ ] **Encrypted name index**: HMAC of name under the folder key, so the server can reject duplicate names without learning them
- [ ] **Metadata padding**: pad sizes and chunk counts to hide exact file sizes
- [ ] **Calendar and contacts**, end-to-end encrypted
- [ ] **Linux mount**: Mount the disk (or a subdirectory) as a linux drive
- [ ] **S3-compatible blob store** behind the existing `BlobStore` interface
