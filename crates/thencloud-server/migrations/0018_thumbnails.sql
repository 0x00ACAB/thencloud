-- A small encrypted thumbnail per file version, made in the browser (see
-- `encrypt_thumbnail`). Kept here rather than in the blob store: they're a
-- few KiB, and go with their version.
ALTER TABLE file_versions ADD COLUMN thumbnail BLOB;
