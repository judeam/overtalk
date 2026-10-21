-- A keyword search excludes live partial passages once rather than looking up
-- every ranked candidate. Stable transcripts add no entries to this index.
CREATE INDEX transcripts_partial_ids ON transcripts(id) WHERE is_partial=1;
