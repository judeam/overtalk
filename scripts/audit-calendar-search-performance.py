#!/usr/bin/env python3
"""Measure calendar import and library browsing in a disposable synthetic library."""
from service_client import Service
import argparse
import json
from pathlib import Path
import sqlite3
import statistics
import tempfile
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    result = {}
    with tempfile.TemporaryDirectory(prefix="overtalk-calendar-search-") as temp:
        root = Path(temp)
        service = Service(args.binary.resolve(), root)
        try:
            database = root / "data/com.judeam.sonefold/meeting_minutes.sqlite"
            with sqlite3.connect(database) as db:
                for i in range(200):
                    date = f"2026-10-{i % 28 + 1:02}T10:00:00Z"
                    db.execute("INSERT INTO meetings(id,title,created_at,updated_at) VALUES(?,?,?,?)",
                               (f"m{i}", f"Synthetic meeting {i}", date, date))
                    db.executemany("INSERT INTO transcripts(id,meeting_id,transcript,timestamp,"
                                   "audio_start_time,audio_end_time) VALUES(?,?,?,'00:00',?,?)",
                                   [(f"m{i}-{j}", f"m{i}", "Synthetic calendar and project review. " * 4,
                                     j * 2, j * 2 + 2) for j in range(250)])
            for label, request in [
                ("recent_library", dict(query="", semantic=False, limit=24)),
                ("scoped_library", dict(query="", semantic=False, limit=24, meeting_id="m42")),
                ("keyword_search", dict(query="project", semantic=False, limit=24)),
            ]:
                samples = []
                previous = None
                for _ in range(9):
                    start = time.perf_counter()
                    response = service.call("workspace_search", request=request)
                    samples.append((time.perf_counter() - start) * 1000)
                    ids = [s["id"] for s in response["sources"]]
                    assert len(ids) == 24 and (previous is None or previous == ids)
                    previous = ids
                result[label] = dict(median_ms=statistics.median(samples), max_ms=max(samples), ids=previous)
            # Use a current date so the same fixture remains in the import window.
            from datetime import datetime, timezone
            date = datetime.now(timezone.utc).strftime("%Y%m%dT100000Z")
            calendar = root / "synthetic.ics"
            calendar.write_text("BEGIN:VCALENDAR\nVERSION:2.0\n" + "".join(
                f"BEGIN:VEVENT\nUID:series-{i}\nDTSTART:{date}\nRRULE:FREQ=HOURLY;COUNT=500\n"
                f"SUMMARY:Synthetic review {i}\nDESCRIPTION:" + "Agenda item. " * 100 +
                " https://meet.google.com/abc-defg-hij\nEND:VEVENT\n" for i in range(6)
            ) + "END:VCALENDAR\n")
            samples = []
            for _ in range(3):
                start = time.perf_counter()
                response = service.call("workspace_import_calendar", path=str(calendar))
                samples.append((time.perf_counter() - start) * 1000)
                assert response["count"] == 3000
            result["calendar_3000_occurrences"] = dict(median_ms=statistics.median(samples), max_ms=max(samples))
        finally:
            service.close()
    args.output.write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
