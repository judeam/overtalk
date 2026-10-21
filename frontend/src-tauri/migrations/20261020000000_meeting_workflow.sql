-- Capture intent exists before the meeting row. It is committed before opening
-- audio and consumed idempotently by the recording journal, including recovery.
CREATE TABLE workspace_capture_workflows (
 meeting_id TEXT PRIMARY KEY,
 event_id TEXT,
 project_id TEXT REFERENCES projects(id) ON DELETE SET NULL,
 auto_notes INTEGER NOT NULL DEFAULT 0,
 template_id TEXT NOT NULL DEFAULT 'standard_meeting',
 language TEXT NOT NULL DEFAULT 'auto',
 finalized INTEGER NOT NULL DEFAULT 0,
 created_at TEXT NOT NULL
);
CREATE TABLE workspace_notepads (
 id TEXT PRIMARY KEY,
 body TEXT NOT NULL DEFAULT '',
 revision INTEGER NOT NULL DEFAULT 0,
 updated_at TEXT NOT NULL
);
CREATE TABLE workspace_calendar_sync (
 connection_id TEXT PRIMARY KEY REFERENCES workspace_connections(id) ON DELETE CASCADE,
 synced_at TEXT,
 error TEXT
);
CREATE TRIGGER meeting_workflow_delete AFTER DELETE ON meetings BEGIN
 DELETE FROM workspace_notepads WHERE id=OLD.id OR id='followup:' || OLD.id;
 DELETE FROM workspace_capture_workflows WHERE meeting_id=OLD.id;
END;
