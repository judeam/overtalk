-- A stopped/overloaded streaming session can retain a provisional hypothesis.
ALTER TABLE transcripts ADD COLUMN is_partial INTEGER NOT NULL DEFAULT 0;
