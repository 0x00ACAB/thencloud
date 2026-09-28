-- Comments on files and folders, encrypted under the node key (see
-- `encrypt_comment`). The server knows who wrote one, on what and roughly
-- when (to the hour), not what it says.
CREATE TABLE comments (
    id          TEXT PRIMARY KEY,
    node_id     TEXT NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
    author_id   TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at  INTEGER NOT NULL,
    enc_body    BLOB NOT NULL
);
CREATE INDEX comments_node ON comments(node_id, created_at);
CREATE INDEX comments_author ON comments(author_id);
