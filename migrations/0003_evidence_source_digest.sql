-- Record what an imported image looked like before its metadata was removed.
--
-- Evidence.sha256 is the digest of the STORED bytes, because that is what a reader can
-- re-hash to prove the vault copy is unmodified. Scrubbing image metadata happens before
-- that hash is taken, which would otherwise silently discard the identity of the file the
-- author actually selected. These two columns keep the chain of custody complete:
-- source_sha256 is the file exactly as supplied, and metadata_stripped says plainly that
-- bytes were removed between the two.
--
-- Existing rows predate scrubbing. source_sha256 stays NULL for them rather than being
-- backfilled from sha256, because asserting the two were equal would be a claim nobody
-- ever checked.
ALTER TABLE evidence ADD COLUMN source_sha256 TEXT;
ALTER TABLE evidence ADD COLUMN metadata_stripped INTEGER NOT NULL DEFAULT 0;
