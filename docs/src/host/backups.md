# Backup and restore

Everything the server holds is ciphertext, wrapped keys and public keys, so a backup is no more sensitive than the server itself. Losing it still loses everyone's files.

## Making a backup

```sh
thencloud-server --data-dir ./data backup /backups/thencloud-2026-10-10
```

This writes a consistent snapshot of the database (SQLite `VACUUM INTO`) and every blob it refers to into a new directory. It's safe while the server is running. On the same filesystem blobs are hard links, which is instant and takes no extra space; elsewhere they're copied. If a file is deleted while the backup runs, its missing pieces are listed and the command exits with status 2. Files people keep only in their linked Google Drive aren't in a backup; they stay in that Drive, and a restored server reaches them with the same `storage-token-key`. A directory backup also gets `link-token-key` and `storage-token-key`, the key public-link tokens are sealed under (the database keeps only their hashes); an S3 backup doesn't, so keep a copy of that file yourself. Without it, a restored server still opens every link but can't show owners the links they made before.

The destination can also be a bucket, using the configured S3 endpoint and credentials, from either a local or an S3 blob store:

```sh
thencloud-server backup s3://my-backups/thencloud/
```

With Docker Compose:

```sh
docker compose -f deploy/compose.yaml exec thencloud thencloud-server backup /data/backup-$(date +%F)
```

The backup then lands in the data volume; copy it somewhere else.

If you store blobs in S3, the bucket already holds [daily database snapshots](s3.md#database-snapshots).

### Locking backups against deletion

A backup the server can delete is a backup an attacker on the server can delete. With a bucket that has **S3 Object Lock** turned on (it has to be turned on when the bucket is made), every object of a backup can be locked for a number of days:

```sh
thencloud-server backup s3://my-backups/2026-10-15/ --retain-days 30
```

Until the date passes, nobody can delete or overwrite them, not even with the credentials that wrote them. `--retain-mode` picks how strictly:

- `governance` (the default): an account with the `s3:BypassGovernanceRetention` permission still can. Don't give that permission to the server's credentials.
- `compliance`: nobody can, the bucket's owner included, until the date. You pay to store them until then, so start with governance.

Use a new prefix for each backup (a backup refuses to write into one that isn't empty), and a lifecycle rule in the bucket to clean up old ones once their lock runs out. Providers differ: AWS S3, Backblaze B2, Wasabi and MinIO support Object Lock this way; Cloudflare R2 has its own bucket-wide locks instead, which work without `--retain-days`.

## Restoring

1. Stop the server.
2. Copy the backup to where the data should live.
3. Start the server with `--data-dir` pointing at it.
4. Run `thencloud-server --data-dir <dir> check`. It checks that every blob the database expects is there with the right size, and lists any that nothing refers to.

To restore an S3 backup, put its `thencloud.db` in a data directory and run with `--s3-prefix` pointing at the backup's `blobs/` prefix.

`check` only sees sizes; it can't decrypt anything. To check that files decrypt, each user can run Settings > **Check your files**.

## Users' own backups

Users can make their own encrypted backups with the [command line](../clients/cli.md#backups), restorable to any server.
