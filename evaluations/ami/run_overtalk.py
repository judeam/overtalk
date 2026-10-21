"""Transcribe AMI meetings with Overtalk's own audio-import path and save the hypotheses.

Drives a release `sonefold-service --stdio` in a disposable XDG profile: the same
decode, resampling, VAD (2 s redemption), 25 s splitting and Parakeet calls the app
uses when a user imports a recording. The speech model is fetched by the app's own
download code into that profile when it is missing. Hypotheses contain model output
for CC BY 4.0 audio; keep them outside source control.
"""
from __future__ import annotations

import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import platform
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT))
sys.path.insert(0, str(ROOT / "scripts"))
from evaluations.ami.reference import TEST_MEETINGS  # noqa: E402
from service_client import Service  # noqa: E402

MODEL = "parakeet-tdt-0.6b-v3-int8"  # crate::config::DEFAULT_PARAKEET_MODEL
CONDITIONS = {"headset": "Mix-Headset", "farfield": "Array1-01"}
COMPLETION = {"import-complete", "import-error"}


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


def default_model(config: Path) -> str:
    """Read DEFAULT_PARAKEET_MODEL so a changed default cannot be measured under an old name."""
    for line in config.read_text().splitlines():
        if line.startswith("pub const DEFAULT_PARAKEET_MODEL"):
            return line.split('"')[1]
    raise ValueError("DEFAULT_PARAKEET_MODEL not found")


def model_status(service: Service) -> str | dict:
    for model in service.call("parakeet_get_available_models"):
        if model["name"] == MODEL:
            return model["status"]
    raise RuntimeError(f"{MODEL} is not in the app's model catalogue")


def ensure_model(service: Service) -> Path:
    service.call("parakeet_init")
    if model_status(service) != "Available":
        print(f"Downloading {MODEL} with the app's own downloader", flush=True)
        service.reply_timeout = 3 * 3600
        try:
            service.call("parakeet_download_model", modelName=MODEL)
        finally:
            service.reply_timeout = 60
        status = model_status(service)
        if status != "Available":
            raise RuntimeError(f"{MODEL} is not available after download: {status}")
    return Path(service.call("parakeet_get_models_directory")) / MODEL


def transcript_rows(service: Service, meeting_id: str) -> list[dict]:
    rows, offset = [], 0
    while True:
        page = service.call("api_get_meeting_transcripts", meetingId=meeting_id, offset=offset, limit=500)
        rows.extend(page["transcripts"])
        offset += len(page["transcripts"])
        if not page["has_more"] or not page["transcripts"]:
            return rows


def transcribe(service: Service, audio: Path, title: str, timeout: float) -> dict:
    # Other work on the machine slows a CPU engine; keep the evidence with the timing.
    load_before = os.getloadavg()
    began = time.perf_counter()
    service.call("start_import_audio_command", sourcePath=str(audio), title=title,
                 language="en", model=MODEL, provider="parakeet")
    event = service.wait_for_event(COMPLETION, timeout)
    wall = time.perf_counter() - began
    load = {"before": load_before, "after": os.getloadavg()}
    if event["event"] == "import-error":
        return {"status": "failed", "error": event["payload"].get("error"), "wall_seconds": wall,
                "load_average_1_5_15": load}
    payload = event["payload"]
    rows = transcript_rows(service, payload["meeting_id"])
    segments = sorted(({"start": row.get("audio_start_time"), "end": row.get("audio_end_time"),
                        "text": row["text"]} for row in rows),
                      key=lambda row: (row["start"] is None, row["start"] or 0.0))
    return {"status": "completed", "wall_seconds": wall, "audio_seconds": payload["duration_seconds"],
            "load_average_1_5_15": load, "segments": segments}


def git(*args: str) -> str:
    return subprocess.run(["git", *args], cwd=ROOT, capture_output=True, text=True, check=True).stdout.strip()


