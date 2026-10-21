"""Add the requested synthetic meeting through the installed native app engine.

Run explicitly: this writes one new meeting to the user's real library. Existing
meetings and model preferences are preserved. Audio is generated beforehand.
"""
import datetime
import argparse
import json
import os
from pathlib import Path
import queue
import shutil
import sqlite3
import subprocess
import threading
import time

ROOT = Path(__file__).resolve().parents[2]
OUTPUT = ROOT / "evaluations/meetingbench/.runtime/very-hard-preview"


def reference_display_names(script, turns):
    """Reference-only listening labels, never model identity predictions.

    Leave merged or ambiguous voices anonymous. Benchmark snapshots are written
    before these annotations and retain the model's actual anonymous IDs.
    """
    scores = {}
    for speaker, start, end in turns:
        person_scores = scores.setdefault(speaker, {})
        for source in script["turns"]:
            overlap = max(0, min(end, source["end_seconds"]) - max(start, source["start_seconds"]))
            person_scores[source["speaker"]] = person_scores.get(source["speaker"], 0) + overlap
    people = {person["id"]: person["name"] for person in script["people"]}
    labels = []
    for speaker, amounts in scores.items():
        ranking = sorted(amounts.items(), key=lambda pair: pair[1], reverse=True)
        person, seconds = ranking[0]
        total = sum(amounts.values())
        if seconds >= 5 and total and seconds / total >= .90:
            labels.append({"speaker_id": speaker, "name": people[person], "reference_purity": seconds / total,
                           "source": "synthetic reference annotation after inference, not automatic name recognition"})
    return labels


