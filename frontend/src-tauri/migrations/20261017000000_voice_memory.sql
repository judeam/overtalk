-- Opt-in local voice memory. Embeddings never leave the device.
CREATE TABLE voice_profiles (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL COLLATE NOCASE UNIQUE CHECK(length(name) BETWEEN 1 AND 120),
    model TEXT NOT NULL,
    embeddings TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE TABLE meeting_voice_matches (
    meeting_id TEXT NOT NULL,
    speaker_id TEXT NOT NULL,
    profile_id TEXT NOT NULL REFERENCES voice_profiles(id) ON DELETE CASCADE,
    similarity REAL NOT NULL,
    confirmed INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY(meeting_id,speaker_id),
    FOREIGN KEY(meeting_id,speaker_id) REFERENCES meeting_speakers(meeting_id,id) ON DELETE CASCADE
);
