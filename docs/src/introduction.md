<img src="logo.png" alt="" width="88" class="tc-logo">

# thencloud

thencloud is an open-source file cloud, like Nextcloud, except that everything is encrypted on your device before it's uploaded. The server stores ciphertext, wrapped keys and public keys. It never receives your password, your keys, your file names or what's in your files.

It's one Rust binary with SQLite and a folder of encrypted blobs (or an S3-compatible bucket), a web client, desktop and Android apps, and a command line that can mount your files as a drive.

## Where to start

- **You have an account on someone's server:** [Getting started](use/getting-started.md).
- **You want to run a server:** [Installing](host/install.md), then [Configuration](host/configuration.md).
- **You want to know what the server can see:** [Threat model](security/threat-model.md).
- **You're writing a client, or checking ours:** [Formats and test vectors](reference/format.md).

## What it does

- **Files:** folders, drag-and-drop and folder uploads, resumable uploads in 4 MiB pieces, zip downloads, versions, trash, quotas, tags and favourites.
- **Sharing:** with other people (read or write) after comparing key fingerprints, or by public link, where the key sits after the `#` and never reaches the server. Links can have a password, an expiry and a limit on opens. Upload-only file drops too.
- **Previews and editing:** images, video, audio, PDF, code, Markdown (with an editor), Office documents, EPUB and comics, all decrypted in the browser.
- **Libraries:** pick a folder and get a music player, a video library with series and episodes, a photo timeline, notes, or audiobooks with chapters.
- **Accounts:** an authenticator app or passkeys as a second step, an optional recovery key, app passwords for other devices, and an admin view that counts things but can't read them.

## Google Drive

On a server that turns it on, you can link your own Google Drive in Settings, as a second copy of your files or as extra space. thencloud then keeps encrypted pieces of your files in a "thencloud" folder in your Drive. It asks only for access to the files it creates there (the `drive.file` permission), never the rest of your Drive, and Google sees only encrypted data under random names. See [Linked storage](host/google-drive.md).

The server at cloud.kkaii.xyz has its own [privacy policy](legal/privacy.md) and [terms of service](legal/terms.md).

## The one rule

No key, password or plaintext ever reaches the server. Every feature is built around that, and the test suite checks it: after each end-to-end run it scans the database and blob store for plaintext, and the browser tests fail if any request carries a name, contents, a password or a key.

thencloud is 1.0 and in use with real data.
