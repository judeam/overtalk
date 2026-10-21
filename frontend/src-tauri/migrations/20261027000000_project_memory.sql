-- Automatic filing and briefs keep two small bookkeeping tables: which meetings were
-- already decided (so each is decided once), and which reminders were already shown.
CREATE TABLE IF NOT EXISTS filing_log (
    meeting_id TEXT PRIMARY KEY,
    outcome TEXT NOT NULL,
    project_id TEXT,
    reason TEXT NOT NULL DEFAULT '',
    decided_at TEXT NOT NULL
);
CREATE TRIGGER IF NOT EXISTS filing_log_delete AFTER DELETE ON meetings BEGIN
    DELETE FROM filing_log WHERE meeting_id = OLD.id;
END;
CREATE TABLE IF NOT EXISTS brief_reminders (
    event_id TEXT PRIMARY KEY,
    notified_at TEXT NOT NULL
);
