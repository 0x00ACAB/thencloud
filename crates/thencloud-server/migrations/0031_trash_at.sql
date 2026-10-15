-- A date the owner chose for a file or folder to go to the trash on its
-- own (rounded to the hour). The owner chooses to tell the server this
-- date, so the janitor can act on it while no device is online; it never
-- learns the name. Cleared when the janitor trashes the node.
ALTER TABLE nodes ADD COLUMN trash_at INTEGER;
CREATE INDEX nodes_trash_at ON nodes(trash_at) WHERE trash_at IS NOT NULL;
