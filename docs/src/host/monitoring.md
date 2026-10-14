# Monitoring

## Health

`GET /api/health` answers `200 ok` while the database and data directory are available, and `503` otherwise. It needs no sign-in.

## Prometheus metrics

Set `--metrics-token` to a long random string and `GET /api/metrics` serves Prometheus metrics (the counts in the Admin view) to requests that carry it:

```yaml
scrape_configs:
  - job_name: thencloud
    scheme: https
    authorization:
      credentials: <the token>
    static_configs:
      - targets: [cloud.example.com]
```

Without the token set, the endpoint doesn't exist. Besides the counts, it reports `thencloud_data_disk_total_bytes` and `thencloud_data_disk_free_bytes` for the disk holding the data directory.

## Storage alerts

The Admin view's **Server storage** shows how full the data directory's disk is (and the blob directory's, when it's on another disk), and lists accounts using 90% or more of their quota. A disk with under a tenth or under 2 GiB free is marked nearly full, and so is Admin in the sidebar, for every admin.

A bucket has no size the server can ask for. If yours has one (a plan or a free tier), pass it as `--s3-capacity` in bytes, and the bucket is shown too, compared with the ciphertext the accounts hold there (database snapshots aren't counted).

## Admin audit log

The Admin view's **Admin activity** lists every admin action (quota, limits, admin, disable, enable, delete, registration mode, invites, the downloader), with who did it, kept for a year.