def main():
    global OUTPUT
    parser = argparse.ArgumentParser()
    parser.add_argument("--case", default="very-hard-preview")
    args = parser.parse_args()
    if Path(args.case).name != args.case:
        raise ValueError("Case must be a single directory name")
    OUTPUT = OUTPUT.parent / args.case
    os.umask(0o077)
    until = time.monotonic() + 600
    while not (OUTPUT / "reference.json").exists():
        if time.monotonic() > until:
            raise RuntimeError("The synthetic audio has not finished; library not changed")
        time.sleep(1)
    if (OUTPUT / "installed.json").exists():
        print((OUTPUT / "installed.json").read_text())
        return
    script = json.loads((OUTPUT / "reference.json").read_text())
    profile = Path.home() / ".local/share/com.judeam.sonefold"
    settings = json.loads((profile / "speech-runtime.json").read_text())
    if settings["model"] != "qwen3-asr-1.7b":
        raise RuntimeError("App speech model differs from the requested model; no library change made")
    stamp = datetime.datetime.now().strftime("%Y%m%dT%H%M%S")
    import tempfile
    backup = Path(tempfile.mkdtemp(prefix="overtalk-before-synthetic-preview-")) / "meeting.sqlite"
    with sqlite3.connect(f"file:{profile}/meeting_minutes.sqlite?mode=ro", uri=True) as source:
        with sqlite3.connect(backup) as target:
            source.backup(target)
    backup.chmod(0o600)
    install = (Path.home() / ".local/opt/sonefold/slint/current").resolve()
    env = dict(os.environ, APPDIR=str(install), SONEFOLD_LLAMA_HELPER=str(install / "bin/llama-helper"))
    env["PATH"] = str(install / "bin") + os.pathsep + env["PATH"]
    env["LD_LIBRARY_PATH"] = str(install / "lib") + (os.pathsep + env["LD_LIBRARY_PATH"] if env.get("LD_LIBRARY_PATH") else "")
    env.setdefault("DISPLAY", ":1")
    env.setdefault("DBUS_SESSION_BUS_ADDRESS", f"unix:path=/run/user/{os.getuid()}/bus")
    messages = queue.Queue()
    events = []
    with (OUTPUT / "app-engine.log").open("w") as log:
        process = subprocess.Popen([str(install / "bin/sonefold-service"), "--stdio"],
                                   stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=log, env=env)

        def receive():
            for line in process.stdout:
                if line.startswith(b"OTRPC1 "):
                    messages.put(json.loads(line[7:]))
            messages.put({"engine_exited": process.poll()})

        threading.Thread(target=receive, daemon=True).start()
        serial = 0

        def call(command, **args):
            nonlocal serial
            serial += 1
            process.stdin.write(b"OTRPC1 " + json.dumps({"id": serial, "command": command, "args": args}).encode() + b"\n")
            process.stdin.flush()
            while True:
                message = messages.get(timeout=600)
                if "engine_exited" in message:
                    raise RuntimeError("The native engine stopped; inspect app-engine.log")
                if message.get("id") == serial:
                    if message.get("error"):
                        raise RuntimeError(f"{command}: {message['error']}")
                    return message["result"]
                if message.get("kind") == "event":
                    events.append(message)

        try:
            while messages.get(timeout=60).get("kind") != "ready":
                pass
            # Check the engine and use its configured recordings folder.
            if call("is_recording"):
                raise RuntimeError("Recording is active; refusing to start analysis")
            if (OUTPUT / "created.json").exists():
                created = json.loads((OUTPUT / "created.json").read_text())
                meeting, folder = created["meeting_id"], Path(created["folder"])
                call("api_get_meeting_metadata", meetingId=meeting)
                if not (folder / "audio.wav").is_file():
                    raise RuntimeError("Previously added preview is missing its audio")
                backup = Path(created["backup"])
            else:
                preferences = call("get_recording_preferences")
                base = Path(preferences.get("save_folder") or Path.home() / "Documents/sonefold-recordings")
                folder = base / f"Synthetic-{args.case}-{stamp}"
                folder.mkdir(parents=True, exist_ok=False)
                shutil.copyfile(OUTPUT / "meeting.wav", folder / "audio.wav")
                saved = call("api_save_transcript", meetingTitle=script["title"], folderPath=str(folder), transcripts=[])
                meeting = saved["meeting_id"]
                (OUTPUT / "created.json").write_text(json.dumps({"meeting_id": meeting, "folder": str(folder), "backup": str(backup)}))
            call("workspace_set_tags", meetingId=meeting, tags=["synthetic", script.get("difficulty", "very-hard"), "-".join(script["languages"]), "benchmark-preview"])
            print("Added synthetic audio; running Qwen3-ASR 1.7B and speaker detection", flush=True)
            analysis = call("speech_analyze_meeting", meetingId=meeting, jobId="synthetic-preview-" + stamp,
                            diarize=True, numSpeakers=None)
            rows = call("api_get_meeting_transcripts", meetingId=meeting, offset=0, limit=10000)["transcripts"]
            if not rows:
                raise RuntimeError("The real ASR pipeline returned no transcript")
            text = "\n".join(
                f"[{int(row.get('audio_start_time') or 0)//60:02}:{int(row.get('audio_start_time') or 0)%60:02}] "
                f"[{row.get('speaker_name') or row.get('speaker_id') or 'Unknown speaker'}] {row['text']}"
                for row in rows
            )
            print("Generating notes with Qwen3.5 4B", flush=True)
            call("api_process_transcript", text=text, model="builtin-ai", modelName="qwen3.5:4b", meetingId=meeting,
                 chunkSize=None, overlap=None, customPrompt=None, templateId="standard_meeting", summaryLanguage="en", authToken=None)
            deadline = time.monotonic() + 600
            while time.monotonic() < deadline:
                summary = call("api_get_summary", meetingId=meeting)
                if summary["status"] == "completed":
                    break
                if summary["status"] in {"error", "failed", "cancelled"}:
                    raise RuntimeError("Summary generation failed: " + str(summary.get("error")))
                time.sleep(.5)
            else:
                raise RuntimeError("Summary generation timed out")
            speakers = call("list_meeting_speakers", meetingId=meeting)
            # Keep the listening sample findable even when notes propose a title.
            call("api_save_meeting_title", meetingId=meeting, title=script["title"])
            metadata = call("api_get_meeting_metadata", meetingId=meeting)
            result = {"meeting_id": meeting, "title": script["title"], "recording_folder": str(folder),
                      "audio": str(folder / "audio.wav"), "duration_seconds": script["duration_seconds"],
                      "transcript_rows": len(rows), "speaker_result": speakers, "analysis": analysis,
                      "summary_status": summary["status"], "metadata": metadata,
                      "summary_model": "qwen3.5:4b", "asr_model": "qwen3-asr-1.7b", "backup": str(backup)}
            (OUTPUT / "actual-transcript.json").write_text(json.dumps(rows, ensure_ascii=False, indent=2))
            (OUTPUT / "actual-summary.json").write_text(json.dumps(summary, ensure_ascii=False, indent=2))
            with sqlite3.connect(f"file:{profile}/meeting_minutes.sqlite?mode=ro", uri=True) as db:
                turns = db.execute("SELECT speaker_id,start_time,end_time FROM speaker_turns WHERE meeting_id=? ORDER BY start_time", (meeting,)).fetchall()
            (OUTPUT / "diarization-turns.json").write_text(json.dumps({"turns": turns}, indent=2))
            result["display_name_annotations"] = reference_display_names(script, turns)
            for label in result["display_name_annotations"]:
                call("rename_meeting_speaker", meetingId=meeting, speakerId=label["speaker_id"], name=label["name"])
            (OUTPUT / "installed.json").write_text(json.dumps(result, ensure_ascii=False, indent=2))
            print(json.dumps({key: result[key] for key in ("meeting_id", "title", "duration_seconds", "transcript_rows", "summary_status", "audio")}), flush=True)
        finally:
            try:
                process.stdin.write(b'OTRPC1 {"id":999999,"command":"shutdown","args":{}}\n')
                process.stdin.flush()
                process.wait(timeout=20)
            except Exception:
                process.kill()
                process.wait()


if __name__ == "__main__":
    main()
