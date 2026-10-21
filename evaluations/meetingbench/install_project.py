"""Import the frozen 200-meeting results into a dedicated local app project.

Dry run by default. --apply uses the native service, with a SQLite backup and
per-meeting recovery markers. No inference, reference transcripts or fabricated
speaker names are substituted for the saved model results.
"""
import argparse
import datetime as dt
import hashlib
import json
import os
from pathlib import Path
import queue
import shutil
import sqlite3
import subprocess
import threading

ROOT = Path(__file__).resolve().parents[2]
PROJECT_ID = "benchmark-meetingbench-200-v1"
CONTEXT = (
    "Synthetic meeting benchmark: 100 normal, 60 hard and 40 very hard meetings. "
    "Audio is generated speech. Transcripts and speaker assignments are the saved "
    "Qwen3-ASR 1.7B / Nemotron results; notes are Qwen3.5 4B generated from those "
    "transcripts. Errors are retained for inspection. Names such as Speaker 1 are "
    "model clusters, not verified identities. These conversations are synthetic "
    "test data, not facts about real projects or people."
)


def digest(path):
    with Path(path).open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def atomic_json(path, value):
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n")
    temporary.replace(path)


def load_cases(run):
    manifest = json.loads((run / "manifest.json").read_text())
    if len(manifest) != 200 or len({x["id"] for x in manifest}) != 200:
        raise ValueError("Expected the original 200 unique benchmark cases")
    cases = []
    counts = {}
    for item in manifest:
        case_id = item["id"]
        if Path(case_id).name != case_id:
            raise ValueError("Invalid case ID")
        audio = Path(item["audio_path"])
        reference = json.loads((audio.parent / "reference.json").read_text())
        transcript = run / "results" / case_id / "transcript.json"
        notes = run / "notes" / f"{case_id}-asr__qwen3.5-4b__app.md"
        if not audio.is_file() or not transcript.is_file() or not notes.is_file():
            raise ValueError(f"Incomplete result: {case_id}")
        content = notes.read_text()
        speech = json.loads(transcript.read_text())
        if not speech["rows"] or not content.strip() or content.startswith("ERROR:"):
            raise ValueError(f"Failed result: {case_id}")
        counts[reference["difficulty"]] = counts.get(reference["difficulty"], 0) + 1
        cases.append(dict(id=case_id, audio=audio, reference=reference,
                          transcript=transcript, speech=speech, notes=notes))
    if counts != {"normal": 100, "hard": 60, "really_hard": 40}:
        raise ValueError(f"Unexpected difficulty distribution: {counts}")
    return cases


def segments(speech):
    names = {s["id"]: s["name"] for s in speech["speakers"]}
    result = []
    for index, row in enumerate(speech["rows"]):
        start, end = row["audio_start_time"], row["audio_end_time"]
        result.append(dict(id=f"import-{index}", text=row["text"],
            timestamp=f"{int(start)//3600:02}:{int(start)//60%60:02}:{int(start)%60:02}",
            audio_start_time=start, audio_end_time=end, duration=end-start,
            speaker=None, speaker_id=row.get("speaker_id"),
            speaker_name=names.get(row.get("speaker_id")),
            speaker_confidence=row.get("speaker_confidence"),
            overlap=row.get("overlap", False), is_partial=False))
    return result


