# Checking a release

Every file in a release, and the Docker image, is signed with [Sigstore](https://www.sigstore.dev) by the repository's release workflow at that tag, and logged in Sigstore's public transparency log. The web client's manifest is also signed with minisign by a maintainer, with this key:

```
RWQ1oV8khQ/NC3Vr+guqukqswrgh3NOwJ8mAlRzjlvivenzDuRex3+U2
```

## Checking what a server sends: `verify-web`

The web client builds reproducibly, byte for byte, so a release can list the SHA-256 of every file in it. `thencloud verify-web` checks that a server sends exactly those files:

1. Download `thencloud-web-<version>.json`, `.json.sigstore.json` and `.json.minisig` from the [release](https://github.com/0x00ACAB/thencloud/releases) into one folder.
2. Run:
   ```sh
   thencloud verify-web https://cloud.example.com --manifest thencloud-web-v1.0.0.json
   ```

It checks both signatures, then fetches every file in every encoding the server offers (plain, gzip, Brotli) and compares. Settings > About in the web client shows the running version and this command for it.

This shows what the server sends to anyone who asks. A server that singles out one browser needs a check inside that browser to catch; the service worker's notice on a changed page is a tripwire for that, not proof.

To compare with a web client you built yourself, use `--unsigned`. To build one that matches: `scripts/release-web.sh <tag>`, or the Docker image with `--build-arg THENCLOUD_VERSION=<tag>`.

## Checking other files

Each release's notes have the `cosign` commands for the binaries, apps and image. For example:

```sh
cosign verify-blob thencloud-v1.0.0-x86_64-unknown-linux-gnu.tar.gz \
  --bundle thencloud-v1.0.0-x86_64-unknown-linux-gnu.tar.gz.sigstore.json \
  --certificate-identity https://github.com/0x00ACAB/thencloud/.github/workflows/release.yml@refs/tags/v1.0.0 \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com
```
