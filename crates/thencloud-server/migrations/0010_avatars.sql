-- Profile pictures, encrypted under a per-user avatar key. The owner keeps
-- that key encrypted under their master key, and seals it to each person
-- they share with (in either direction). The server sees none of it.
ALTER TABLE users ADD COLUMN enc_avatar BLOB;
ALTER TABLE users ADD COLUMN enc_avatar_key BLOB;
ALTER TABLE users ADD COLUMN avatar_updated_at INTEGER;

CREATE TABLE avatar_grants (
    owner_id   TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    grantee_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    sealed_key BLOB NOT NULL,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (owner_id, grantee_id)
);
CREATE INDEX avatar_grants_grantee ON avatar_grants(grantee_id);
