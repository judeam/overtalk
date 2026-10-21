-- Project knowledge is explicit, scoped and reversible. No model output is an
-- accepted fact until reviewed. Source text is fingerprinted by the application.
CREATE TABLE projects (
 id TEXT PRIMARY KEY, name TEXT NOT NULL CHECK(length(name) BETWEEN 1 AND 120),
 context TEXT NOT NULL DEFAULT '', archived INTEGER NOT NULL DEFAULT 0,
 created_at TEXT NOT NULL, updated_at TEXT NOT NULL
);
CREATE TABLE project_meetings (
 meeting_id TEXT PRIMARY KEY REFERENCES meetings(id) ON DELETE CASCADE,
 project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
 series TEXT NOT NULL DEFAULT ''
);
CREATE INDEX project_meetings_project ON project_meetings(project_id,series,meeting_id);
CREATE TABLE project_people (
 id TEXT PRIMARY KEY, project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
 name TEXT NOT NULL CHECK(length(name) BETWEEN 1 AND 120), role TEXT NOT NULL DEFAULT '',
 profile_id TEXT REFERENCES voice_profiles(id) ON DELETE SET NULL,
 UNIQUE(project_id,name)
);
CREATE INDEX project_people_voice ON project_people(profile_id);
CREATE UNIQUE INDEX project_people_unique_voice ON project_people(project_id,profile_id) WHERE profile_id IS NOT NULL;
ALTER TABLE voice_profiles ADD COLUMN scope_project_id TEXT REFERENCES projects(id) ON DELETE CASCADE;
CREATE TRIGGER remove_project_voice AFTER DELETE ON project_people BEGIN
 DELETE FROM voice_profiles WHERE id=OLD.profile_id AND scope_project_id=OLD.project_id;
END;
CREATE TABLE project_vocabulary (
 id TEXT PRIMARY KEY, project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
 term TEXT NOT NULL CHECK(length(term) BETWEEN 1 AND 160), meaning TEXT NOT NULL DEFAULT '',
 alternatives TEXT NOT NULL DEFAULT '', UNIQUE(project_id,term)
);
CREATE TABLE project_facts (
 id TEXT PRIMARY KEY, project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
 body TEXT NOT NULL, status TEXT NOT NULL DEFAULT 'proposed' CHECK(status IN ('proposed','accepted','rejected','retired')),
 source_meeting_id TEXT REFERENCES meetings(id) ON DELETE SET NULL,
 source_segment_id TEXT, source_quote TEXT NOT NULL DEFAULT '', source_hash TEXT NOT NULL DEFAULT '',
 created_at TEXT NOT NULL, updated_at TEXT NOT NULL,
 UNIQUE(project_id,source_meeting_id,source_segment_id,body)
);
CREATE INDEX project_facts_status ON project_facts(project_id,status,updated_at);
CREATE TABLE review_resolutions (
 issue_id TEXT PRIMARY KEY, fingerprint TEXT NOT NULL,
 resolution TEXT NOT NULL CHECK(resolution IN ('reviewed','dismissed')), updated_at TEXT NOT NULL
);
CREATE TABLE note_versions (
 id INTEGER PRIMARY KEY AUTOINCREMENT,
 meeting_id TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
 content TEXT NOT NULL, created_at TEXT NOT NULL, origin TEXT NOT NULL DEFAULT 'generated'
);
CREATE INDEX note_versions_meeting ON note_versions(meeting_id,id DESC);
INSERT INTO note_versions(meeting_id,content,created_at,origin)
 SELECT meeting_id,result,updated_at,'initial' FROM summary_processes WHERE result IS NOT NULL AND result <> '';
CREATE TRIGGER note_version_insert AFTER INSERT ON summary_processes
 WHEN NEW.result IS NOT NULL AND NEW.result <> '' BEGIN
 INSERT INTO note_versions(meeting_id,content,created_at,origin)
 VALUES(NEW.meeting_id,NEW.result,NEW.updated_at,CASE WHEN json_extract(NEW.result,'$.manual')=1 THEN 'edited' ELSE 'generated' END);
END;
CREATE TRIGGER note_version_update AFTER UPDATE OF result ON summary_processes
 WHEN NEW.result IS NOT NULL AND NEW.result <> '' AND NEW.result IS NOT OLD.result BEGIN
 INSERT INTO note_versions(meeting_id,content,created_at,origin)
 VALUES(NEW.meeting_id,NEW.result,NEW.updated_at,CASE WHEN json_extract(NEW.result,'$.manual')=1 THEN 'edited' ELSE 'generated' END);
 DELETE FROM note_versions WHERE meeting_id=NEW.meeting_id AND id NOT IN
 (SELECT id FROM note_versions WHERE meeting_id=NEW.meeting_id ORDER BY id DESC LIMIT 100);
