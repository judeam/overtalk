-- Meeting knowledge remains local. Every material source has a stable ID.
CREATE TABLE workspace_documents (
    id TEXT PRIMARY KEY,
    meeting_id TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
    source_kind TEXT NOT NULL,
    source_id TEXT NOT NULL,
    title TEXT NOT NULL,
    body TEXT NOT NULL,
    start_time REAL,
    end_time REAL,
    recorded_at TEXT NOT NULL
);
CREATE INDEX workspace_documents_meeting ON workspace_documents(meeting_id);
CREATE VIRTUAL TABLE workspace_fts USING fts5(title,body,content='workspace_documents',content_rowid='rowid',tokenize='unicode61 remove_diacritics 2');
CREATE TRIGGER workspace_documents_ai AFTER INSERT ON workspace_documents BEGIN
  INSERT INTO workspace_fts(rowid,title,body) VALUES(new.rowid,new.title,new.body);
END;
CREATE TRIGGER workspace_documents_ad AFTER DELETE ON workspace_documents BEGIN
  INSERT INTO workspace_fts(workspace_fts,rowid,title,body) VALUES('delete',old.rowid,old.title,old.body);
END;
CREATE TRIGGER workspace_documents_au AFTER UPDATE ON workspace_documents BEGIN
  INSERT INTO workspace_fts(workspace_fts,rowid,title,body) VALUES('delete',old.rowid,old.title,old.body);
  INSERT INTO workspace_fts(rowid,title,body) VALUES(new.rowid,new.title,new.body);
END;
INSERT INTO workspace_documents(id,meeting_id,source_kind,source_id,title,body,start_time,end_time,recorded_at) SELECT 'transcript:'||s.id,s.meeting_id,'transcript',s.id,m.title,s.transcript,s.audio_start_time,s.audio_end_time,m.created_at FROM transcripts s JOIN meetings m ON m.id=s.meeting_id;
CREATE TRIGGER workspace_transcripts_ai AFTER INSERT ON transcripts BEGIN INSERT INTO workspace_documents(id,meeting_id,source_kind,source_id,title,body,start_time,end_time,recorded_at) SELECT 'transcript:'||new.id,new.meeting_id,'transcript',new.id,m.title,new.transcript,new.audio_start_time,new.audio_end_time,m.created_at FROM meetings m WHERE m.id=new.meeting_id ON CONFLICT(id) DO UPDATE SET title=excluded.title,body=excluded.body,start_time=excluded.start_time,end_time=excluded.end_time; END;
CREATE TRIGGER workspace_transcripts_au AFTER UPDATE ON transcripts BEGIN INSERT INTO workspace_documents(id,meeting_id,source_kind,source_id,title,body,start_time,end_time,recorded_at) SELECT 'transcript:'||new.id,new.meeting_id,'transcript',new.id,m.title,new.transcript,new.audio_start_time,new.audio_end_time,m.created_at FROM meetings m WHERE m.id=new.meeting_id ON CONFLICT(id) DO UPDATE SET title=excluded.title,body=excluded.body,start_time=excluded.start_time,end_time=excluded.end_time; END;
CREATE TRIGGER workspace_transcripts_ad AFTER DELETE ON transcripts BEGIN DELETE FROM workspace_documents WHERE id='transcript:'||old.id; END;
INSERT INTO workspace_documents(id,meeting_id,source_kind,source_id,title,body,start_time,end_time,recorded_at) SELECT 'note:'||s.meeting_id,s.meeting_id,'note',s.meeting_id,m.title,COALESCE(s.notes_markdown,''),NULL,NULL,m.created_at FROM meeting_notes s JOIN meetings m ON m.id=s.meeting_id;
CREATE TRIGGER workspace_meeting_notes_ai AFTER INSERT ON meeting_notes BEGIN INSERT INTO workspace_documents(id,meeting_id,source_kind,source_id,title,body,start_time,end_time,recorded_at) SELECT 'note:'||new.meeting_id,new.meeting_id,'note',new.meeting_id,m.title,COALESCE(new.notes_markdown,''),NULL,NULL,m.created_at FROM meetings m WHERE m.id=new.meeting_id ON CONFLICT(id) DO UPDATE SET title=excluded.title,body=excluded.body,start_time=excluded.start_time,end_time=excluded.end_time; END;
CREATE TRIGGER workspace_meeting_notes_au AFTER UPDATE ON meeting_notes BEGIN INSERT INTO workspace_documents(id,meeting_id,source_kind,source_id,title,body,start_time,end_time,recorded_at) SELECT 'note:'||new.meeting_id,new.meeting_id,'note',new.meeting_id,m.title,COALESCE(new.notes_markdown,''),NULL,NULL,m.created_at FROM meetings m WHERE m.id=new.meeting_id ON CONFLICT(id) DO UPDATE SET title=excluded.title,body=excluded.body,start_time=excluded.start_time,end_time=excluded.end_time; END;
CREATE TRIGGER workspace_meeting_notes_ad AFTER DELETE ON meeting_notes BEGIN DELETE FROM workspace_documents WHERE id='note:'||old.meeting_id; END;
INSERT INTO workspace_documents(id,meeting_id,source_kind,source_id,title,body,start_time,end_time,recorded_at) SELECT 'summary:'||s.meeting_id,s.meeting_id,'summary',s.meeting_id,m.title,CASE WHEN json_valid(s.result) THEN COALESCE(json_extract(s.result,'$.markdown'),'') ELSE '' END,NULL,NULL,m.created_at FROM summary_processes s JOIN meetings m ON m.id=s.meeting_id;
CREATE TRIGGER workspace_summary_processes_ai AFTER INSERT ON summary_processes BEGIN INSERT INTO workspace_documents(id,meeting_id,source_kind,source_id,title,body,start_time,end_time,recorded_at) SELECT 'summary:'||new.meeting_id,new.meeting_id,'summary',new.meeting_id,m.title,CASE WHEN json_valid(new.result) THEN COALESCE(json_extract(new.result,'$.markdown'),'') ELSE '' END,NULL,NULL,m.created_at FROM meetings m WHERE m.id=new.meeting_id ON CONFLICT(id) DO UPDATE SET title=excluded.title,body=excluded.body,start_time=excluded.start_time,end_time=excluded.end_time; END;
CREATE TRIGGER workspace_summary_processes_au AFTER UPDATE ON summary_processes BEGIN INSERT INTO workspace_documents(id,meeting_id,source_kind,source_id,title,body,start_time,end_time,recorded_at) SELECT 'summary:'||new.meeting_id,new.meeting_id,'summary',new.meeting_id,m.title,CASE WHEN json_valid(new.result) THEN COALESCE(json_extract(new.result,'$.markdown'),'') ELSE '' END,NULL,NULL,m.created_at FROM meetings m WHERE m.id=new.meeting_id ON CONFLICT(id) DO UPDATE SET title=excluded.title,body=excluded.body,start_time=excluded.start_time,end_time=excluded.end_time; END;
CREATE TRIGGER workspace_summary_processes_ad AFTER DELETE ON summary_processes BEGIN DELETE FROM workspace_documents WHERE id='summary:'||old.meeting_id; END;
INSERT INTO workspace_documents(id,meeting_id,source_kind,source_id,title,body,recorded_at) SELECT 'meeting:'||id,id,'meeting',id,title,title,created_at FROM meetings;
CREATE TRIGGER workspace_meetings_ai AFTER INSERT ON meetings BEGIN INSERT INTO workspace_documents(id,meeting_id,source_kind,source_id,title,body,recorded_at) VALUES('meeting:'||new.id,new.id,'meeting',new.id,new.title,new.title,new.created_at); END;
CREATE TRIGGER workspace_meeting_title_au AFTER UPDATE OF title ON meetings BEGIN
 UPDATE workspace_documents SET title=new.title,body=CASE WHEN source_kind='meeting' THEN new.title ELSE body END WHERE meeting_id=new.id;
