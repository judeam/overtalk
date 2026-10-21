"""Exercise project workflows through the real typed service in a disposable profile.

No provider credentials, user meetings, external writes or microphone access.
"""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import uuid
import wave
import sqlite3
import statistics
import tempfile
import time


from service_client import Service


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--report", type=Path)
    args = parser.parse_args()
    report = {}
    with tempfile.TemporaryDirectory(prefix="overtalk-project-check-") as temporary:
        root = Path(temporary)
        service = Service(args.binary.resolve(), root)
        try:
            database = root / "data/com.judeam.sonefold/meeting_minutes.sqlite"
            with sqlite3.connect(database) as db:
                db.executescript("""
                PRAGMA foreign_keys=ON;
                INSERT INTO meetings(id,title,created_at,updated_at) VALUES
                 ('one','Synthetic project meeting','2026-10-15','2026-10-15'),
                 ('two','Synthetic separate project','2026-10-15','2026-10-15');
                INSERT INTO transcripts(id,meeting_id,transcript,timestamp,audio_start_time,audio_end_time)
                 VALUES('line','one','Launch is on Friday.','00:00',0,4);
                INSERT INTO meeting_speakers(meeting_id,id,name) VALUES('one','A','Alex');
                """)
            project = service.call("workspace_project", action=dict(op="save", name="Launch", context="Synthetic context"))["id"]
            other = service.call("workspace_project", action=dict(op="save", name="Other", context="Do not mix projects"))["id"]
            for meeting, chosen in [("one", project), ("two", other)]:
                service.call("workspace_project", action=dict(op="assign", meeting_id=meeting, project_id=chosen, series="Weekly"))
                service.call("workspace_project", action=dict(op="person", project_id=chosen, name="Alex", role="Engineering"))
            service.call("workspace_project", action=dict(op="term", project_id=project, term="Overtalk", meaning="Meeting notebook"))
            snapshot = service.call("workspace_project", action=dict(op="get", id=project))
            assert len(snapshot["people"]) == 1 and len(snapshot["meetings"]) == 1
            assert snapshot["meetings"][0]["id"] == "one"
            report["project_isolation"] = "passed"

            hits = service.call("workspace_search", request=dict(query="Launch", project_id=other))
            assert not hits["sources"], hits
            report["project_search_scope"] = "passed"
            issues = service.call("workspace_review", action=dict(op="list", project_id=project))
            assert any(i["segment_id"] == "line" for i in issues)
            service.call("workspace_correct_transcript", change=dict(meeting_id="one", segment_id="line", text="Launch is on Friday.",
                         speaker_id="A", expected_text="Launch is on Friday.", expected_speaker_id=None))
            assert not service.call("workspace_review", action=dict(op="list", project_id=project))
            report["review_correction"] = "passed"

            for text in ["First synthetic notes", "Second synthetic notes"]:
                service.call("api_save_meeting_summary", meetingId="one", summary=dict(markdown=text))
            versions = service.call("workspace_review", action=dict(op="versions", meeting_id="one"))
            service.call("workspace_review", action=dict(op="restore", meeting_id="one", version_id=versions["versions"][-1]["id"], expected_hash=versions["current_hash"]))
            assert "First synthetic notes" in json.dumps(service.call("api_get_summary", meetingId="one"))
            report["notes_history_restore"] = "passed"

            task = service.call("workspace_save_task", input=dict(meeting_id="one", title="Synthetic task", owner="Alex", due_date="2026-01-01", status="open"))
            service.call("workspace_review", action=dict(op="remind", task_id=task, at="2026-01-01T10:00:00Z"))
            assert any(t["id"] == task for t in service.call("workspace_review", action=dict(op="due")))
            report["reminder_schedule"] = "passed"
            service.call("workspace_jobs", action=dict(op="pause", paused=True))
            job = service.call("workspace_jobs", action=dict(op="submit", kind="translate", meeting_id="one", language="German"))["id"]
            assert any(j["id"] == job and j["state"] == "queued" for j in service.call("workspace_jobs", action=dict(op="list")))
            service.call("workspace_jobs", action=dict(op="cancel", id=job))
            report["durable_job_queue"] = "passed"

            # Exercise the same open path with a long transcript and a full
            # page of evidence, not just the one-line smoke-test fixture.
            with sqlite3.connect(database) as db:
                lines = [(f"bulk-{i}", "one", f"Synthetic evidence {i}: confirmed launch constraint.", f"{i//60:02}:{i%60:02}", i, i+1) for i in range(5000)]
                db.executemany("INSERT INTO transcripts(id,meeting_id,transcript,timestamp,audio_start_time,audio_end_time) VALUES(?,?,?,?,?,?)", lines)
                facts = [(f"fact-{i}", project, f"Constraint {i}", "accepted", "one", f"bulk-{i}", row[2], hashlib.sha256(row[2].encode()).hexdigest(), "2026-10-15", "2026-10-15") for i,row in enumerate(lines[:300])]
                db.executemany("INSERT INTO project_facts(id,project_id,body,status,source_meeting_id,source_segment_id,source_quote,source_hash,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?,?,?)", facts)
            times = []
            for _ in range(40):
                before = time.perf_counter()
                page = service.call("workspace_project", action=dict(op="get", id=project))
                assert len(page["facts"]) == 300
                times.append((time.perf_counter() - before) * 1000)
            report["project_open_5000_passages_300_facts_ms"] = dict(median=round(statistics.median(times), 2), p95=round(sorted(times)[37], 2))
            assert sorted(times)[37] < 250, "Project opening exceeded the CI latency budget"
            archive = root / "synthetic.age"
            service.call("workspace_create_backup", path=str(archive), passphrase="synthetic-test-only-passphrase", includeAudio=False)
            assert archive.read_bytes().startswith(b"age-encryption.org")
            result = service.call("workspace_schedule_restore", path=str(archive), passphrase="synthetic-test-only-passphrase")
            assert result["restart_required"]
            service.close()
            service = Service(args.binary.resolve(), root)
            assert service.call("workspace_project", action=dict(op="get", id=project))["project"]["name"] == "Launch"
            assert list(database.parent.glob("before-restore-*"))
            report["encrypted_backup_and_restart_restore"] = "passed"
            # Recover an interrupted synthetic video and attach its audio using
            # the production recovery/remux path, without capturing a screen.
            pending = database.parent / "pending-video"
            pending.mkdir(exist_ok=True)
            recording = root / "video-fixture"
            recording.mkdir()
            with wave.open(str(recording / "audio.wav"), "wb") as audio:
                audio.setparams((1, 2, 16000, 0, "NONE", "not compressed"))
                audio.writeframes(b"\0\0" * 8000)
            clip = str(uuid.uuid4())
            ffmpeg = shutil.which("ffmpeg")
            assert ffmpeg, "FFmpeg is required for the video recovery test"
            subprocess.run([ffmpeg, "-nostdin", "-v", "error", "-f", "lavfi", "-i", "color=c=black:s=64x64:r=15:d=0.5", "-c:v", "libx264", "-threads", "1", "-pix_fmt", "yuv420p", "-movflags", "+frag_keyframe+empty_moov", str(pending / f"{clip}.mp4")], check=True)
            (pending / f"{clip}.json").write_text(json.dumps(dict(id=clip, meeting_id="one", offset=0., ready=True)))
            with sqlite3.connect(database) as db:
                db.execute("UPDATE meetings SET folder_path=? WHERE id='one'", (str(recording),))
            service.close()
            service = Service(args.binary.resolve(), root)
            deadline = time.monotonic() + 15
            while time.monotonic() < deadline:
                if service.call("workspace_video", action=dict(op="status", meeting_id="one"))["has_video"]:
                    break
                time.sleep(.1)
            output = recording / f"video-{clip}.mp4"
            assert output.is_file(), "Recovered video was not attached"
            streams = json.loads(subprocess.check_output([shutil.which("ffprobe"), "-v", "error", "-show_entries", "stream=codec_type", "-of", "json", str(output)]))["streams"]
            assert {s["codec_type"] for s in streams} == {"audio", "video"}
            assert not (pending / f"{clip}.json").exists()
            report["video_recovery_and_audio_mux"] = "passed"
        finally:
            service.close()
    print(json.dumps(report, indent=2))
    if args.report:
        args.report.parent.mkdir(parents=True, exist_ok=True)
        args.report.write_text(json.dumps(report, indent=2) + "\n")


if __name__ == "__main__":
    main()
