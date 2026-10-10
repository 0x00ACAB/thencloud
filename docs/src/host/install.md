# Installing

A thencloud server is one binary, a SQLite database and a folder of encrypted blobs (or an S3 bucket). It needs no other services. The browser needs a secure context, so put it behind HTTPS (or a [Tor onion service](tor.md)).

## Docker

Each release is on GitHub's registry, for amd64 and arm64:

```sh
docker run -p 127.0.0.1:8080:8080 -v thencloud-data:/data ghcr.io/0x00acab/thencloud:v1.0.0
```

### With Caddy and a certificate

[`deploy/compose.yaml`](https://github.com/0x00ACAB/thencloud/blob/main/deploy/compose.yaml) runs thencloud behind [Caddy](https://caddyserver.com), which gets a TLS certificate for your domain:

1. Point your domain's DNS at the machine, and open ports 80 and 443.
2. Start it:
   ```sh
   THENCLOUD_DOMAIN=cloud.example.com docker compose -f deploy/compose.yaml up -d
   ```
3. Get the setup code: `docker compose -f deploy/compose.yaml logs thencloud | grep "setup code"`.

The compose file lists the common settings as commented-out environment variables. Caddy keeps no access log unless you configure one; leave it that way, since it would record who opened which public link.

## Release binaries

Each [release](https://github.com/0x00ACAB/thencloud/releases) has server binaries for Linux (x86_64, arm64) and macOS (arm64), and the web client as `thencloud-web-<version>.tar.gz`. Unpack the web client and point the server at it:

```sh
tar xf thencloud-web-v1.0.0.tar.gz
./thencloud-server --bind 127.0.0.1:8080 --data-dir /var/lib/thencloud --web-dir ./dist
```

Run it under systemd or similar, behind a reverse proxy that terminates TLS. Turn on `--trust-proxy` when only the proxy can reach the server.

## From source

You need a [rustup](https://rustup.rs) toolchain (`build.sh` adds the `wasm32-unknown-unknown` target), `wasm-pack` (`cargo install wasm-pack`) and Node.js 22 or newer.

```sh
git clone https://github.com/0x00ACAB/thencloud && cd thencloud
./build.sh --release
./target/release/thencloud-server --bind 127.0.0.1:8080
```

Or build the image: `docker build -t thencloud .`. To get an image whose web client passes [`verify-web`](../security/verify.md), build from a release tag with `--build-arg THENCLOUD_VERSION=<tag>`.

## The first account

The first account to register becomes the admin. So that nobody else can claim a new server first, it needs the **setup code** the server prints in its log on first start. The code is also kept in `<data dir>/setup-code` until it's used. `--admin-username` additionally fixes the admin's username.

After that, registration is open to anyone by default. Admins can switch between open, invite only and closed in the Admin view; `--allow-registration false` sets the starting point.

## Before you rely on it

- Set up [backups](backups.md).
- Read the [threat model](../security/threat-model.md), so you know what you, as the operator, can and can't see.
