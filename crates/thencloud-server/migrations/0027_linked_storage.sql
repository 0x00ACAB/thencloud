-- Storage accounts people link to their own (Google Drive first), to keep a
-- mirror of their files there or to use as extra space. Only ciphertext goes
-- to them, encrypted exactly like the server's own blobs.
CREATE TABLE storage_accounts (
    id          TEXT PRIMARY KEY,
    user_id     TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    provider    TEXT NOT NULL,              -- 'google'
    mode        TEXT NOT NULL,              -- 'mirror' or 'extra'
    -- The refresh token and the account's address, sealed under
    -- <data dir>/storage-token-key (format `storage-secret`).
    enc_token   BLOB NOT NULL,
    enc_label   BLOB,
    folder_id   TEXT NOT NULL,              -- thencloud's folder there
    used_bytes  INTEGER NOT NULL DEFAULT 0, -- what thencloud keeps there
    free_bytes  INTEGER,                    -- free space when last asked; NULL: no limit
    checked_at  INTEGER,
    broken_at   INTEGER,                    -- the token stopped working: link it again
    created_at  INTEGER NOT NULL
);
CREATE INDEX storage_accounts_user ON storage_accounts(user_id);

-- Where each chunk kept at a provider is. No foreign keys: a version's rows
-- stay until the copies there are deleted, so a failed delete is retried.
CREATE TABLE remote_chunks (
    account_id  TEXT NOT NULL,
    version_id  TEXT NOT NULL,
    idx         INTEGER NOT NULL,
    remote_id   TEXT NOT NULL,
    size        INTEGER NOT NULL,
    PRIMARY KEY (account_id, version_id, idx)
);
CREATE INDEX remote_chunks_version ON remote_chunks(version_id);

-- A version (or upload) kept only at a provider names its account; NULL
-- means this server, as for everything written before.
ALTER TABLE file_versions ADD COLUMN account_id TEXT;
ALTER TABLE uploads ADD COLUMN account_id TEXT;

-- Where new files go first: 'server' or 'linked'.
ALTER TABLE users ADD COLUMN storage_prefer TEXT NOT NULL DEFAULT 'server';
