#!/usr/bin/env python3
"""Exercise long-meeting playback in the real, headless Slint UI/controller."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile

root = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--binary", type=Path, default=root / "target/debug/examples/verify_playback")
args = parser.parse_args()
fixture = json.loads((root / "native/tests/fixtures/visual.json").read_text(encoding="utf-8"))
speakers = fixture["meeting_timeline"]["speakers"]
lines = []
for index in range(230):
    speaker = speakers[index % len(speakers)]
    lines.append({
        "id": f"t{index}",
        "text": ("A longer wrapped explanation exercises variable passage heights. " *
                 (8 if index % 9 == 0 else 2)) + f"Passage {index}.",
        "audio_start_time": index * 5,
        "audio_end_time": (index + 1) * 5,
        "speaker_id": speaker["id"], "speaker_name": speaker["name"],
    })
fixture["meeting_overviews"][0]["speakers"] = []
fixture["api_get_meeting_transcripts"] = {"transcripts": lines, "total_count": len(lines)}
fixture["meeting_timeline"]["duration"] = 1150
fixture["meeting_timeline"]["turns"] = [
    {"speaker_id": line["speaker_id"], "start": line["audio_start_time"], "end": line["audio_end_time"]}
    for line in lines
]
fixture["api_get_summary"]["data"]["markdown"] = (
    "## Decisions\n- First decision [00:38]\n- Second decision [00:40]\n"
    "- Third decision [00:42]\n\n## Actions\n- Complete the work [10:00]"
)
with tempfile.TemporaryDirectory(prefix="overtalk-playback-") as temporary:
    fixture_path = Path(temporary) / "fixture.json"
    fixture_path.write_text(json.dumps(fixture), encoding="utf-8")
    env = dict(os.environ, OVERTALK_FIXTURE=str(fixture_path),
               OVERTALK_SERVICE=str(root / "native/tests/fixtures/engine.py"),
               XDG_DATA_HOME=f"{temporary}/data", XDG_CONFIG_HOME=f"{temporary}/config",
               XDG_CACHE_HOME=f"{temporary}/cache", OVERTALK_REDUCED_MOTION="1")
    subprocess.run([str(args.binary.resolve())], env=env, check=True, timeout=30)
