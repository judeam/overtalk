"""Exercise the real native service using only a disposable library and silent audio."""
import argparse
import datetime
import json
import os
from pathlib import Path
import queue
import shutil
import subprocess
import tempfile
import threading
import time
import wave

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("binary", type=Path)
parser.add_argument("--audio", action="store_true", help="Also play a generated silent recording through the real audio device")
parser.add_argument("--ui", type=Path, help="Also run the production Slint controller workflow checker")
parser.add_argument("--local-ai-data", type=Path, help="Use existing local model files/settings read-only in the disposable profile")
parser.add_argument("--record", action="store_true", help="Record generated speech through private PipeWire virtual devices (Linux)")
parser.add_argument("--default-devices", action="store_true", help="Exercise recording with saved device preferences rather than explicit device arguments")
parser.add_argument("--batch", action="store_true", help="Also import and retranscribe generated speech using the existing local ASR model")
args = parser.parse_args()
if args.batch and not args.local_ai_data:
    parser.error("--batch requires --local-ai-data")
binary = args.binary.resolve()
with tempfile.TemporaryDirectory(prefix="overtalk-workflows-") as temporary:
    root = Path(temporary)
    env = dict(os.environ, XDG_DATA_HOME=str(root / "data"), XDG_CONFIG_HOME=str(root / "config"), XDG_CACHE_HOME=str(root / "cache"))
    # Isolate application settings while retaining permission to the user's
    # desktop audio server. Never copy or print the authentication cookie.
    pulse_cookie = Path(os.environ.get("XDG_CONFIG_HOME", str(Path.home() / ".config"))) / "pulse/cookie"
    if pulse_cookie.is_file():
        env.setdefault("PULSE_COOKIE", str(pulse_cookie))
    if os.name == "posix" and hasattr(os, "getuid"):
        runtime = Path(f"/run/user/{os.getuid()}")
        if runtime.is_dir():
            env.setdefault("XDG_RUNTIME_DIR", str(runtime))
            if (runtime / "pulse/native").exists():
                env.setdefault("PULSE_SERVER", f"unix:{runtime}/pulse/native")
    virtual_audio = None
    feed = None
    profile = root / "data/com.judeam.sonefold"
    if args.local_ai_data:
        source = args.local_ai_data.resolve()
        profile.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source / "speech-runtime.json", profile / "speech-runtime.json")
        for item in (source / "models/summary").glob("*.gguf"):
            target = profile / "models/summary" / item.name
            target.parent.mkdir(parents=True, exist_ok=True)
            target.symlink_to(item)
    env.setdefault("DISPLAY", ":1")
    env.setdefault("DBUS_SESSION_BUS_ADDRESS", "unix:path=/run/user/1000/bus")
    messages = queue.Queue()
    log = root / "service.log"
    with log.open("w") as output:
        process = subprocess.Popen([str(binary), "--stdio"], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=output, env=env, cwd=root)
        def collect():
            for line in process.stdout:
                if line.startswith(b"OTRPC1 "):
                    messages.put(json.loads(line[7:]))
        threading.Thread(target=collect, daemon=True).start()
        serial = 0
        completions = []
        completion_events = {"import-complete", "import-error", "retranscription-complete", "retranscription-error"}
        def call(command, **values):
            global serial
            serial += 1
            process.stdin.write(b"OTRPC1 " + json.dumps(dict(id=serial, command=command, args=values)).encode() + b"\n")
            process.stdin.flush()
            while True:
                reply = messages.get(timeout=300 if args.local_ai_data else 30)
                if reply.get("event") in completion_events:
                    completions.append(reply)
                if reply.get("id") == serial:
                    assert reply.get("error") is None, (command, reply.get("error"))
                    return reply["result"]
        def wait_for_completion(success, failure):
            deadline = time.monotonic() + 300
            while time.monotonic() < deadline:
                reply = completions.pop(0) if completions else messages.get(timeout=max(0.1, deadline - time.monotonic()))
                assert reply.get("event") != failure, reply.get("payload")
                if reply.get("event") == success:
                    return reply["payload"]
            raise TimeoutError(success)
        try:
            while messages.get(timeout=300 if args.local_ai_data else 30).get("kind") != "ready":
                pass
            assert isinstance(call("get_notification_settings")["recording_notifications"], bool)
            capabilities = call("workspace_account_auth", action={"op":"capabilities"})
            assert isinstance(capabilities["google_configured"], bool)
            providers = {entry["id"]: entry["configured"] for entry in capabilities["providers"]}
            assert set(providers) == {"linear_mcp", "atlassian_mcp", "notion_mcp", "google", "microsoft", "slack", "github"}
            assert all(isinstance(value, bool) for value in providers.values())
            assert all(providers[name] for name in ["linear_mcp", "atlassian_mcp", "notion_mcp"])
            assert call("workspace_account_auth", action={"op":"cancel","attempt":"no-pending-test-signin"}) is None
            if args.audio:
                assert isinstance(call("get_audio_devices"), list)
            folder = root / "recording"
            folder.mkdir()
            with wave.open(str(folder / "audio.wav"), "wb") as audio:
                audio.setparams((1, 2, 48000, 0, "NONE", "not compressed"))
                audio.writeframes(b"\0\0" * 48000 * 2)
            if args.local_ai_data or args.record:
                subprocess.run(["espeak-ng", "-s", "145", "-w", str(folder / "audio.wav"),
                    "We agreed to ship the search feature on Friday. Alex will finish the tests. The next review is on Monday."], check=True)
            ffmpeg = binary.with_name("ffmpeg")
            if not ffmpeg.exists():
                ffmpeg = Path(shutil.which("ffmpeg") or "ffmpeg")
            subprocess.run([str(ffmpeg), "-nostdin", "-v", "error", "-i", str(folder / "audio.wav"), "-c:a", "aac", "-movflags", "+frag_keyframe+empty_moov", str(folder / "audio.mp4")], check=True)
            saved = call("api_save_transcript", meetingTitle="Native workflow check", folderPath=str(folder), transcripts=[dict(id="fixture", text="We agreed to ship the search feature on Friday.", timestamp="2026-10-14T10:00:00Z", audio_start_time=0.0, audio_end_time=1.5, duration=1.5, speaker="mic", speaker_id="speaker_1", speaker_name="Test speaker")], speakerTurns=[dict(start=0.2, end=1.2, speaker="speaker_1", confidence=0.9)])
            meeting = saved["meeting_id"]
            rows = call("api_get_meeting_transcripts", meetingId=meeting, offset=0, limit=100)["transcripts"]
            assert len(rows) == 1
            timeline = call("meeting_timeline", meetingId=meeting)
            assert len(timeline["turns"]) == 1
            assert timeline["turns"][0]["start"] == 0.2 and timeline["turns"][0]["end"] == 1.2
            assert rows[0]["speaker_id"] == "speaker_1"
            call("api_save_meeting_title", meetingId=meeting, title="Renamed native workflow check")
            assert call("api_get_meeting_metadata", meetingId=meeting)["title"] == "Renamed native workflow check"
            print("PASS: save, open, paginate and rename meeting", flush=True)
            timings = []
            for _ in range(40):
                began = time.perf_counter()
                call("api_get_meeting_metadata", meetingId=meeting)
                call("api_get_meeting_transcripts", meetingId=meeting, offset=0, limit=100)
                timings.append((time.perf_counter() - began) * 1000)
            p95 = sorted(timings)[int(len(timings) * .95) - 1]
            print(f"PERF: meeting metadata + first transcript page p95={p95:.2f} ms", flush=True)
            assert p95 < 100, f"Meeting read regression: {p95:.2f} ms"

            call("api_save_meeting_summary", meetingId=meeting, summary={"markdown": "# Notes\n\n- Ship search on Friday."})
            assert "Ship search" in call("api_get_summary", meetingId=meeting)["data"]["markdown"]
            call("api_save_meeting_summary", meetingId=meeting, summary={"markdown": ""})
            assert call("api_get_summary", meetingId=meeting)["data"]["markdown"] == ""
            call("api_save_meeting_summary", meetingId=meeting, summary={"markdown": "# Notes\n\n- Ship search on Friday."})
            print("PASS: write and reopen notes before generation", flush=True)
            pad = call("workspace_workflow", action={"op":"load","id":meeting})
            assert pad["body"] == "" and pad["revision"] == 0
            saved_pad = call("workspace_workflow", action={"op":"save","id":meeting,"body":"Ask about accessibility; preserve this thought.","revision":0})
            assert saved_pad["revision"] == 1
            reloaded_pad = call("workspace_workflow", action={"op":"load","id":meeting})
            assert reloaded_pad["body"] == "Ask about accessibility; preserve this thought."
            assert reloaded_pad["revision"] == 1
            dashboard = call("workspace_workflow", action={"op":"dashboard","refresh":False})
            assert dashboard["events"] == [] and dashboard["accounts"] == []
            print("PASS: personal notepad persistence and empty connected-calendar dashboard", flush=True)

            row = rows[0]
            call("workspace_correct_transcript", change=dict(meeting_id=meeting, segment_id=row["id"], text="We agreed to ship search on Monday.", speaker_id=row.get("speaker_id"), expected_text=row["text"], expected_speaker_id=row.get("speaker_id")))
            updated = call("api_get_meeting_transcripts", meetingId=meeting, offset=0, limit=100)["transcripts"]
            assert updated[0]["text"].endswith("Monday.")
            found = call("workspace_search", request=dict(query="Monday", limit=20))["sources"]
            assert any(source["meeting_id"] == meeting for source in found)
            call("workspace_set_tags", meetingId=meeting, tags=["native-check"])
            assert call("workspace_search", request=dict(query="Monday", tag="native-check", limit=20))["sources"]
            assert not call("workspace_search", request=dict(query="Monday", tag="different-tag", limit=20))["sources"]
            future = (datetime.date.today() + datetime.timedelta(days=2)).isoformat()
            assert not call("workspace_search", request=dict(query="Monday", after=future, limit=20))["sources"]
            print("PASS: transcript correction, search and date/tag filters", flush=True)
            task = call("workspace_save_task", input=dict(id=None, meeting_id=meeting, title="Ship search", owner="Test speaker", due_date=None, status="open", evidence=[]))
            call("workspace_task_status", id=task, status="done")
            assert any(row["id"] == task and row["status"] == "done" for row in call("workspace_tasks"))
            call("workspace_organise_meeting", change=dict(meeting_id=meeting, pinned=True, archived=True))
            assert any(row["meeting_id"] == meeting and row["pinned"] and row["archived"] for row in call("workspace_meeting_states"))
            call("workspace_organise_meeting", change=dict(meeting_id=meeting, trashed=True))
            assert not call("workspace_search", request=dict(query="Monday", limit=20))["sources"]
            call("workspace_organise_meeting", change=dict(meeting_id=meeting, trashed=False, archived=False))
            print("PASS: actions, pin, archive, trash and restore", flush=True)
            template = dict(name="Native check", description="Disposable template", sections=[dict(title="Decisions", instruction="Summarize decisions.", format="list")])
            saved_template = call("api_save_template", templateId=None, templateJson=json.dumps(template))
            assert call("api_get_template", templateId=saved_template["id"])["template"]["name"] == template["name"]
            call("api_delete_template", templateId=saved_template["id"])
            print("PASS: create, read and delete a notes template", flush=True)
            marks = call("meeting_add_marks", meetingId=meeting, marks=[dict(at=0.5, label="Decision")])
            assert any(mark["label"] == "Decision" for mark in marks)
            mark = next(mark for mark in marks if mark["label"] == "Decision")
            call("meeting_delete_mark", meetingId=meeting, id=mark["id"])
            assert not call("meeting_marks", meetingId=meeting)
            print("PASS: timeline bookmark persistence and deletion", flush=True)
            if args.audio:
                assert call("native_audio", action="load", meetingId=meeting, value=0)["playing"]
                # Device startup depends on the host audio server. Wait for an
                # observed frame advance, with a deadline, rather than racing
                # a fixed 100ms sleep on a cold device.
                deadline = time.monotonic() + 2
                while call("native_audio", action="status")["position"] <= 0 and time.monotonic() < deadline:
                    time.sleep(0.025)
                assert call("native_audio", action="pause")["position"] > 0
                assert not call("native_audio", action="seek", value=0.5)["playing"]
                call("native_audio", action="speed", value=1.5)
                assert call("native_audio", action="play", value=0.5)["playing"]
                assert not call("native_audio", action="stop")["playing"]
                print("PASS: fragmented MP4 playback, pause, seek, speed and stop", flush=True)
            if args.local_ai_data:
                call("api_save_transcript_config", provider="localSpeech", model="qwen3-asr-1.7b")
                analyzed = call("speech_analyze_meeting", meetingId=meeting, jobId="native-workflow-asr", diarize=False, numSpeakers=None)
                speech_rows = call("api_get_meeting_transcripts", meetingId=meeting, offset=0, limit=100)["transcripts"]
                assert speech_rows and any("search" in row["text"].lower() for row in speech_rows), speech_rows
                print("PASS: real local speech model transcribes generated speech", flush=True)
                if args.batch:
                    preferences = call("get_recording_preferences")
                    preferences["save_folder"] = str(root / "imported")
                    call("set_recording_preferences", preferences=preferences)
                    call("start_import_audio_command", sourcePath=str(folder / "audio.wav"),
                         title="Synthetic batch import", language="en", model="qwen3-asr-1.7b", provider="localSpeech")
                    imported = wait_for_completion("import-complete", "import-error")
                    imported_id = imported["meeting_id"]
                    imported_rows = call("api_get_meeting_transcripts", meetingId=imported_id, offset=0, limit=100)["transcripts"]
                    assert any("search" in row["text"].lower() for row in imported_rows), imported_rows
                    metadata = call("api_get_meeting_metadata", meetingId=imported_id)
                    assert Path(metadata["folder_path"]).is_relative_to(root / "imported")
                    call("start_retranscription_command", meetingId=imported_id, meetingFolderPath=metadata["folder_path"],
                         language="en", model="qwen3-asr-1.7b", provider="localSpeech")
                    retranscribed = wait_for_completion("retranscription-complete", "retranscription-error")
                    assert retranscribed["meeting_id"] == imported_id
                    updated_rows = call("api_get_meeting_transcripts", meetingId=imported_id, offset=0, limit=100)["transcripts"]
                    assert any("search" in row["text"].lower() for row in updated_rows), updated_rows
                    print("PASS: real batch import and retranscription with persisted speech results", flush=True)
                call("api_save_model_config", provider="builtin-ai", model="gemma4:e2b", whisperModel="base")
                call("api_process_transcript", text="We agreed to ship search on Friday. Alex will finish the tests before the Monday review.", model="builtin-ai", modelName="qwen3.5:4b", meetingId=meeting, templateId="standard_meeting", summaryLanguage="en")
                deadline = time.monotonic() + 300
                while time.monotonic() < deadline:
                    summary = call("api_get_summary", meetingId=meeting)
                    assert summary["status"] != "error", summary.get("error")
                    if summary["status"] == "completed":
                        assert summary.get("data"), summary
                        break
                    time.sleep(0.3)
                else:
                    raise AssertionError("Local summary did not complete")
                print("PASS: real local notes generation and saved result", flush=True)
            if args.record:
                assert os.name == "posix" and Path("/proc").exists(), "Virtual recording check requires Linux"
                virtual_audio = subprocess.Popen(["pw-loopback", "--name=overtalk_workflow",
                    "--capture-props=media.class=Audio/Sink node.name=overtalk_workflow_sink node.description=Overtalk_Workflow_Sink priority.session=0",
                    "--playback-props=media.class=Audio/Source node.name=overtalk_workflow_source node.description=Overtalk_Workflow_Source priority.session=0"], env=env, stdout=subprocess.DEVNULL, stderr=output)
                deadline = time.monotonic() + 10
                while time.monotonic() < deadline:
                    devices = call("get_audio_devices")
                    mic = next((d["name"] for d in devices if "Overtalk_Workflow_Source" in str(d)), None)
                    system = next((d["name"] for d in devices if "Overtalk_Workflow_Sink" in str(d)), None)
                    if mic and system:
                        break
                    time.sleep(0.1)
                assert mic and system, "Private recording devices did not appear"
                preferences = call("get_recording_preferences")
                preferences["save_folder"] = str(root / "recorded")
                preferences["preferred_mic_device"] = mic + " (input)"
                preferences["preferred_system_device"] = system + " (output)"
                call("set_recording_preferences", preferences=preferences)
                notice = call("workspace_privacy", action={"op":"recording_notice"})
                assert notice["required"] and notice["text"], notice
                call("workspace_privacy", action={"op":"acknowledge_recording","method":"read_aloud"})
                start_options = dict(meetingName="Generated speech recording check", workflow={"auto_notes":False,"personal_notes":"Check both audio sources","template_id":"standard_meeting","language":"en"})
                if args.default_devices:
                    started = call("start_recording_with_devices_and_meeting", micDeviceName=None, systemDeviceName=None, **start_options)
                else:
                    started = call("start_recording_with_devices_and_meeting", micDeviceName=mic + " (input)", systemDeviceName=system + " (output)", **start_options)
                assert "falling back to system default" not in log.read_text(), "Recording device unexpectedly fell back to a physical device"
                feed = subprocess.Popen(["pw-play", "--target=overtalk_workflow_sink", str(folder / "audio.wav")], env=env, stdout=subprocess.DEVNULL, stderr=output)
                time.sleep(2)
                call("pause_recording")
                assert call("is_recording")
                time.sleep(0.2)
                call("resume_recording")
                feed.wait(timeout=30)
                stopped = call("stop_recording", args={"save_path": ""})
                assert not call("is_recording")
                meetings = call("meeting_overviews", limit=500)
                recorded = next(m for m in meetings if m["title"] == "Generated speech recording check")
                assert stopped["meeting_id"] == recorded["id"], "Stop did not return the saved meeting"
                assert started["recording_id"] == recorded["id"]
                pad = call("workspace_workflow", action={"op":"load","id":recorded["id"]})
                assert pad["body"] == "Check both audio sources" and not pad["jobs"]
                metadata = call("api_get_meeting_metadata", meetingId=recorded["id"])
                assert Path(metadata["folder_path"]).is_dir()
                assert metadata["recording_notice"].endswith("(notice read aloud)"), metadata.get("recording_notice")
                assert Path(metadata["folder_path"]).is_relative_to(root / "recorded"), "Recording ignored selected storage folder"
                assert list(Path(metadata["folder_path"]).glob("*.mp4")), "Recording audio was not persisted"
                if args.local_ai_data:
                    recorded_lines = call("api_get_meeting_transcripts", meetingId=recorded["id"], offset=0, limit=100)["transcripts"]
                    assert any(row["text"].strip() for row in recorded_lines), "Final recorded speech was lost during stop"
                print("PASS: real record, pause, resume, stop and audio/database persistence", flush=True)
            process.stdin.write(b'OTRPC1 {"id":9999,"command":"shutdown","args":{}}\n')
            process.stdin.flush()
            assert process.wait(timeout=30) == 0
            if args.ui:
                ui_env = dict(env, OVERTALK_SERVICE=str(binary), OVERTALK_VERIFY_MEETING=meeting)
                subprocess.run([str(args.ui.resolve())] + (["--audio"] if args.audio else []), env=ui_env, check=True, timeout=40, cwd=root)
            database = next((root / "data").rglob("meeting_minutes.sqlite"))
            checked = subprocess.run([str(binary), '--check-database', str(database)],
                                     capture_output=True, text=True, env=env, check=True)
            assert json.loads(checked.stdout)['valid']
            print("PASS: clean shutdown and database integrity", flush=True)
        except BaseException:
            print("Service log saved to /tmp/overtalk-workflow-failure.log", flush=True)
            shutil.copyfile(log, "/tmp/overtalk-workflow-failure.log")
            raise
        finally:
            for child in (feed, virtual_audio):
                if child is not None and child.poll() is None:
                    child.terminate()
                    child.wait(timeout=10)
            if process.poll() is None:
                process.kill()
                process.wait()
