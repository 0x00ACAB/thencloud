# Changelog

Each release's section here is also its release notes on GitHub. From 1.0 on,
everything an earlier release wrote keeps working: databases, files, links and
the apps (see "Keeping existing data working" in CLAUDE.md).

## v1.1.0 (2026-10-14)

Everything 1.0 wrote keeps working: no format changed, the new database
migrations only add, and older apps and the CLI work against this server.

### New

- **Linked storage:** link your own Google Drive in Settings, as a mirror (a
  second copy of everything, filled in the background) or as extra space (new
  files can be kept there, on top of your quota), and choose which comes
  first. Only ciphertext goes to Google. Move files or whole folders between
  the server and your Drive ("Where it is kept"); big files move over several
  batches, with progress shown as they go. The storage meter shows the
  server's share and your Drive's. Servers turn it on with
  `--google-client-id` and `--google-client-secret` (tokens are sealed under
  `<data dir>/storage-token-key`), admins can turn linking off, and `check`
  compares each linked account with what the server expects there.
- **Secure Send:** share a password or a short note as a one-time link. It's
  kept as an encrypted file in a "Secure Send" folder in My files.
- **Look inside archives:** `.zip`, `.tar`, `.tar.gz` and `.tgz` files open
  in the preview, by folder, and a single file in them downloads without
  extracting the rest. Read in the browser with bounds-checked readers and
  caps on size and file count.
- **What takes space:** Settings > Account > "See what takes space" shows a
  map of My files by folder or by type, from sizes decrypted in the browser.
  Nothing is downloaded.
- **Duplicate files:** Settings finds files with exactly the same content
  (read and compared in the browser) and offers to move the extra copies to
  the trash.
- **Compare any two versions** of a text file, not only a version with the
  one before it.
- **Tag colours:** give a tag a colour in the Tags view; it shows on every
  file it's on.
- **For admins:** Admin > Server storage shows how full the server's disks are
  (and the bucket, with the new `--s3-capacity`) and which accounts are near
  their quota, and Admin in the sidebar is marked while something needs
  attention. `/api/metrics` also reports the data disk's size and free space.
- **Android:** share files from other apps to thencloud, and send a file from
  thencloud to another app. Files you open are cached encrypted on the device
  and checked against the server, so music and video keep playing on slow
  connections; changed files are refreshed only on Wi-Fi with Data Saver off.
  Music keeps playing with the screen off or another app open, with a
  notification, lock screen controls and headphone buttons.
- **Documentation** at docs.kkaii.xyz: using thencloud, self-hosting, and the
  formats.

### Security

- The web client and the apps refuse to sign in when a server asks for weaker
  password hashing (Argon2 settings or salt) than thencloud allows. A
  malicious server could otherwise have asked for cheap settings and guessed
  the password from what the client sent.
- Markdown previews drop inline SVG and MathML. An SVG image filter in a
  shared `.md` file could make the browser request a path from the server.
- Sending the same piece of an upload many times at once can no longer push
  an account's used space down past what it stores.
- Wrong passwords from one address lock the account for that address only
  (with a higher limit across all addresses), so nobody can keep you out by
  guessing. A sign-in waiting for a second factor ends after three wrong
  codes. Public links of a disabled account stop working until it's enabled
  again. Server errors no longer include internal details.
- Public-link tokens are no longer stored in the database. The server keeps
  their SHA-256 and a copy sealed under `<data dir>/link-token-key`, so a
  database backup or S3 snapshot alone holds no working link. Existing links
  are converted at start and keep working. Back that file up: directory
  backups (`backup DEST`) include it, S3 backups and snapshots don't.
- The video downloader's connections go through a proxy inside the server
  that only connects to public addresses, so a redirect or a changing DNS
  answer can't reach the server's own network. IPv6 addresses that carry an
  IPv4 one are judged by that address.
- Stalled request bodies time out, and pages send a `Permissions-Policy`.

### Self-hosting

- `--hsts` (`THENCLOUD_HSTS`) sends `Strict-Transport-Security`, so browsers
  only reach the server over HTTPS. Off by default; the example Caddy setup
  now sends it.
- `--public-origin` (`THENCLOUD_PUBLIC_ORIGIN`): the address people open the
  server at. When set, passkeys only work from there.
- The server warns at start while registration is open to anyone. The example
  `deploy/compose.yaml` now sets `THENCLOUD_ALLOW_REGISTRATION=false`: if you
  run it and rely on open sign-up, choose it in the Admin view (that setting
  wins) or remove the line. The compose example is hardened further
  (a read-only root, no capabilities beyond binding the port, no new
  privileges).
- Docker: images with the video downloader (yt-dlp, ffmpeg and deno) are
  published next to the default ones, tagged `*-downloader`, or built with
  `--target with-downloader` (`THENCLOUD_TARGET=with-downloader` with the
  compose example). An `edge` image is published from every change to main.

## v1.0.0 (2026-10-05)

The first release. thencloud is a file cloud that encrypts everything on your
device before upload: the server stores ciphertext, wrapped keys and public
keys, and never sees your password, your keys, your file names or their
contents.

### What's in it

- **Files:** folders, chunked and resumable uploads, versions with history
  thinning, a trash, quotas, drag and drop, folder uploads, zip downloads
  (streamed, no 4 GB limit), multi-select, keyboard shortcuts, encrypted
  thumbnails and a grid view.
- **Sharing:** with other people (keys sealed with X25519 and ML-KEM-768, with
  fingerprints to compare and verified contacts), read or write, with expiry;
  public links with the key after `#`, optional passwords that are part of the
  key, open limits and expiry; upload-only file drops.
- **Previews and editing:** images, video and audio (streamed), PDF, text with
  highlighting, Markdown with an editor, Office documents, EPUB and comic
  books, CSV, subtitles, and comparing text versions.
- **Libraries:** music with playlists and tags, videos with series and episodes,
  photos by date taken, notes, favourites, recent files, tags and saved
  searches, and search inside files from an encrypted index.
- **Accounts:** passkeys (also for signing in without a password), two-factor
  codes, app passwords, an optional recovery key, sessions and devices, account
  export, and deleting your account. Admins get accounts, quotas, invites, an
  audit log and server counts, never content.
- **Clients:** the web app (installable), desktop apps for Linux and Windows,
  an Android app, and the `thencloud` command line: a FUSE `mount` on Linux,
  a local WebDAV bridge for Finder and Explorer, encrypted backups, and import
  from Nextcloud.
- **Self-hosting:** one server binary with SQLite and a local or S3-compatible
  blob store, a Docker image, backups, an integrity check, health and
  Prometheus metrics, and a robots.txt that keeps crawlers out.
- **In English, Polish and German.**

### Checking what you run

The web client builds reproducibly, and this release's manifest of every file's
SHA-256 is signed twice: with Sigstore by the release workflow, and with
minisign by a maintainer. `thencloud verify-web https://your.server --manifest
thencloud-web-v1.0.0.json` checks a server against it. Every other file and the
Docker image are signed with Sigstore.

### Not yet

- No independent security audit yet. The formats are written down, with test
  vectors, in `docs/format/README.md`.
- Groups, master key rotation, file reports, collaborative editing, federation
  and a desktop sync client are on the roadmap (MILESTONES.md).
- The apps can't use passkeys or show a server's Cloudflare check, and the
  Windows installers aren't code-signed, so Windows will warn before running
  them.