class Engine:
    def __init__(self, install, log):
        env = dict(os.environ, APPDIR=str(install),
            SONEFOLD_LLAMA_HELPER=str(install / "bin/llama-helper"))
        env["PATH"] = str(install / "bin") + os.pathsep + env.get("PATH", "")
        env["LD_LIBRARY_PATH"] = str(install / "lib") + os.pathsep + env.get("LD_LIBRARY_PATH", "")
        env.setdefault("DISPLAY", ":1")
        env.setdefault("DBUS_SESSION_BUS_ADDRESS", f"unix:path=/run/user/{os.getuid()}/bus")
        self.process = subprocess.Popen([str(install / "bin/sonefold-service"), "--stdio"],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=log, env=env)
        self.messages, self.serial = queue.Queue(), 0
        def receive():
            for line in self.process.stdout:
                if line.startswith(b"OTRPC1 "):
                    self.messages.put(json.loads(line[7:]))
            self.messages.put({"engine_exited": True})
        threading.Thread(target=receive, daemon=True).start()
        while True:
            message = self.messages.get(timeout=120)
            if message.get("engine_exited"):
                raise RuntimeError("Native service exited before ready")
            if message.get("kind") == "ready":
                break

    def call(self, command, **args):
        self.serial += 1
        self.process.stdin.write(b"OTRPC1 " + json.dumps(dict(id=self.serial,
            command=command, args=args)).encode() + b"\n")
        self.process.stdin.flush()
        while True:
            message = self.messages.get(timeout=120)
            if message.get("engine_exited"):
                raise RuntimeError("Native service exited during import")
            if message.get("id") == self.serial:
                if message.get("error"):
                    raise RuntimeError(f"{command}: {message['error']}")
                return message["result"]

    def close(self):
        try:
            self.process.stdin.write(b'OTRPC1 {"id":999999,"command":"shutdown","args":{}}\n')
            self.process.stdin.flush()
            self.process.wait(timeout=20)
        except (Exception,):
            self.process.terminate()
            self.process.wait(timeout=20)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--run", type=Path, default=ROOT / "evaluations/meetingbench/.runtime/experiments/fast-full200-v2")
    parser.add_argument("--install", type=Path, default=Path.home() / ".local/opt/sonefold/slint/current")
    parser.add_argument("--apply", action="store_true")
    args = parser.parse_args()
    os.umask(0o077)
    cases = load_cases(args.run)
    print(json.dumps(dict(cases=len(cases), audio_bytes=sum(c["audio"].stat().st_size for c in cases),
                          apply=args.apply)), flush=True)
    if not args.apply:
        return
    profile = Path.home() / ".local/share/com.judeam.sonefold"
    database = profile / "meeting_minutes.sqlite"
    stamp = dt.datetime.now(dt.timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    backup = profile / "backups" / f"before-benchmark-import-{stamp}.sqlite"
    backup.parent.mkdir(parents=True, exist_ok=True)
    with sqlite3.connect(f"file:{database}?mode=ro", uri=True) as source:
        with sqlite3.connect(backup) as target:
            source.backup(target)
            if target.execute("PRAGMA integrity_check").fetchone()[0] != "ok":
                raise RuntimeError("Backup integrity check failed")
        # A recovery lookup works even if a previous import stopped before
        # writing its marker. Never replace existing user-edited notes.
        existing = dict(source.execute("SELECT folder_path,id FROM meetings WHERE folder_path IS NOT NULL"))
    state = args.run / "app-project-import"
    state.mkdir(exist_ok=True)
    with (state / f"engine-{stamp}.log").open("w") as log:
        engine = Engine(args.install.resolve(), log)
        try:
            if engine.call("is_recording") or engine.call("is_import_in_progress_command"):
                raise RuntimeError("Recording or audio import is active; retry after it finishes")
            preferences = engine.call("get_recording_preferences")
            root = Path(preferences.get("save_folder") or Path.home() / "Documents/sonefold-recordings") / "MeetingBench-200"
            root.mkdir(parents=True, exist_ok=True)
            missing_bytes = sum(c["audio"].stat().st_size for c in cases if not (root/c["id"]/"audio.wav").exists())
            if shutil.disk_usage(root).free < missing_bytes + 1024**3:
                raise RuntimeError("Insufficient disk space to copy benchmark audio safely")
            engine.call("workspace_project", action=dict(op="save", id=PROJECT_ID,
                name="MeetingBench · 200 test conversations", context=CONTEXT))
            imported = []
            for case in cases:
                folder = root / case["id"]
                marker = state / (case["id"] + ".json")
                folder.mkdir(exist_ok=True)
                audio = folder / "audio.wav"
                if not audio.exists():
                    temporary = folder / "audio.wav.partial"
                    shutil.copyfile(case["audio"], temporary)
                    if digest(temporary) != digest(case["audio"]):
                        raise RuntimeError("Copied audio checksum mismatch")
                    temporary.replace(audio)
                audio_digest = digest(audio)
                if audio.stat().st_size != case["audio"].stat().st_size or audio_digest != digest(case["audio"]):
                    raise RuntimeError(f"Existing audio differs: {case['id']}")
                meeting = existing.get(str(folder))
                resumed = meeting is not None
                if meeting is None:
                    saved = engine.call("api_save_transcript", meetingTitle=case["reference"]["title"],
                        transcripts=segments(case["speech"]), speakerTurns=case["speech"]["turns"], folderPath=str(folder))
                    meeting = saved["meeting_id"]
                    existing[str(folder)] = meeting
                prior = json.loads(marker.read_text()) if marker.exists() else {}
                if not prior.get("notes_saved"):
                    # An existing summary could be a user's edit after an interrupted import.
                    current = engine.call("api_get_summary", meetingId=meeting)
                    if not resumed or not current.get("data"):
                        engine.call("api_save_meeting_summary", meetingId=meeting,
                            summary=dict(markdown=case["notes"].read_text(), model="qwen3.5:4b",
                                         provenance="saved-benchmark-asr-output"))
                engine.call("workspace_set_tags", meetingId=meeting, tags=["synthetic", "meetingbench-200",
                    case["reference"]["difficulty"], *case["reference"]["languages"]])
                engine.call("workspace_project", action=dict(op="assign", meeting_id=meeting,
                    project_id=PROJECT_ID, series=case["reference"]["difficulty"]))
                provenance = dict(case_id=case["id"], meeting_id=meeting, notes_saved=True,
                    transcript_sha256=digest(case["transcript"]), notes_sha256=digest(case["notes"]),
                    audio_sha256=audio_digest, source_run=str(args.run.resolve()),
                    speaker_labels="model clusters, not reference identities")
                atomic_json(folder / "benchmark-provenance.json", provenance)
                atomic_json(marker, provenance)
                imported.append(provenance)
                if len(imported) % 20 == 0:
                    print(json.dumps(dict(imported=len(imported), total=200)), flush=True)
            project = engine.call("workspace_project", action=dict(op="get", id=PROJECT_ID))
            if {m["id"] for m in project["meetings"]} != {m["meeting_id"] for m in imported}:
                raise RuntimeError("Project membership did not match all 200 imported meetings")
            atomic_json(state / "completed.json", dict(project_id=PROJECT_ID, backup=str(backup),
                meetings=imported, project=project))
            print(json.dumps(dict(project_id=PROJECT_ID, imported=len(imported), backup=str(backup))), flush=True)
        finally:
            engine.close()


if __name__ == "__main__":
    main()
