-- Folder listings in the order they're served (see `get_children`), so a
-- page of a big folder is a seek, not a sort of the whole folder.
CREATE INDEX nodes_children ON nodes(parent_id, kind DESC, created_at, id);
DROP INDEX nodes_parent;