END;
CREATE TABLE task_reminders (
 task_id TEXT PRIMARY KEY REFERENCES workspace_tasks(id) ON DELETE CASCADE,
 remind_at TEXT NOT NULL, notified_at TEXT
);
CREATE TABLE workspace_jobs (
 id TEXT PRIMARY KEY, meeting_id TEXT REFERENCES meetings(id) ON DELETE CASCADE,
 kind TEXT NOT NULL, state TEXT NOT NULL CHECK(state IN ('queued','running','completed','failed','cancelled','interrupted')),
 progress REAL NOT NULL DEFAULT 0, detail TEXT NOT NULL DEFAULT '', payload TEXT NOT NULL DEFAULT '{}',
 created_at TEXT NOT NULL, updated_at TEXT NOT NULL
);
CREATE INDEX workspace_jobs_state ON workspace_jobs(state,created_at);
CREATE UNIQUE INDEX workspace_jobs_active ON workspace_jobs(meeting_id,kind) WHERE state IN ('queued','running');
CREATE TABLE reading_cache (
 meeting_id TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
 source_hash TEXT NOT NULL, language TEXT NOT NULL, content TEXT NOT NULL,
 created_at TEXT NOT NULL, PRIMARY KEY(meeting_id,language)
);
CREATE TABLE workspace_connections (
 id TEXT PRIMARY KEY, provider TEXT NOT NULL CHECK(provider IN ('linear','jira','google','confluence')),
 name TEXT NOT NULL, base_url TEXT NOT NULL DEFAULT '', username TEXT NOT NULL DEFAULT '',
 project_id TEXT REFERENCES projects(id) ON DELETE SET NULL, created_at TEXT NOT NULL
);
CREATE TABLE outgoing_drafts (
 id TEXT PRIMARY KEY, connection_id TEXT NOT NULL REFERENCES workspace_connections(id) ON DELETE CASCADE,
 kind TEXT NOT NULL, target TEXT NOT NULL, title TEXT NOT NULL, body TEXT NOT NULL,
 expected_revision TEXT NOT NULL DEFAULT '', status TEXT NOT NULL DEFAULT 'draft',
 created_at TEXT NOT NULL, result TEXT
);
ALTER TABLE workspace_calendar ADD COLUMN participants TEXT NOT NULL DEFAULT '[]';
CREATE TABLE meeting_videos (
 id TEXT PRIMARY KEY, meeting_id TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
 path TEXT NOT NULL, audio_offset REAL NOT NULL DEFAULT 0, created_at TEXT NOT NULL
);
CREATE TABLE meeting_revisions (
 meeting_id TEXT PRIMARY KEY REFERENCES meetings(id) ON DELETE CASCADE,
 revision INTEGER NOT NULL DEFAULT 0
);
INSERT INTO meeting_revisions(meeting_id) SELECT id FROM meetings;
CREATE TRIGGER content_revision_insert AFTER INSERT ON transcripts BEGIN
 INSERT INTO meeting_revisions(meeting_id,revision) VALUES(NEW.meeting_id,1) ON CONFLICT(meeting_id) DO UPDATE SET revision=revision+1;
END;
CREATE TRIGGER content_revision_update AFTER UPDATE OF transcript,speaker_id,audio_start_time,audio_end_time ON transcripts BEGIN
 INSERT INTO meeting_revisions(meeting_id,revision) VALUES(NEW.meeting_id,1) ON CONFLICT(meeting_id) DO UPDATE SET revision=revision+1;
END;
CREATE TRIGGER content_revision_delete AFTER DELETE ON transcripts WHEN EXISTS(SELECT 1 FROM meetings WHERE id=OLD.meeting_id) BEGIN
 INSERT INTO meeting_revisions(meeting_id,revision) VALUES(OLD.meeting_id,1) ON CONFLICT(meeting_id) DO UPDATE SET revision=revision+1;
END;
CREATE TRIGGER content_revision_speaker AFTER UPDATE OF name ON meeting_speakers BEGIN
 INSERT INTO meeting_revisions(meeting_id,revision) VALUES(NEW.meeting_id,1) ON CONFLICT(meeting_id) DO UPDATE SET revision=revision+1;
END;
