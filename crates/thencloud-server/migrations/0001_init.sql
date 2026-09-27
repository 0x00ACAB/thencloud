-- thencloud initial schema.
-- Every BLOB column holding user data is ciphertext produced by the client.
-- Timestamps are Unix seconds.

CREATE TABLE server_secrets (
    name  TEXT PRIMARY KEY,
    value BLOB NOT NULL
);

CREATE TABLE users (
    id              TEXT PRIMARY KEY,
    username        TEXT NOT NULL UNIQUE,
    -- Argon2id PHC hash of the client-derived auth_key (never the password).
    auth_hash       TEXT NOT NULL,
    kdf_salt        BLOB NOT NULL,
    kdf_params      TEXT NOT NULL,
    enc_master_key  BLOB NOT NULL,
    public_key      BLOB NOT NULL,
    enc_private_key BLOB NOT NULL,
    root_node_id    TEXT NOT NULL,
    quota_bytes     INTEGER NOT NULL,
    -- Stored ciphertext bytes, including in-flight uploads.
    used_bytes      INTEGER NOT NULL DEFAULT 0,
    is_admin        INTEGER NOT NULL DEFAULT 0,
    created_at      INTEGER NOT NULL
);

CREATE TABLE sessions (
    token_hash  BLOB PRIMARY KEY,
    user_id     TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    device_name TEXT NOT NULL,
    created_at  INTEGER NOT NULL,
    last_seen   INTEGER NOT NULL,
    expires_at  INTEGER NOT NULL
);
CREATE INDEX sessions_user ON sessions(user_id);

CREATE TABLE nodes (
    id                 TEXT PRIMARY KEY,
    -- Owner of the tree this node lives in; charged for its storage.
    owner_id           TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_by         TEXT NOT NULL,
    parent_id          TEXT REFERENCES nodes(id) ON DELETE CASCADE,
    kind               TEXT NOT NULL CHECK (kind IN ('file', 'folder')),
    enc_key            BLOB NOT NULL,
    enc_metadata       BLOB NOT NULL,
    current_version_id TEXT,
    revision           INTEGER NOT NULL DEFAULT 1,
    created_at         INTEGER NOT NULL,
    updated_at         INTEGER NOT NULL
);
CREATE INDEX nodes_parent ON nodes(parent_id);
CREATE INDEX nodes_owner ON nodes(owner_id);

CREATE TABLE file_versions (
    id              TEXT PRIMARY KEY,
    node_id         TEXT NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
    enc_content_key BLOB NOT NULL,
    chunk_count     INTEGER NOT NULL,
    size            INTEGER NOT NULL,
    created_by      TEXT NOT NULL,
    created_at      INTEGER NOT NULL
);
CREATE INDEX file_versions_node ON file_versions(node_id);

CREATE TABLE uploads (
    id              TEXT PRIMARY KEY,
    user_id         TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    owner_id        TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    node_id         TEXT NOT NULL,
    -- Set when this upload creates a new file.
    parent_id       TEXT,
    enc_key         BLOB,
    enc_metadata    BLOB NOT NULL,
    version_id      TEXT NOT NULL UNIQUE,
    enc_content_key BLOB NOT NULL,
    chunk_count     INTEGER NOT NULL,
    if_revision     INTEGER,
    received_bytes  INTEGER NOT NULL DEFAULT 0,
    created_at      INTEGER NOT NULL,
    expires_at      INTEGER NOT NULL
);
CREATE INDEX uploads_expires ON uploads(expires_at);

CREATE TABLE upload_chunks (
    upload_id TEXT NOT NULL REFERENCES uploads(id) ON DELETE CASCADE,
    idx       INTEGER NOT NULL,
    size      INTEGER NOT NULL,
    PRIMARY KEY (upload_id, idx)
);

CREATE TABLE shares (
    id           TEXT PRIMARY KEY,
    node_id      TEXT NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
    owner_id     TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    recipient_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    -- Node key sealed to the recipient's X25519 public key.
    wrapped_key  BLOB NOT NULL,
    permission   TEXT NOT NULL CHECK (permission IN ('read', 'write')),
    created_at   INTEGER NOT NULL,
    UNIQUE (node_id, recipient_id)
);
CREATE INDEX shares_recipient ON shares(recipient_id);
CREATE INDEX shares_owner ON shares(owner_id);

CREATE TABLE public_links (
    id            TEXT PRIMARY KEY,
    token         TEXT NOT NULL UNIQUE,
    node_id       TEXT NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
    owner_id      TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    -- Optional access gate. The decryption key is in the URL fragment and
    -- is never sent to the server.
    password_hash TEXT,
    expires_at    INTEGER,
    created_at    INTEGER NOT NULL
);
CREATE INDEX public_links_owner ON public_links(owner_id);
