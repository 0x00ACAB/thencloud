# Backup and restore

Everything the server holds is ciphertext, wrapped keys and public keys, so a backup is no more sensitive than the server itself. Losing it still loses everyone's files.

## Making a backup

```sh
thencloud-server --data-dir ./data backup /backups/thencloud-2026-10-10
```

This writes a consistent snapshot of the database (SQLite `VACUUM INTO`) and every blob it refers to into a new directory. It's safe while the server is running. On the same filesystem blobs are hard links, which is instant and takes no extra space; elsewhere they're copied. If a file is deleted while the backup runs, its missing pieces are listed and the command exits with status 2.

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

## Restoring

1. Stop the server.
2. Copy the backup to where the data should live.
3. Start the server with `--data-dir` pointing at it.
4. Run `thencloud-server --data-dir <dir> check`. It checks that every blob the database expects is there with the right size, and lists any that nothing refers to.

To restore an S3 backup, put its `thencloud.db` in a data directory and run with `--s3-prefix` pointing at the backup's `blobs/` prefix.

`check` only sees sizes; it can't decrypt anything. To check that files decrypt, each user can run Settings > **Check your files**.

## Users' own backups

Users can make their own encrypted backups with the [command line](../clients/cli.md#backups), restorable to any server.
