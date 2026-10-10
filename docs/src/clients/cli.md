# Command line

`thencloud` is a command-line client that encrypts and decrypts on your machine with the same code as the web client. Release binaries are on the [releases page](https://github.com/0x00ACAB/thencloud/releases) for Linux (x86_64, arm64) and macOS (arm64), or build it:

```sh
cargo build --release -p thencloud-cli     # ./target/release/thencloud
```

## Signing in

The CLI signs in with an **app password** (Settings > App passwords in the web client), never your account password. A read-only one is enough to list, download and mount read only.

```sh
thencloud login https://cloud.example.com  # asks for the app password, or set THENCLOUD_APP_PASSWORD
thencloud logout
```

The server and app password are saved in `~/.config/thencloud/config.json`, readable only by you. Anyone who can read that file can read your files, so revoke the app password in Settings if the machine is lost.

## Commands

Paths are inside My files; `""` or `/` is My files itself.

```sh
thencloud ls Photos
thencloud get Photos/cat.jpg
thencloud put report.pdf Documents          # a new version if the name exists
thencloud mkdir Documents/2026
thencloud pull Documents ~/Documents-copy   # new and changed files down, nothing deleted
thencloud push ~/Notes Notes                # new and changed files up, nothing deleted
```

| Command | What it does |
|---|---|
| `login SERVER` / `logout` | Save or forget the server and app password |
| `ls`, `get`, `put`, `mkdir` | List, download, upload, make a folder |
| `pull`, `push` | One-way sync of new and changed files |
| `backup`, `restore` | Encrypted backups you can restore to any server (below) |
| `mount`, `serve` | Your files as a drive ([Drive: FUSE and WebDAV](drive.md)) |
| `import-nextcloud` | Copy files from Nextcloud ([Moving from Nextcloud](nextcloud.md)) |
| `verify-web` | Check a server's web client against a signed release ([Checking a release](../security/verify.md)) |

`thencloud <command> --help` lists every option.

## Backups

```sh
thencloud backup "" ~/thencloud.backup                                       # everything in My files, into one encrypted file
THENCLOUD_BACKUP_KEY=... thencloud restore ~/thencloud.backup "From backup"   # on any server
```

`backup` makes a new backup key for each backup and shows it once, in the same format as a recovery key. Keep it with the file: nothing else opens the backup, and it isn't stored anywhere.

`restore` puts the files into a folder in whatever account you're signed in to, on this server or another, encrypting them afresh there. Folders already there are reused, and files with the same name get a new version. A damaged or incomplete backup stops the restore at the damage.

The file format is in [Formats](../reference/format.md#backups).

## Tor

The CLI knows nothing about Tor; run it under `torsocks` to reach an onion server.
