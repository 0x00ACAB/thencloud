-- What happened in a folder: who added, changed, renamed, moved, trashed or
-- restored what, and when (to the hour). The server sees these as they
-- happen; this keeps them for 90 days so people sharing a folder can see
-- them. Names aren't here: the browser decrypts them from the node ids.
-- Each event is listed under every folder that held the item at the time
-- (for a move, both before and after), so it stays in a folder's history
-- after the item moves on or is deleted.
CREATE TABLE activity (
    id          INTEGER PRIMARY KEY,
    actor_id    TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    node_id     TEXT NOT NULL,
    kind        TEXT NOT NULL,
    is_folder   INTEGER NOT NULL,
    at          INTEGER NOT NULL
);
CREATE INDEX activity_at ON activity(at);
CREATE INDEX activity_node ON activity(node_id, actor_id, kind, at);
CREATE TABLE activity_scope (
    folder_id   TEXT NOT NULL,
    event_id    INTEGER NOT NULL REFERENCES activity(id) ON DELETE CASCADE,
    PRIMARY KEY (folder_id, event_id)
);
CREATE INDEX activity_scope_event ON activity_scope(event_id);
