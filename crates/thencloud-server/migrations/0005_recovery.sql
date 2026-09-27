-- Optional recovery keys: a second way to unwrap the master key when the
-- password is forgotten. The server keeps an Argon2 hash of the key's auth
-- part and the master key wrapped under its KEK, never the key itself.
ALTER TABLE users ADD COLUMN recovery_hash TEXT;
ALTER TABLE users ADD COLUMN enc_master_key_recovery BLOB;
ALTER TABLE users ADD COLUMN recovery_created_at INTEGER;
