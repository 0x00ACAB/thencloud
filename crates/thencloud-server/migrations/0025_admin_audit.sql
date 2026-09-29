-- What admins did: accounts and settings, never content. Names are kept as
-- text so the history outlives a deleted account. Pruned after a year.
CREATE TABLE admin_audit (
    id      INTEGER PRIMARY KEY AUTOINCREMENT,
    at      INTEGER NOT NULL,
    actor   TEXT NOT NULL,
    action  TEXT NOT NULL,
    target  TEXT,
    detail  TEXT
);
CREATE INDEX admin_audit_at ON admin_audit(at);
