# HTTP API

Every route a thencloud server answers under `/api` is described in [`openapi.json`](openapi.json) (OpenAPI 3.1). Load it into any OpenAPI viewer or client generator.

It is generated from the server's own wire types (`crates/thencloud-crypto/src/api.rs`), and a test fails whenever a route in the router is missing from it or the checked-in file is out of date, so it can't drift from what the server does.

## What goes over the wire

The server only ever sees ciphertext, wrapped keys and public keys. File and folder names, file contents, comments, app data and display names arrive encrypted, and a client has to encrypt and decrypt them itself with `thencloud-crypto` (Rust, or the WASM build the web client uses). The formats and test vectors are in [Formats and test vectors](format.md). A client that sends plaintext to the server breaks the promise thencloud makes to its users, so don't write one that does.

Binary fields (keys, encrypted metadata, salts) are unpadded base64url strings.

## Signing in

1. `POST /auth/prelogin` with the username gives the Argon2id salt and parameters.
2. Derive the keys from the password (`derive_account_keys` in the crypto crate). Only the auth key is sent; the other key unwraps the master key, which stays on your side.
3. `POST /auth/login` returns a session, or a `second_factor` ticket to finish with `POST /auth/login/second-factor`.
4. Send the session token as `Authorization: Bearer <token>` from then on.

Scripts and other clients should use an app password instead (Settings > App passwords): `POST /auth/app-login` takes one and skips the second step. A read-only app password can only make GET requests.

## Conventions

- **Errors** are JSON: `{"error": "<code>", "message": "<text>"}`. The code is stable (`not_found`, `conflict`, `name_taken`, `quota_exceeded`, `rate_limited`, `transfer_limit`, and so on); the message is for people and may change.
- **Concurrent changes:** a new file version, a rename or move, a version restore and app data take an `if_revision`; if someone else changed it first, the answer is 409 `conflict` instead of an overwrite.
- **Duplicate names:** names are encrypted, so the server compares name tags (`name_tag`: a keyed hash of the name under the folder's key, which it can't reverse). A clash is 409 `name_taken`.
- **Folder listings** come back whole, or a page at a time with `?limit=`, continued with the `after` cursor. Sort by name after decrypting.
- **Uploads:** `POST /uploads`, then each encrypted chunk with `PUT /uploads/{id}/chunks/{idx}`, then `POST /uploads/{id}/finish`.
- **Public links** (`/public/{token}`) need no account. The link's key is after the `#` in the address and never reaches the server; a password-protected link is unlocked with an auth key derived from the password, and the server hands back a pass to send as `X-Link-Token`.

## Compatibility

Older desktop, Android and command-line clients keep working against newer servers, so routes and fields are only ever added: none is removed or changes meaning. Ignore fields you don't know.
