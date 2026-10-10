# Configuration

Every option is a command-line flag and an environment variable. `thencloud-server --help` prints the same list.

## General

| Flag | Env | Default |
|---|---|---|
| `--bind` | `THENCLOUD_BIND` | `127.0.0.1:8080` |
| `--data-dir` | `THENCLOUD_DATA_DIR` | `./data`. The SQLite database, and the encrypted blobs unless S3 is configured |
| `--web-dir` | `THENCLOUD_WEB_DIR` | `./web/dist`, the built web client |
| `--trust-proxy` | `THENCLOUD_TRUST_PROXY` | `false`. Behind a reverse proxy, take the client's address from the last `X-Forwarded-For` entry. Only turn it on when clients can't reach the server directly. The address is used to rate-limit sign-in attempts and never stored |
| `--public-origin` | `THENCLOUD_PUBLIC_ORIGIN` | unset. The address people open the server at, e.g. `https://cloud.example.com` (comma-separated for several). When set, passkeys only work from pages at one of these, so a passkey made on any other site is refused |
| `--limit-by-address` | `THENCLOUD_LIMIT_BY_ADDRESS` | `true`. Rate-limit sign-in attempts by address as well as by account. Turn it off for a [Tor onion service](tor.md) |

## Accounts

| Flag | Env | Default |
|---|---|---|
| `--allow-registration` | `THENCLOUD_ALLOW_REGISTRATION` | `true`. The first account can always be made (with the setup code). Admins can switch between open, invite only and closed at runtime, which overrides this |
| `--admin-username` | `THENCLOUD_ADMIN_USERNAME` | unset. The username the first account must have |
| `--default-quota` | `THENCLOUD_DEFAULT_QUOTA` | 10 GiB, in bytes of ciphertext |
| `--session-days` | `THENCLOUD_SESSION_DAYS` | `30`, sliding |

Per-user quotas and daily download and upload limits are set in the Admin view.

## Files

| Flag | Env | Default |
|---|---|---|
| `--max-versions` | `THENCLOUD_MAX_VERSIONS` | `10`, versions kept per file, including the current one |
| `--version-thinning` | `THENCLOUD_VERSION_THINNING` | `true`. Keep all versions from the last hour, then one per hour for a day, one per day for 30 days, one per week after that |
| `--trash-days` | `THENCLOUD_TRASH_DAYS` | `30` |
| `--upload-ttl-hours` | `THENCLOUD_UPLOAD_TTL_HOURS` | `24`, how long an unfinished upload is kept |

## Storage

See [Storing blobs in S3](s3.md).

| Flag | Env | Default |
|---|---|---|
| `--s3-endpoint` | `THENCLOUD_S3_ENDPOINT` | unset |
| `--s3-region` | `THENCLOUD_S3_REGION` | `us-east-1` |
| `--s3-bucket` | `THENCLOUD_S3_BUCKET` | unset |
| `--s3-access-key` | `THENCLOUD_S3_ACCESS_KEY` | unset |
| `--s3-secret-key` | `THENCLOUD_S3_SECRET_KEY` | unset |
| `--s3-prefix` | `THENCLOUD_S3_PREFIX` | empty |
| `--s3-mirror` | `THENCLOUD_S3_MIRROR` | `false` |
| `--s3-snapshot-hours` | `THENCLOUD_S3_SNAPSHOT_HOURS` | `24`; `0` turns it off |
| `--s3-snapshots-kept` | `THENCLOUD_S3_SNAPSHOTS_KEPT` | `7` |

## Optional features

| Flag | Env | Default |
|---|---|---|
| `--metrics-token` | `THENCLOUD_METRICS_TOKEN` | unset. See [Monitoring](monitoring.md) |
| `--turnstile-site-key` | `THENCLOUD_TURNSTILE_SITE_KEY` | unset. See [Bot check](turnstile.md) |
| `--turnstile-secret` | `THENCLOUD_TURNSTILE_SECRET` | unset |
| `--turnstile-hostnames` | `THENCLOUD_TURNSTILE_HOSTNAMES` | the host the sign-in request was sent to; comma-separated |
| `--yt-dlp` | `THENCLOUD_YT_DLP` | `yt-dlp`. See [Video downloader](downloader.md) |
| `--ffmpeg` | `THENCLOUD_FFMPEG` | `ffmpeg` |
| `--downloader-max-bytes` | `THENCLOUD_DOWNLOADER_MAX_BYTES` | 2 GiB per video |

## Logging

Logging uses `RUST_LOG` (for example `RUST_LOG=thencloud_server=debug`). The server never logs keys, names or contents; it can't, since it never has them.

## Subcommands

| Command | What it does |
|---|---|
| `serve` | Run the server (the default) |
| `backup DEST` | Snapshot the database and its blobs; see [Backup and restore](backups.md) |
| `check` | Check every blob the database expects is there with the right size |
| `restore-snapshot` | Put the newest S3 database snapshot into an empty data directory |

## The Admin view

Admins see accounts and counts, never content: they can change quotas and transfer limits, make other admins, disable, enable and delete accounts, create invite links, switch registration mode, and turn the video downloader on. Every admin action goes into an audit log, kept for a year.
