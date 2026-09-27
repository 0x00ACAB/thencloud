-- Encrypted name index: a keyed hash of each name under its folder's key
-- (see thencloud_crypto::name_tag), so the server can refuse two items with
-- the same name in one folder without learning the names. Nodes made before
-- this, and dropped files until they're taken in, have none.
ALTER TABLE nodes ADD COLUMN name_tag BLOB;
CREATE UNIQUE INDEX nodes_name_tag ON nodes(parent_id, name_tag)
    WHERE name_tag IS NOT NULL AND trashed_at IS NULL AND dropped = 0;
ALTER TABLE uploads ADD COLUMN name_tag BLOB;
