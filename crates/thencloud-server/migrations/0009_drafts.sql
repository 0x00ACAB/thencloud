-- Unsaved edits, kept while someone edits a text file so a closed tab or a
-- crash doesn't lose them. Encrypted under the editor's master key and bound
-- to them and the file; the server only stores the ciphertext.
CREATE TABLE drafts (
    node_id       TEXT NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
    user_id       TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    data          BLOB NOT NULL,
    -- The file's revision the draft started from.
    base_revision INTEGER NOT NULL,
    updated_at    INTEGER NOT NULL,
    PRIMARY KEY (node_id, user_id)
);
CREATE INDEX drafts_user ON drafts(user_id);
