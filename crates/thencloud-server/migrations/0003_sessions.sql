-- Milestone 4: a session list where you can sign devices out.
--
-- Sessions are keyed by the hash of their bearer token, which must never be
-- sent back to clients, so each one gets a separate random public id.
ALTER TABLE sessions ADD COLUMN id TEXT;
UPDATE sessions SET id = lower(hex(randomblob(16)));
CREATE UNIQUE INDEX sessions_id ON sessions(id);
