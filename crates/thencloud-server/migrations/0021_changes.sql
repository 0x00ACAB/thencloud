-- The change feed (`GET /api/changes`): every change to a node, in order,
-- with the tree's owner and every folder that held the node at the time
-- (and the node itself), so someone a folder is shared with sees changes
-- under it. Unlike `activity` nothing is merged: a sync client must see
-- every change after its cursor. Node ids only, like the live stream.
CREATE TABLE changes (
    seq         INTEGER PRIMARY KEY AUTOINCREMENT,
    node_id     TEXT NOT NULL,
    owner_id    TEXT NOT NULL,
    at          INTEGER NOT NULL
);
CREATE INDEX changes_owner ON changes(owner_id, seq);
CREATE INDEX changes_at ON changes(at);
CREATE TABLE change_scope (
    folder_id   TEXT NOT NULL,
    seq         INTEGER NOT NULL REFERENCES changes(seq) ON DELETE CASCADE,
    PRIMARY KEY (folder_id, seq)
);
CREATE INDEX change_scope_seq ON change_scope(seq);
