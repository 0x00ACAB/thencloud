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

Without the token set, the endpoint doesn't exist.

## Admin audit log

The Admin view's **Admin activity** lists every admin action (quota, limits, admin, disable, enable, delete, registration mode, invites, the downloader), with who did it, kept for a year.
