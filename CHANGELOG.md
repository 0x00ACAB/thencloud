# Changelog

Each release's section here is also its release notes on GitHub. From 1.0 on,
everything an earlier release wrote keeps working: databases, files, links and
the apps (see "Keeping existing data working" in CLAUDE.md).

## Unreleased

- **Android:** music keeps playing with the screen off or another app open,
  with a notification, lock screen controls and headphone buttons.
- `--public-origin` (`THENCLOUD_PUBLIC_ORIGIN`): the address people open the
  server at. When set, passkeys only work from there.

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
