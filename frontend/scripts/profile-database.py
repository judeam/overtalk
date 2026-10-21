"""Reproducible SQLite query timings using synthetic meetings, never user data."""
import json
import pathlib
import sqlite3
import statistics
import time

root = pathlib.Path(__file__).resolve().parents[1]
db = sqlite3.connect(":memory:")
for migration in sorted((root / "src-tauri/migrations").glob("*.sql")):
    if migration.name == "20261016000000_transcript_read_indexes.sql":
        continue  # Reproduce the pre-audit schema before applying the measured indexes.
    db.executescript(migration.read_text())
with db:
    db.executemany("INSERT INTO meetings(id,title,created_at,updated_at) VALUES(?,?,?,?)", ((f"m{i}", f"Meeting {i}", "2026-10-14", "2026-10-14") for i in range(2000)))
    db.executemany("INSERT INTO transcripts(id,meeting_id,transcript,timestamp,audio_start_time,audio_end_time) VALUES(?,?,?,?,?,?)", ((f"t{m}_{i}", f"m{m}", "Synthetic transcript for a repeatable benchmark", "2026-10-14", i * 3, i * 3 + 2) for m in range(2000) for i in range(30000 if m == 0 else 100)))

page = "SELECT t.*, s.name AS speaker_name FROM transcripts t LEFT JOIN meeting_speakers s ON s.meeting_id=t.meeting_id AND s.id=t.speaker_id WHERE t.meeting_id=? ORDER BY t.audio_start_time ASC,t.id LIMIT 100 OFFSET 10000"
compact = """WITH turn_ends AS (SELECT meeting_id, MAX(end_time) AS duration FROM speaker_turns GROUP BY meeting_id),
transcript_ends AS (SELECT meeting_id, MAX(audio_end_time) AS duration FROM transcripts GROUP BY meeting_id)
SELECT m.id,m.title,m.created_at,MAX(0.0,COALESCE(t.duration,0.0),COALESCE(s.duration,0.0)) FROM meetings m
LEFT JOIN turn_ends t ON t.meeting_id=m.id LEFT JOIN transcript_ends s ON s.meeting_id=m.id
WHERE NOT EXISTS(SELECT 1 FROM meeting_workspace_state ws WHERE ws.meeting_id=m.id AND ws.trashed_at IS NOT NULL) ORDER BY m.created_at DESC"""
indexed_compact = """SELECT m.id,m.title,m.created_at,MAX(0.0,
COALESCE((SELECT MAX(end_time) FROM speaker_turns WHERE meeting_id=m.id),0.0),
COALESCE((SELECT MAX(audio_end_time) FROM transcripts WHERE meeting_id=m.id),0.0)) FROM meetings m
WHERE NOT EXISTS(SELECT 1 FROM meeting_workspace_state ws WHERE ws.meeting_id=m.id AND ws.trashed_at IS NOT NULL) ORDER BY m.created_at DESC"""

def measure(sql, args=()):
    durations = []
    for _ in range(7):
        start = time.perf_counter()
        rows = db.execute(sql, args).fetchall()
        durations.append((time.perf_counter() - start) * 1000)
    return {"median_ms": round(statistics.median(durations), 3), "rows": len(rows), "plan": [r[3] for r in db.execute("EXPLAIN QUERY PLAN " + sql, args)]}

result = {"transcripts": db.execute("SELECT COUNT(*) FROM transcripts").fetchone()[0], "page": measure(page, ("m0",)), "overview": measure(compact)}
# Show the same workload with proposed covering indexes, including their write cost in the audit.
pages_before = db.execute("PRAGMA page_count").fetchone()[0]
index_start = time.perf_counter()
db.executescript("CREATE INDEX IF NOT EXISTS transcripts_meeting_time ON transcripts(meeting_id,audio_start_time,id); CREATE INDEX IF NOT EXISTS transcripts_meeting_end ON transcripts(meeting_id,audio_end_time); CREATE INDEX IF NOT EXISTS speaker_turns_meeting_end ON speaker_turns(meeting_id,end_time);")
result["index_build_ms"] = round((time.perf_counter() - index_start) * 1000, 2)
result["index_bytes"] = (db.execute("PRAGMA page_count").fetchone()[0] - pages_before) * db.execute("PRAGMA page_size").fetchone()[0]
assert db.execute(compact).fetchall() == db.execute(indexed_compact).fetchall(), "Duration results changed"
result["indexed_page"] = measure(page, ("m0",))
result["indexed_overview"] = measure(indexed_compact)
print(json.dumps(result, indent=2))
