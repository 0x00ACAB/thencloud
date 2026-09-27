-- App passwords: per-device credentials for sync clients. Each is 32 random
-- bytes made in the browser; the server keeps a SHA-256 of its auth half (a
-- slow hash adds nothing for a random 256-bit secret) and the master key
-- wrapped under its KEK.
CREATE TABLE app_passwords (
    id             TEXT PRIMARY KEY,
    user_id        TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name           TEXT NOT NULL,
    scope          TEXT NOT NULL CHECK (scope IN ('full', 'read')),
    auth_hash      BLOB NOT NULL UNIQUE,
    enc_master_key BLOB NOT NULL,
    created_at     INTEGER NOT NULL,
    last_used_at   INTEGER
);
CREATE INDEX app_passwords_user ON app_passwords(user_id);

-- Revoking an app password signs out its sessions.
ALTER TABLE sessions ADD COLUMN app_password_id TEXT REFERENCES app_passwords(id) ON DELETE CASCADE;
