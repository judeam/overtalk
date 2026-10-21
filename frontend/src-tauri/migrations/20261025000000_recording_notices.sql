-- How participants were told about a recording: the notice copied to the call
-- chat, read aloud, confirmed, or skipped. It is written before audio starts,
-- so a skipped notice is recorded as skipped rather than left out. `required`
-- says whether the notice step was turned on in Privacy settings at the time.
CREATE TABLE meeting_notices (
 meeting_id TEXT PRIMARY KEY,
 method TEXT NOT NULL CHECK(method IN ('copied','read_aloud','confirmed','skipped')),
 route TEXT NOT NULL CHECK(route IN ('local','external')),
 notice TEXT NOT NULL DEFAULT '',
 required INTEGER NOT NULL DEFAULT 1,
 given_at TEXT NOT NULL
);
CREATE TRIGGER meeting_notice_delete AFTER DELETE ON meetings BEGIN
 DELETE FROM meeting_notices WHERE meeting_id=OLD.id;
END;