END;
CREATE TABLE meeting_tags (
 meeting_id TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
 tag TEXT NOT NULL CHECK(length(tag) BETWEEN 1 AND 64),
 PRIMARY KEY(meeting_id,tag)
);
CREATE INDEX meeting_tags_tag ON meeting_tags(tag,meeting_id);
CREATE TABLE workspace_tasks (
 id TEXT PRIMARY KEY,
 meeting_id TEXT REFERENCES meetings(id) ON DELETE CASCADE,
 title TEXT NOT NULL,
 owner TEXT NOT NULL DEFAULT '',
 due_date TEXT,
 status TEXT NOT NULL DEFAULT 'open' CHECK(status IN ('open','done','cancelled')),
 evidence_json TEXT NOT NULL DEFAULT '[]',
 created_at TEXT NOT NULL,
 updated_at TEXT NOT NULL
);
CREATE INDEX workspace_tasks_due ON workspace_tasks(status,due_date);
CREATE TABLE workspace_answers (
 id TEXT PRIMARY KEY,
 question TEXT NOT NULL,
 scope_json TEXT NOT NULL,
 answer_json TEXT NOT NULL,
 created_at TEXT NOT NULL
);
CREATE TABLE workspace_vectors (
 document_id TEXT NOT NULL REFERENCES workspace_documents(id) ON DELETE CASCADE,
 model_revision TEXT NOT NULL,
 text_hash TEXT NOT NULL,
 vector_json TEXT NOT NULL,
 PRIMARY KEY(document_id,model_revision)
);
CREATE TABLE workspace_calendar (
 id TEXT PRIMARY KEY,
 source TEXT NOT NULL,
 uid TEXT NOT NULL,
 recurrence_id TEXT NOT NULL DEFAULT '',
 title TEXT NOT NULL,
 description TEXT NOT NULL DEFAULT '',
 starts_at TEXT NOT NULL,
 ends_at TEXT NOT NULL,
 timezone TEXT NOT NULL,
 all_day INTEGER NOT NULL DEFAULT 0,
 location TEXT NOT NULL DEFAULT '',
 meeting_url TEXT,
 UNIQUE(source,uid,recurrence_id)
);
