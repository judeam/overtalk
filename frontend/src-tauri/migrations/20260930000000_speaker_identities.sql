-- Audio-source attribution (transcripts.speaker) remains independent of people.
ALTER TABLE transcripts ADD COLUMN speaker_id TEXT;
ALTER TABLE transcripts ADD COLUMN speaker_confidence REAL;
ALTER TABLE transcripts ADD COLUMN overlap INTEGER NOT NULL DEFAULT 0;

CREATE TABLE meeting_speakers (
    meeting_id TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
    id TEXT NOT NULL,
    name TEXT NOT NULL,
    PRIMARY KEY (meeting_id, id)
);

-- Preserve all overlapping model turns, independently of transcript grouping.
CREATE TABLE speaker_turns (
    meeting_id TEXT NOT NULL,
    speaker_id TEXT NOT NULL,
    start_time REAL NOT NULL CHECK (start_time >= 0),
    end_time REAL NOT NULL CHECK (end_time > start_time),
    confidence REAL,
    model TEXT NOT NULL,
    FOREIGN KEY (meeting_id, speaker_id) REFERENCES meeting_speakers(meeting_id, id) ON DELETE CASCADE
);
CREATE INDEX speaker_turns_meeting_time ON speaker_turns(meeting_id, start_time);
CREATE INDEX transcripts_meeting_speaker ON transcripts(meeting_id, speaker_id);
