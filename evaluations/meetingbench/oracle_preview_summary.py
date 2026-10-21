"""Run the installed 4B notes pipeline on the authored reference in isolation.

This control does not change the user's meeting, transcript, notes, or settings.
"""
import json
import os
from pathlib import Path
import queue
import subprocess
import tempfile
import threading
import time

from evaluations.meetingbench.gemini import atomic_json


def main():
    root = Path(__file__).resolve().parents[2]
    output = root / "evaluations/meetingbench/.runtime/very-hard-preview"
    script = json.loads((output / "reference.json").read_text())
    install = (Path.home() / ".local/opt/sonefold/slint/current").resolve()
    model = Path.home() / ".local/share/com.judeam.sonefold/models/summary/Qwen3.5-4B-Q4_K_M.gguf"
    with tempfile.TemporaryDirectory(prefix="meetingbench-reference-summary-") as temporary:
        folder = Path(temporary)
        models = folder / "data/com.judeam.sonefold/models/summary"
        models.mkdir(parents=True)
        (models / model.name).symlink_to(model)
        env = dict(os.environ, APPDIR=str(install), SONEFOLD_LLAMA_HELPER=str(install / "bin/llama-helper"),
                   XDG_DATA_HOME=str(folder / "data"), XDG_CONFIG_HOME=str(folder / "config"), XDG_CACHE_HOME=str(folder / "cache"))
        env["LD_LIBRARY_PATH"] = str(install / "lib") + (os.pathsep + env["LD_LIBRARY_PATH"] if env.get("LD_LIBRARY_PATH") else "")
        env["PATH"] = str(install / "bin") + os.pathsep + env["PATH"]
        env.setdefault("DISPLAY", ":1")
        env.setdefault("DBUS_SESSION_BUS_ADDRESS", f"unix:path=/run/user/{os.getuid()}/bus")
        messages = queue.Queue()
        with (output / "oracle-summary-engine.log").open("w") as log:
            process = subprocess.Popen([str(install / "bin/sonefold-service"), "--stdio"], stdin=subprocess.PIPE,
                                       stdout=subprocess.PIPE, stderr=log, env=env)

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
                    result = messages.get(timeout=180)
                    if "engine_exited" in result:
                        raise RuntimeError("Reference summary engine stopped")
                    if result.get("id") == serial:
                        if result.get("error"):
                            raise RuntimeError(str(result["error"]))
                        return result["result"]

            try:
                while messages.get(timeout=60).get("kind") != "ready":
                    pass
                rows = [{"id": turn["id"], "text": turn["text"], "timestamp": "00:00:00",
                         "audio_start_time": turn["start_seconds"], "audio_end_time": turn["end_seconds"],
                         "duration": turn["end_seconds"] - turn["start_seconds"], "speaker": turn["speaker"],
                         "speaker_id": turn["speaker"], "speaker_name": turn["speaker_name"]}
                        for turn in script["turns"]]
                meeting = call("api_save_transcript", meetingTitle="Reference control for synthetic incident",
                               folderPath=str(folder / "reference"), transcripts=rows)["meeting_id"]
                call("api_save_model_config", provider="builtin-ai", model="qwen3.5:4b", whisperModel="base")
                start = time.perf_counter()
                call("native_generate", meetingId=meeting, templateId="standard_meeting", summaryLanguage="en", customPrompt=None)
                until = time.monotonic() + 300
                while time.monotonic() < until:
                    summary = call("api_get_summary", meetingId=meeting)
                    if summary["status"] == "completed":
                        break
                    if summary["status"] in {"failed", "error", "cancelled"}:
                        raise RuntimeError(str(summary.get("error")))
                    time.sleep(.25)
                else:
                    raise RuntimeError("Reference summary timed out")
                summary["control"] = {"input": "authored reference transcript with speaker names", "model": "qwen3.5:4b",
                                      "wall_seconds": time.perf_counter() - start, "user_library_unchanged": True}
                atomic_json(output / "oracle-summary.json", summary)
                print(json.dumps(summary["control"]))
            finally:
                try:
                    process.stdin.write(b'OTRPC1 {"id":99999,"command":"shutdown","args":{}}\n')
                    process.stdin.flush()
                    process.wait(timeout=20)
                except Exception:
                    process.kill()
                    process.wait()


if __name__ == "__main__":
    main()
