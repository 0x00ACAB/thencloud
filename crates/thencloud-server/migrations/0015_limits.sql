-- Public links that stop working after a number of opens (1 for "expires
-- after first open"), and user shares that end at a set time.
ALTER TABLE public_links ADD COLUMN max_opens INTEGER;
ALTER TABLE public_links ADD COLUMN opens INTEGER NOT NULL DEFAULT 0;
ALTER TABLE shares ADD COLUMN expires_at INTEGER;
