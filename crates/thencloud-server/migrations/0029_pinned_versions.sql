-- A pinned version is kept: the per-file limit, age-based thinning and
-- pruning for space all pass it over. Its name, if it has one, is in the
-- pinner's encrypted app data, never here.
ALTER TABLE file_versions ADD COLUMN pinned INTEGER NOT NULL DEFAULT 0;
