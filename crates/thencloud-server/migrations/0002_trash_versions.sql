-- Milestone 3: file version history and a trash bin.

-- Each version keeps the (encrypted) metadata it was uploaded with, so the
-- client can show its plaintext size and mtime. Backfill from the node.
ALTER TABLE file_versions ADD COLUMN enc_metadata BLOB;
UPDATE file_versions
   SET enc_metadata = (SELECT n.enc_metadata FROM nodes n WHERE n.id = file_versions.node_id);

-- Trashed nodes stay where they are in the tree (so their keys still unwrap
-- under the original parent); a node is hidden if it or any ancestor is
-- trashed.
ALTER TABLE nodes ADD COLUMN trashed_at INTEGER;
ALTER TABLE nodes ADD COLUMN trashed_by TEXT;
CREATE INDEX nodes_trashed ON nodes(owner_id, trashed_at) WHERE trashed_at IS NOT NULL;
