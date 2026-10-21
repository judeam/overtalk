#!/usr/bin/env python3
"""Verify hostile calendar and notes handling through a disposable real service."""
from service_client import Service
import argparse
from datetime import datetime, timezone
import json
from pathlib import Path
import sqlite3
import tempfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--report", type=Path)
    args = parser.parse_args()
    checks = {}
    with tempfile.TemporaryDirectory(prefix="overtalk-import-boundaries-") as temp:
        root = Path(temp)
        service = Service(args.binary.resolve(), root)
        database = root / "data/com.judeam.sonefold/meeting_minutes.sqlite"
        try:
            calendar = root / "calendar.ics"
            date = datetime.now(timezone.utc).strftime("%Y%m%dT100000Z")

            def content(extra):
                return ("BEGIN:VCALENDAR\nVERSION:2.0\nBEGIN:VEVENT\nUID:synthetic\n"
                        f"DTSTART:{date}\nSUMMARY:Safe original\n{extra}"
                        "END:VEVENT\nEND:VCALENDAR\n")

            calendar.write_text(content(""))
            assert service.call("workspace_import_calendar", path=str(calendar))["count"] == 1
            with sqlite3.connect(database) as db:
                original = db.execute("SELECT * FROM workspace_calendar").fetchall()
            for name, extra in [
                ("field_limit", "DESCRIPTION:" + "x" * (65 * 1024) + "\n"),
                ("expansion_limit", "RRULE:FREQ=HOURLY;COUNT=512\nDESCRIPTION:" + "x" * (40 * 1024) + "\n"),
            ]:
                calendar.write_text(content(extra))
                try:
                    service.call("workspace_import_calendar", path=str(calendar))
                except RuntimeError:
                    pass
                else:
                    raise AssertionError(f"Accepted unsafe calendar: {name}")
                with sqlite3.connect(database) as db:
                    assert db.execute("SELECT * FROM workspace_calendar").fetchall() == original
                checks[name + "_preserves_calendar"] = "passed"
            calendar.write_text(content(""))
            assert service.call("workspace_import_calendar", path=str(calendar))["count"] == 1
            checks["failed_import_releases_admission"] = "passed"

            meeting = service.call("api_save_transcript", meetingTitle="Synthetic document",
                folderPath=None, transcripts=[dict(id="synthetic", text="Original passage",
                timestamp="00:00", audio_start_time=0., audio_end_time=1., duration=1.)])["meeting_id"]
            rows = [{"cells": [""] * 512}] + [{"cells": []} for _ in range(512)]
            notes = {"summary_json": [{"type": "table", "content": {"rows": rows}}]}
            service.call("api_save_meeting_summary", meetingId=meeting, summary={"markdown": "Safe original notes"})
            try:
                service.call("api_save_meeting_summary", meetingId=meeting, summary=notes)
            except RuntimeError:
                pass
            else:
                raise AssertionError("Unsafe notes replaced the saved document")
            with sqlite3.connect(database) as db:
                assert "Safe original notes" in db.execute("SELECT result FROM summary_processes WHERE meeting_id=?", (meeting,)).fetchone()[0]
                # Simulate a legacy/restored document in this disposable library.
                db.execute("UPDATE summary_processes SET result=? WHERE meeting_id=?", (json.dumps(notes), meeting))
                original_notes = db.execute("SELECT result FROM summary_processes WHERE meeting_id=?", (meeting,)).fetchone()
            checks["unsafe_save_preserves_previous_notes"] = "passed"
            assert "Original passage" in service.call("native_transcript_text", meetingId=meeting)
            checks["transcript_copy_independent_of_bad_notes"] = "passed"
            try:
                service.call("workspace_review", action={"op": "list", "meeting_id": meeting})
            except RuntimeError as error:
                assert "table" in str(error)
            else:
                raise AssertionError("Unsafe structured notes were rendered")
            with sqlite3.connect(database) as db:
                assert db.execute("SELECT result FROM summary_processes WHERE meeting_id=?", (meeting,)).fetchone() == original_notes
            checks["unsafe_notes_rejected_original_preserved"] = "passed"
            service.call("api_save_meeting_summary", meetingId=meeting, summary={"markdown": "Reviewed notes"})
            assert "Original passage" in service.call("native_transcript_text", meetingId=meeting)
            checks["service_usable_after_rejection"] = "passed"
        finally:
            service.close()
    if args.report:
        args.report.write_text(json.dumps(checks, indent=2) + "\n")
    print(json.dumps(checks, indent=2))


if __name__ == "__main__":
    main()
