-- Upload-only "file drop" links. A dropped file's enc_key is sealed to the
-- folder owner's public key until the owner's client wraps it under the
-- folder key; until then it is hidden like a trashed node.
ALTER TABLE public_links ADD COLUMN upload_only INTEGER NOT NULL DEFAULT 0;
ALTER TABLE nodes ADD COLUMN dropped INTEGER NOT NULL DEFAULT 0;
CREATE INDEX nodes_dropped ON nodes(owner_id) WHERE dropped = 1;
-- Uploads made through a link (the visitor has no account).
ALTER TABLE uploads ADD COLUMN link_id TEXT REFERENCES public_links(id) ON DELETE CASCADE;
