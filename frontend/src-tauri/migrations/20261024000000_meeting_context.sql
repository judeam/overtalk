-- Calendar rows are replaced on every sync. A recording keeps its own copy of
-- the event it was linked to, so a later sync cannot erase who was invited.
ALTER TABLE workspace_calendar ADD COLUMN organizer TEXT NOT NULL DEFAULT '';
ALTER TABLE workspace_calendar ADD COLUMN series_id TEXT NOT NULL DEFAULT '';
CREATE TABLE meeting_events (
 meeting_id TEXT PRIMARY KEY,
 event_id TEXT NOT NULL,
 source TEXT NOT NULL,
 series_id TEXT NOT NULL DEFAULT '',
 title TEXT NOT NULL,
 starts_at TEXT NOT NULL,
 ends_at TEXT NOT NULL,
 organizer TEXT NOT NULL DEFAULT '',
 participants TEXT NOT NULL DEFAULT '[]',
 linked_by TEXT NOT NULL CHECK(linked_by IN ('join','time')),
 created_at TEXT NOT NULL
);
CREATE INDEX meeting_events_event ON meeting_events(event_id);
CREATE INDEX meeting_events_series ON meeting_events(series_id) WHERE series_id <> '';

-- How a meeting reached its project. A person's choice is never replaced by an
-- automatic one; automatic choices keep the reason they were made.
ALTER TABLE project_meetings ADD COLUMN origin TEXT NOT NULL DEFAULT 'manual'
 CHECK(origin IN ('manual','automatic'));
ALTER TABLE project_meetings ADD COLUMN reason TEXT NOT NULL DEFAULT '';
ALTER TABLE workspace_capture_workflows ADD COLUMN project_origin TEXT NOT NULL DEFAULT 'manual'
 CHECK(project_origin IN ('manual','automatic','undecided'));
ALTER TABLE workspace_capture_workflows ADD COLUMN project_reason TEXT NOT NULL DEFAULT '';
CREATE TABLE project_suggestions (
 meeting_id TEXT PRIMARY KEY REFERENCES meetings(id) ON DELETE CASCADE,
 project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
 reason TEXT NOT NULL DEFAULT '',
 created_at TEXT NOT NULL
);
CREATE TRIGGER project_suggestion_resolved AFTER INSERT ON project_meetings BEGIN
 DELETE FROM project_suggestions WHERE meeting_id=NEW.meeting_id;
END;
CREATE TRIGGER project_suggestion_replaced AFTER UPDATE OF project_id ON project_meetings BEGIN
 DELETE FROM project_suggestions WHERE meeting_id=NEW.meeting_id;
END;

-- Projects for upcoming calendar events. NULL means the person chose no project.
CREATE TABLE event_projects (
 event_id TEXT PRIMARY KEY,
 project_id TEXT REFERENCES projects(id) ON DELETE CASCADE,
 origin TEXT NOT NULL CHECK(origin IN ('manual','automatic','suggested')),
 reason TEXT NOT NULL DEFAULT '',
 updated_at TEXT NOT NULL
);

-- Signals that file meetings into a project: invitee addresses, organisations,
-- recurring series, title phrases and calendar accounts. Every row is visible
-- and removable; learned rows record how often they were confirmed.
CREATE TABLE project_match_rules (
 id TEXT PRIMARY KEY,
 project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
 kind TEXT NOT NULL CHECK(kind IN ('email','domain','series','title','account')),
 value TEXT NOT NULL CHECK(length(value) BETWEEN 1 AND 320),
 label TEXT NOT NULL DEFAULT '',
 origin TEXT NOT NULL CHECK(origin IN ('manual','learned')),
 confirmations INTEGER NOT NULL DEFAULT 1,
 enabled INTEGER NOT NULL DEFAULT 1,
 updated_at TEXT NOT NULL,
 UNIQUE(project_id,kind,value)
);
CREATE INDEX project_match_rules_value ON project_match_rules(kind,value);

-- Prepared before a calendar event. The body is structured JSON whose items
-- carry their source; it is reference material, never a meeting record.
CREATE TABLE meeting_briefs (
 event_id TEXT PRIMARY KEY,
 project_id TEXT REFERENCES projects(id) ON DELETE SET NULL,
 title TEXT NOT NULL,
 starts_at TEXT NOT NULL,
 body TEXT NOT NULL DEFAULT '{}',
 status TEXT NOT NULL CHECK(status IN ('facts','ready','failed')),
 created_at TEXT NOT NULL,
 updated_at TEXT NOT NULL,
 notified_at TEXT
);
CREATE INDEX meeting_briefs_start ON meeting_briefs(starts_at);

CREATE TRIGGER meeting_context_delete AFTER DELETE ON meetings BEGIN
 DELETE FROM meeting_events WHERE meeting_id=OLD.id;
END;
