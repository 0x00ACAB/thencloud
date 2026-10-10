# Drive: FUSE and WebDAV

The CLI can show your files as a folder on your computer, decrypted on your machine. Use `mount` on Linux, and `serve` (WebDAV) for macOS Finder, Windows Explorer and anything else that speaks WebDAV.

## Linux: `thencloud mount`

```sh
sudo apt install fuse3          # or: pacman -S fuse3 / dnf install fuse3 / emerge sys-fs/fuse
mkdir -p ~/thencloud
thencloud mount ~/thencloud              # My files
thencloud mount ~/photos Photos          # just one folder
thencloud mount ~/thencloud --read-only
```

It runs until you press Ctrl+C or unmount with `fusermount3 -u ~/thencloud`. `--allow-other` lets other users on the machine in (needs `user_allow_other` in `/etc/fuse.conf`). In Dolphin, right-click the folder and choose "Add to Places" to keep it in the sidebar.

How it behaves:

- Files are fetched and decrypted 4 MiB at a time as programs read them, so opening a large video doesn't download all of it.
- Writes go to an unlinked temporary file in `$TMPDIR` (removed as soon as the file is closed or the program exits) and are uploaded, encrypted, as a new version when the file is closed. Point `TMPDIR` at a tmpfs such as `/run/user/$UID` if you'd rather never have plaintext on disk.
- Deleting moves things to the trash.
- Saving from an editor (write a temporary file, rename it over the original) makes a new version of the original, so its history and shares stay.
- Folder listings are kept until the server's change feed says they changed.

## Finder and Explorer: `thencloud serve`

```sh
thencloud serve                 # My files, on 127.0.0.1:4918
thencloud serve Photos --read-only
```

It prints an address like `http://127.0.0.1:4918/<secret>/`. Connect to it:

- **macOS:** Finder > Go > Connect to Server, paste the address.
- **Windows:** Explorer > This PC > Map network drive, paste the address.
- **Linux:** most file managers take `dav://127.0.0.1:4918/<secret>/`.

It only listens on 127.0.0.1, every path is under the random secret, and requests for other host names are refused, so web pages you visit can't reach it. The secret is new each time; set `--secret` (or `THENCLOUD_SERVE_SECRET`, at least 32 characters) to keep a mapped drive working across restarts. `--port` changes the port.

Like the mount, it reads a chunk at a time, stages writes in an unlinked temporary file and uploads them on close, and moves deleted folders to the trash whole. Finder's `.DS_Store` and `._*` files are kept in memory only and never uploaded.
