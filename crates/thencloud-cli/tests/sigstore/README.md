Real Sigstore bundles for `tests/sigstore.rs`, with the files they sign:

- `goreleaser-v2.18.2-checksums.txt` and its bundle: `checksums.txt` from the
  [goreleaser v2.18.2 release](https://github.com/goreleaser/goreleaser/releases/tag/v2.18.2)
  (MIT), signed by goreleaser's release workflow from GitHub Actions, the
  same way thencloud's releases are.
- `cosign-v3.1.3-checksums.txt` and its bundle: `cosign_checksums.txt` from the
  [cosign v3.1.3 release](https://github.com/sigstore/cosign/releases/tag/v3.1.3)
  (Apache-2.0), signed by a Google service account (an email identity).

Both are logged in Rekor v1 with a signed entry timestamp.
