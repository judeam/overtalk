CREATE TABLE speech_transcript_backups (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    meeting_id TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
    created_at TEXT NOT NULL,
    transcripts TEXT NOT NULL,
    speakers TEXT NOT NULL DEFAULT '[]',
    turns TEXT NOT NULL DEFAULT '[]'
);
CREATE INDEX speech_backups_meeting ON speech_transcript_backups(meeting_id, id DESC);
