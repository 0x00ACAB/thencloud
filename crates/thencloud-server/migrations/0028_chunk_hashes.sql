-- SHA-256 hashes of stored ciphertext chunks, used to verify local mobile
-- cache entries without downloading the ciphertext just to check freshness.
ALTER TABLE upload_chunks ADD COLUMN sha256 BLOB;

CREATE TABLE file_chunk_hashes (
    version_id TEXT NOT NULL REFERENCES file_versions(id) ON DELETE CASCADE,
    idx        INTEGER NOT NULL,
    sha256     BLOB NOT NULL,
    PRIMARY KEY (version_id, idx)
);