def cpu_model() -> str:
    for line in Path("/proc/cpuinfo").read_text().splitlines():
        if line.startswith("model name"):
            return line.split(":", 1)[1].strip()
    return platform.processor()


def write_json(path: Path, value: dict) -> None:
    temporary = path.with_suffix(".tmp")
    temporary.write_text(json.dumps(value, ensure_ascii=False, indent=1))
    os.replace(temporary, path)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True, help="Release sonefold-service")
    parser.add_argument("--audio-dir", type=Path, required=True)
    parser.add_argument("--profile", type=Path, required=True, help="Disposable XDG profile root")
    parser.add_argument("--output", type=Path, required=True, help="Directory for hypotheses")
    parser.add_argument("--conditions", nargs="*", choices=sorted(CONDITIONS), default=sorted(CONDITIONS))
    parser.add_argument("--meetings", nargs="*", default=list(TEST_MEETINGS))
    parser.add_argument("--timeout", type=float, default=4 * 3600, help="Seconds allowed per meeting")
    args = parser.parse_args()
    configured = default_model(ROOT / "frontend/src-tauri/src/config.rs")
    if configured != MODEL:
        parser.error(f"The app default is now {configured}; update MODEL before measuring")
    real_library = Path.home() / ".local/share/com.judeam.sonefold"
    profile = args.profile.resolve()
    if profile == real_library or real_library in profile.parents or profile in real_library.parents:
        parser.error("Use a disposable profile, never the real library")
    profile.mkdir(parents=True, exist_ok=True)
    args.output.mkdir(parents=True, exist_ok=True)
    binary = args.binary.resolve()
    service = Service(binary, profile)
    try:
        model_dir = ensure_model(service)
        preferences = service.call("get_recording_preferences")
        preferences["save_folder"] = str(profile / "imported")
        service.call("set_recording_preferences", preferences=preferences)
        identity = {
            "git_commit": git("rev-parse", "HEAD"),
            "git_dirty": bool(git("status", "--porcelain", "--untracked-files=no")),
            "binary_sha256": sha256(binary),
            "model": MODEL,
            "model_files_sha256": {path.name: sha256(path) for path in sorted(model_dir.iterdir())
                                   if path.is_file()},
        }
        provenance = args.output / "run.json"
        if provenance.exists():
            # Resuming is allowed only with the same build, model and sources.
            earlier = json.loads(provenance.read_text())
            if any(earlier.get(key) != value for key, value in identity.items()):
                parser.error("This output was produced by a different build or model; use a new --output")
        else:
            write_json(provenance, {
                "date": datetime.date.today().isoformat(), **identity,
                "engine": "Parakeet TDT (ONNX Runtime via the ort crate, CPUExecutionProvider)",
                "code_path": "start_import_audio_command -> audio::import::run_import (provider parakeet)",
                "cpu": cpu_model(), "logical_cpus": os.cpu_count(),
                "os": f"{platform.system()} {platform.release()}",
                "command": [Path(sys.executable).name, *sys.argv],
            })
        for condition in args.conditions:
            (args.output / condition).mkdir(exist_ok=True)
            for meeting in args.meetings:
                target = args.output / condition / f"{meeting}.json"
                if target.exists():
                    continue
                audio = args.audio_dir / f"{meeting}.{CONDITIONS[condition]}.wav"
                if not audio.is_file():
                    print(f"{condition} {meeting}: audio missing", flush=True)
                    continue
                result = transcribe(service, audio, f"AMI {meeting} {condition}", args.timeout)
                result.update(meeting=meeting, condition=condition, audio_file=audio.name,
                              audio_sha256=sha256(audio), model=MODEL)
                write_json(target, result)
                print(f"{condition} {meeting}: {result['status']}, {len(result.get('segments', []))} segments, "
                      f"{result['wall_seconds']:.1f} s wall", flush=True)
    finally:
        service.close()


if __name__ == "__main__":
    main()
