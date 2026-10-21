-- Moments the listener marked while recording or reviewing, in recording-relative seconds.
CREATE TABLE meeting_marks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    meeting_id TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
    at REAL NOT NULL CHECK (at >= 0),
    label TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE INDEX meeting_marks_meeting ON meeting_marks(meeting_id, at);
