-- Per-user app data (the music and video libraries: playlists, edited
-- titles), one blob per name, encrypted under the user's master key.
-- Opaque to the server, like verified contacts.
CREATE TABLE app_data (
    user_id    TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name       TEXT NOT NULL,
    data       BLOB NOT NULL,
    revision   INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (user_id, name)
);
