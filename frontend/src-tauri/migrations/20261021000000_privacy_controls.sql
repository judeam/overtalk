CREATE TABLE privacy_settings (
 id INTEGER PRIMARY KEY CHECK(id=1),
 credential_namespace TEXT NOT NULL DEFAULT (lower(hex(randomblob(16)))),
 policy TEXT NOT NULL DEFAULT '{}'
);
INSERT INTO privacy_settings(id) VALUES(1);
CREATE TABLE privacy_events (
 sequence INTEGER PRIMARY KEY AUTOINCREMENT,
 occurred_at TEXT NOT NULL,
 actor TEXT NOT NULL,
 operation TEXT NOT NULL,
 object_id TEXT NOT NULL DEFAULT '',
 outcome TEXT NOT NULL,
 previous_hash TEXT NOT NULL,
 event_hash TEXT NOT NULL
);
CREATE TABLE privacy_authorizations (
 id TEXT PRIMARY KEY,
 subject_name TEXT NOT NULL,
 purpose TEXT NOT NULL CHECK(purpose IN ('recording','voice_identification')),
 basis TEXT NOT NULL,
 notice_version TEXT NOT NULL,
 recorded_at TEXT NOT NULL,
 expires_at TEXT,
 withdrawn_at TEXT
);
CREATE TABLE privacy_erasures (
 meeting_id TEXT PRIMARY KEY,
 occurred_at TEXT NOT NULL,
 pending_paths TEXT NOT NULL DEFAULT '[]'
);
ALTER TABLE voice_profiles ADD COLUMN authorization_id TEXT REFERENCES privacy_authorizations(id) ON DELETE CASCADE;
CREATE TABLE privacy_holds (
 meeting_id TEXT PRIMARY KEY REFERENCES meetings(id) ON DELETE CASCADE,
 reason TEXT NOT NULL,
 created_at TEXT NOT NULL
);

ALTER TABLE outgoing_drafts ADD COLUMN source_meeting_id TEXT REFERENCES meetings(id) ON DELETE CASCADE;
CREATE INDEX outgoing_drafts_source ON outgoing_drafts(source_meeting_id);

-- FTS has its own deletion setting in addition to PRAGMA secure_delete.
-- The packaged SQLite runtime supports this (introduced in 3.42).
INSERT INTO workspace_fts(workspace_fts,rank) VALUES('secure-delete',1);
