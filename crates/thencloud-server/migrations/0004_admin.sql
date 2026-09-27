-- Milestone 4: administration.

-- A disabled account can't sign in; its data stays until it's deleted.
ALTER TABLE users ADD COLUMN disabled_at INTEGER;

-- Settings an admin can change at runtime, as key/value pairs:
--   registration = open | invite | closed
CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

-- Single-use invite links. Only a hash of the token is stored.
CREATE TABLE invites (
    id          TEXT PRIMARY KEY,
    token_hash  BLOB NOT NULL UNIQUE,
    created_by  TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at  INTEGER NOT NULL,
    expires_at  INTEGER NOT NULL,
    used_by     TEXT REFERENCES users(id) ON DELETE SET NULL,
    used_at     INTEGER
);
