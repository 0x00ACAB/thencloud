# thencloud CLI

A command-line client that encrypts and decrypts on your machine with the same code as the web client (`thencloud-crypto`). The server only ever gets ciphertext, wrapped keys and name tags.

It signs in with an **app password** (Settings > App passwords in the web client), never your account password. A read-only app password is enough to list, download and mount read-only.

```sh
cargo build --release -p thencloud-cli     # ./target/release/thencloud

thencloud login https://cloud.example.com  # asks for the app password (or set THENCLOUD_APP_PASSWORD)
thencloud ls Photos
thencloud get Photos/cat.jpg
thencloud put report.pdf Documents
thencloud mkdir Documents/2026
thencloud pull Documents ~/Documents-copy  # new and changed files down, nothing deleted
thencloud push ~/Notes Notes               # new and changed files up, nothing deleted
thencloud logout
```

The server and app password are saved in `~/.config/thencloud/config.json`, readable only by you. Anyone who can read that file can read your files, so revoke the app password in Settings if the machine is lost.

## Backups

```sh
thencloud backup "" ~/thencloud.backup     # everything in My files, into one encrypted file
THENCLOUD_BACKUP_KEY=... thencloud restore ~/thencloud.backup "From backup"   # on any server
```

`backup` makes a new backup key for each backup and shows it once, in the same format as a recovery key. Keep it with the file: nothing else opens the backup, and it isn't stored anywhere. `restore` puts the files back into a folder in whatever account you're signed in to, on this server or another one, encrypting them afresh there. Folders already there are reused, and files with the same name get a new version. A damaged or incomplete backup stops the restore at the damage. The format is in [docs/format](../../docs/format/README.md#backups).

## Moving from Nextcloud

`import-nextcloud` copies files from a Nextcloud account over its WebDAV, encrypting them on this machine as they stream through. Folders, names and modification times are kept; nothing is written to disk on the way. Make an app password in Nextcloud (Settings > Security > Devices & sessions) and use it instead of your real one:

```sh
NEXTCLOUD_PASSWORD=... thencloud import-nextcloud https://nc.example.com alice                 # everything, into My files
NEXTCLOUD_PASSWORD=... thencloud import-nextcloud https://nc.example.com alice "From Nextcloud" --from Photos
```

It's safe to stop and run again: files already copied, with the same size and time, are skipped. A file that changes while it's being copied (its size no longer matches the listing) stops the import rather than being saved cut short. Shares, comments, versions and trash on Nextcloud aren't copied, only the current files.

## Mounting as a drive (Linux)

`thencloud mount` shows My files, or one folder, as a normal folder on your machine through FUSE, so Dolphin, Nautilus, editors and the shell can use it directly.

```sh
sudo apt install fuse3          # or: pacman -S fuse3 / dnf install fuse3 / emerge sys-fs/fuse
mkdir -p ~/thencloud
thencloud mount ~/thencloud              # My files
thencloud mount ~/photos Photos          # just one folder
thencloud mount ~/thencloud --read-only
```

It runs until you press Ctrl+C or unmount it with `fusermount3 -u ~/thencloud`. In Dolphin, right-click the folder and choose "Add to Places" to keep it in the sidebar.

How it behaves:
- Files are fetched and decrypted 4 MiB at a time as programs read them, so opening a large video doesn't download all of it.
- Writes are kept in an unlinked temporary file (in `$TMPDIR`, removed as soon as the file is closed or the program exits) and uploaded, encrypted, as a new version when the file is closed. Point `TMPDIR` at a tmpfs such as `/run/user/$UID` if you'd rather never have plaintext on disk.
- Deleting moves things to the trash, like in the web client.
- Saving from an editor (write a temporary file, rename it over the original) makes a new version of the original, so its history and shares stay.
- If a file was changed elsewhere while you had it open, your copy is saved next to it as "name (conflicted copy)" instead of overwriting.
- Names are unique regardless of case, as in the web client, so `Notes.txt` and `notes.txt` can't sit in the same folder.
- Folder listings are cached for a few seconds, so changes made in the browser show up shortly after.
- Permissions, owners and symlinks aren't stored; everything belongs to the user who mounted it.

To mount at login, a systemd user service works well (`~/.config/systemd/user/thencloud.service`):

```ini
[Unit]
Description=thencloud drive
After=network-online.target

[Service]
ExecStartPre=/usr/bin/mkdir -p %h/thencloud
ExecStart=%h/.cargo/bin/thencloud mount %h/thencloud
ExecStopPost=-/usr/bin/fusermount3 -u %h/thencloud
Restart=on-failure

[Install]
WantedBy=default.target
```

```sh
systemctl --user enable --now thencloud
```

## Serving over WebDAV (macOS, Windows and others)

`thencloud serve` decrypts My files, or one folder, on your machine and serves it over WebDAV on `127.0.0.1`, so Finder, Explorer and any other WebDAV client can open it as a drive. The server still gets only ciphertext; the decrypted files never leave this machine.

```sh
thencloud serve                  # My files, on port 4918
thencloud serve Photos --port 8000 --read-only
```

It prints an address like `http://127.0.0.1:4918/3f9c.../`. The long part is a secret made fresh each time: without it the bridge answers nothing, so other users on this machine and web pages that try to reach localhost can't read your files. Don't share it. To keep the same address across restarts (for a mapped drive), set `THENCLOUD_SERVE_SECRET` to 32 or more random letters and digits.

- **macOS**: in Finder, Go > Connect to Server (Cmd+K), paste the address, and connect as a guest.
- **Windows**: in Explorer, right-click This PC > Map network drive, choose "Connect to a Web site..." or paste the address as the folder. Explorer refuses files over 50 MB unless you raise `FileSizeLimitInBytes` under `HKLM\SYSTEM\CurrentControlSet\Services\WebClient\Parameters`.
- **Linux**: `dav://127.0.0.1:4918/<secret>/` in GNOME Files or `webdav://...` in Dolphin (though `thencloud mount` is the better fit there).

How it behaves:
- Files are fetched and decrypted 4 MiB at a time as they're read; seeking in a video fetches only the pieces it needs.
- Uploads are kept in an unlinked temporary file in `$TMPDIR` until the request ends, then encrypted and uploaded. Writing over a file makes a new version of it.
- Deleting moves things to the trash (a folder goes as one item). Moving and renaming keep a file's history and shares.
- It only ever listens on `127.0.0.1`. A read-only app password (or `--read-only`) refuses every change.
- Folder listings are kept until the server's change feed says something in them changed (checked every five seconds), so changes made elsewhere show up shortly after.
- Finder's own files (`.DS_Store`, and `._name` files for extended attributes) are kept in memory while `serve` runs and never uploaded, so they don't clutter your folders elsewhere. They're gone when it stops.

## Checking the web client a server sends

The browser runs whatever JavaScript the server sends, so a compromised server could quietly send a version that leaks keys. Each release of the web client is built reproducibly and comes with a manifest of every file's SHA-256, signed with minisign. `verify-web` fetches every file from the server (plain, gzip and brotli, plus `/` and a share link) and compares:

```sh
# from the release page: thencloud-web-v1.2.0.json and thencloud-web-v1.2.0.json.minisig
thencloud verify-web https://cloud.example.com --manifest thencloud-web-v1.2.0.json --key RWQ...

# or build that release yourself and compare with your own build
git checkout v1.2.0 && scripts/release-web.sh v1.2.0
thencloud verify-web https://cloud.example.com --manifest thencloud-web-v1.2.0.json --unsigned
```

It also checks that the Content-Security-Policy only lets in scripts and connections from the server itself. It needs no account. It shows what the server sends to anyone who asks, so run it from the network you use; it can't catch a server that sends a different page only to your browser.
