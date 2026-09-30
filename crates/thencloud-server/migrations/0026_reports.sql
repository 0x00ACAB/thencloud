-- File reports (#108). Someone who can open a file they don't own shows one
-- version of it to the admins: the version's content key, the name, type and
-- a note are sealed to each admin in the reporter's browser, so the server
-- keeps only sealed boxes. While a report is open, the reported version's
-- blobs are kept even if the file is deleted (no foreign key to nodes).
CREATE TABLE reports (
    id          TEXT PRIMARY KEY,
    node_id     TEXT NOT NULL,
    version_id  TEXT NOT NULL,
    chunk_count INTEGER NOT NULL,
    owner_id    TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    reason      TEXT NOT NULL
                CHECK (reason IN ('illegal', 'malware', 'copyright', 'harassment', 'other')),
    reporter_id TEXT REFERENCES users(id) ON DELETE SET NULL,
    via_link    INTEGER NOT NULL DEFAULT 0,
    created_at  INTEGER NOT NULL,
    status      TEXT NOT NULL DEFAULT 'open' CHECK (status IN ('open', 'safe', 'removed')),
    handled_by  TEXT,
    handled_at  INTEGER
);
CREATE INDEX reports_status ON reports(status, created_at);
CREATE INDEX reports_open_versions ON reports(version_id) WHERE status = 'open';
CREATE INDEX reports_node ON reports(node_id);
CREATE UNIQUE INDEX reports_one_per_reporter ON reports(node_id, reporter_id)
    WHERE status = 'open' AND reporter_id IS NOT NULL;

CREATE TABLE report_boxes (
    report_id TEXT NOT NULL REFERENCES reports(id) ON DELETE CASCADE,
    admin_id  TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    sealed    BLOB NOT NULL,
    PRIMARY KEY (report_id, admin_id)
);
