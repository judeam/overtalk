-- The person's own usage report: counts of what they did, by week, and when each
-- thing first happened. No titles, names, text or audio. Nothing here is sent anywhere;
-- the person can save the report as a file and decide whether to share it.
CREATE TABLE usage_counts (
    event TEXT PRIMARY KEY,
    count INTEGER NOT NULL DEFAULT 0,
    first_at TEXT NOT NULL,
    last_at TEXT NOT NULL
);
CREATE TABLE usage_weeks (
    week TEXT NOT NULL,
    event TEXT NOT NULL,
    count INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (week, event)
);
-- Events that count once per meeting or calendar event (a transcript, notes, a brief),
-- however often the work behind them runs.
CREATE TABLE usage_once (
    event TEXT NOT NULL,
    subject TEXT NOT NULL,
    PRIMARY KEY (event, subject)
);
