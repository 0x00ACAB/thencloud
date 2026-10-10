# How the encryption works

Everything below runs in your browser (as WebAssembly built from the Rust crate `thencloud-crypto`) or in the native clients, which use the same crate. The server only stores what comes out of it. Every byte is specified in [Formats and test vectors](../reference/format.md).

## The key tree

```
password ──Argon2id──► root ──HKDF──┬─► auth_key  → server (stored only as Argon2id(auth_key))
                                    └─► kek       (never leaves the client)
kek        ──wraps──► master key
recovery key (optional, 256-bit) ──HKDF──┬─► recovery auth → server (stored only as Argon2id)
                                         └─► recovery kek ──wraps──► master key
passkey PRF output (optional) ──HKDF──► passkey kek ──wraps──► master key
master key ──wraps──► X25519 private key, ML-KEM-768 seed, root folder key
folder key ──wraps──► child node keys
node key   ──seals──► metadata {name, mime, size, mtime}
file key   ──wraps──► per-version content key ──seals──► 4 MiB chunks
```

Your password becomes two keys: one to prove who you are (the server hashes it again and keeps only that hash) and one to unwrap your master key, which never leaves your device. Everything else hangs off the master key.

## Primitives

- **AEAD:** XChaCha20-Poly1305.
- **KDF:** Argon2id (64 MiB, t=3) and HKDF-SHA256.
- **Sharing:** hybrid X25519 + ML-KEM-768 sealed boxes (plain X25519 to accounts that don't have an ML-KEM key yet).

## Context binding

Every ciphertext carries associated data that ties it to where it belongs:

- Node keys and metadata are bound to the node id.
- Content keys are bound to the node id and version id.
- Chunks are bound to the version id, chunk index and an is-last flag.

So a malicious server can't swap files, move ciphertext between nodes, reorder or truncate chunks, or serve an old version's chunks under a new one. Decryption fails if it tries.

## Sharing with a person

The owner fetches the recipient's public key, checks its fingerprint (shown in both people's Settings), and seals the folder or file key to it. The recipient opens it with their private key and can then unwrap the whole subtree.

**Post-quantum sealing.** Every account has an ML-KEM-768 key pair (FIPS 203) next to its X25519 one. A key sealed to such an account runs both key exchanges and feeds both shared secrets to HKDF, with every public value in the salt, so a recording of the share stays closed unless both X25519 and ML-KEM are broken. The fingerprint covers the X25519 key and the SHA-256 of the ML-KEM key. Accounts made before ML-KEM get a key pair on their next sign-in, and shares to them made before that are sealed again by the recipient.

## Public links

`https://host/s/<token>#<key>`. Browsers never send the part after `#`, so the server only sees the token. The share page reads the key from the address and decrypts locally. `Referrer-Policy: no-referrer` and a strict same-origin CSP keep the address from leaking to anyone else.

With a password, the link carries a random secret after `#` instead, and the key is wrapped under the secret and the password together (Argon2id in the browser). Visitors prove the password with a key derived from it, so the server never sees the password, and a server that skipped the check would still hand out nothing anyone can open without it.

## Two-step sign-in

With an authenticator app or any passkey, a correct password gets a short-lived ticket instead of a session, and the wrapped master key is only handed out once a code or passkey checks out. Neither holds a key: the TOTP secret is a verifier, and codes are single-use.

## Passkeys

When the authenticator supports the PRF extension, the browser asks the passkey for its PRF output for a fixed input, derives a key from it with HKDF, and wraps a copy of the master key under it, bound to the credential id. Signing in with the passkey alone needs user verification (PIN or biometric); the server checks the signature (ES256, EdDSA or RS256) against a one-time challenge and only then returns the wrapped copy, which it can't unwrap.

## Recovery key and app passwords

Both are 256-bit secrets made in the browser and shown once, as 11 groups of 5 characters (Crockford base32 with a checksum). HKDF splits each into an auth part (the server keeps only a hash) and a key that wraps its own copy of the master key. That's why they can skip the second step: each is a strong key in its own right.

## Profile pictures and names

Pictures, display names and pronouns are encrypted under a random avatar key. Your copy is wrapped under your master key and sealed to each person you share with or who shares with you. A new picture gets a new key, so people you stopped sharing with can't see it. Display names are padded so their length doesn't show.

## Padding

File contents are padded (every file under 16 KiB to 16 KiB, larger ones with Padmé, at most about 12% extra) and metadata to 128-byte steps, so sizes give only a rough idea.
