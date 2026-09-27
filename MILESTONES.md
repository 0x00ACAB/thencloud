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
- [ ] Age-based version retention (e.g. thin out old versions: hourly, then daily, then weekly)

## Milestone 4: Admin and devices

- [ ] Admin UI and API: list users, set quotas, disable or delete users, open or close registration at runtime, invite links
- [ ] Session and device list: see active sessions (device name, last seen) and revoke them
- [ ] App passwords or per-device credentials for sync clients (scoped, revocable, never the account password)
- [ ] Server stats for admins, without revealing anything encrypted: user count, storage used, blob count

## UI

Bigger pieces:
- [x] **Motion**: subtle, fast animations for dialogs, menus, toasts, row insert/remove, view changes and transfers. Respects `prefers-reduced-motion`
- [x] **Custom accent colour**: presets or any colour in Settings; shades derived per theme, black or white text picked for contrast
- [x] **File previews**: images, video, audio, PDF (pdf.js) and text, decrypted in the browser; ← and → step through the folder. Also on public links
- [x] **Syntax highlighting** in text and code previews
- [x] **Markdown preview**: rendered and sanitised, with a Source toggle
- [x] **Markdown editor**: WYSIWYG (Milkdown), toolbar and shortcuts, Ctrl+S, saves each change as a new encrypted version with conflict detection; "New note" creates a file and opens it in the editor
- [ ] Editor: clickable task-list checkboxes, tables toolbar, autosave drafts, and editing plain-text/code files
- [ ] Previews: streamed video (today the whole file is decrypted into memory first, up to 256 MB), images referenced from Markdown by relative path, PDF text selection and links
- [x] **Per-file-type icons**: choose Minimal (Lucide), Seti, Material or vscode-icons in Settings; served locally, and only the chosen pack's tables and the icons on screen are downloaded
- [ ] More icon packs (Catppuccin: its icons are on npm but its file-name mapping isn't), and optionally per-name folder icons
- [x] **Multi-select** with bulk move, download and delete (checkboxes, shift-click ranges, `x`, select all, a floating action bar; one Undo for a bulk trash)
- [ ] **Folder uploads** (drag a whole folder in) and zip download of folders
- [x] **Search and sort** within a folder (search is client-side over decrypted names; sort by name, size or date)
- [x] **Keyboard shortcuts** (`/` search, `j`/`k` or arrows through rows, Backspace up a folder, `n` new folder, `u` upload, Delete to trash) plus a `?` cheat sheet
- [ ] Search across all folders (needs a client-side index of decrypted names, built as you browse or on demand)

- [ ] **Profile pictures**: shown in the top bar, share dialogs and shared-with lists. Decide who can see them: encrypted to people you share with (server can't see) vs. plain on the server (simpler, but visible to it)

Smaller things:
- [ ] Inline rename on the row instead of a dialog
- [x] Remember sort order per device
- [ ] Remember the transfer tray's collapsed state per device
- [ ] Better empty states with the sticker logo
- [ ] Full timestamps on hover everywhere dates are shown
- [x] Skeleton rows instead of a spinner while a folder loads
- [ ] Mobile pass: bottom action bar, larger touch targets

## Later

- [ ] **Recovery key**: an optional printable key that also wraps the master key, since today a forgotten password means lost data
- [ ] **Key rotation on revocation**: re-key a folder subtree when a share is revoked, so former recipients can't decrypt future content
- [ ] **Contact verification / key transparency**: signed key directory or verified-contact store, so a malicious server can't substitute public keys when a user doesn't compare fingerprints
- [ ] **Upload-only "file drop" links**
- [ ] **Streaming downloads** through a service worker, so large files aren't buffered in memory
- [ ] **Native CLI and desktop sync client** reusing `thencloud-crypto`
- [ ] **Encrypted name index**: HMAC of name under the folder key, so the server can reject duplicate names without learning them
- [ ] **Metadata padding**: pad sizes and chunk counts to hide exact file sizes
- [ ] **Calendar and contacts**, end-to-end encrypted
- [ ] **Linux mount**: Mount the disk (or a subdirectory) as a linux drive
- [ ] **S3-compatible blob store** behind the existing `BlobStore` interface
