-- Per-user daily transfer limits set by an admin (NULL: no limit), and what
-- each user moved per UTC day, in encrypted bytes. Downloads and uploads
-- through a public link count for the link's owner.
ALTER TABLE users ADD COLUMN daily_download_limit INTEGER;
ALTER TABLE users ADD COLUMN daily_upload_limit INTEGER;
CREATE TABLE transfer_usage (
    user_id     TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    day         INTEGER NOT NULL,
    down_bytes  INTEGER NOT NULL DEFAULT 0,
    up_bytes    INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (user_id, day)
);
