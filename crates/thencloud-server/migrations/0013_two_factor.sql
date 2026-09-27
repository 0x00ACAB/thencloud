-- Two-factor sign-in.
--
-- TOTP: the shared secret is a verifier, not a key; it can't decrypt
-- anything. The last time step used is kept so each code works once.
ALTER TABLE users ADD COLUMN totp_secret BLOB;
ALTER TABLE users ADD COLUMN totp_created_at INTEGER;
ALTER TABLE users ADD COLUMN totp_last_step INTEGER;

-- Passkeys (WebAuthn). `public_key` is the credential's COSE key. When the
-- authenticator supports the PRF extension, `enc_master_key` is the master
-- key wrapped under a key derived from its PRF output, so the passkey alone
-- can unlock the account; the server can't compute that output.
CREATE TABLE passkeys (
    id             TEXT PRIMARY KEY,
    user_id        TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    credential_id  BLOB NOT NULL UNIQUE,
    public_key     BLOB NOT NULL,
    rp_id          TEXT NOT NULL,
    sign_count     INTEGER NOT NULL,
    name           TEXT NOT NULL,
    enc_master_key BLOB,
    created_at     INTEGER NOT NULL,
    last_used_at   INTEGER
);
CREATE INDEX passkeys_user ON passkeys(user_id);

-- Short-lived state: a sign-in waiting for its second factor, a passkey
-- being registered, a TOTP secret being confirmed, and passkey sign-in
-- challenges already used. `id` is a hash of the token the client holds.
CREATE TABLE auth_challenges (
    id         TEXT PRIMARY KEY,
    user_id    TEXT REFERENCES users(id) ON DELETE CASCADE,
    purpose    TEXT NOT NULL,
    challenge  BLOB NOT NULL,
    data       BLOB,
    expires_at INTEGER NOT NULL
);
CREATE INDEX auth_challenges_expiry ON auth_challenges(expires_at);
