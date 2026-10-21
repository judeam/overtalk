#!/usr/bin/env python3
"""Check that Tauri's speech bundle contains sources/assets, never Python caches."""
import glob
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
BASE = ROOT / "frontend/src-tauri"
VENDOR = ROOT / "speech-worker/sonefold_speech/vendor"
config = json.loads((BASE / "tauri.conf.json").read_text())
packaged = set()
for pattern, destination in config["bundle"]["resources"].items():
    if "sonefold_speech/vendor" not in pattern:
        continue
    matches = [Path(p).resolve() for p in glob.glob(str(BASE / pattern))]
    assert matches, f"Resource pattern matches no files: {pattern}"
    for source in matches:
        assert source.is_file(), f"Unfiltered resource directory: {source}"
        assert "__pycache__" not in source.parts and source.suffix != ".pyc"
        expected = "speech-worker/sonefold_speech/vendor/" + source.relative_to(VENDOR).as_posix()
        actual = destination + source.name if destination.endswith("/") else destination
        assert actual == expected, f"Incorrect packaged path: {actual} != {expected}"
        packaged.add(source.resolve())
expected = {p.resolve() for p in VENDOR.rglob("*") if p.is_file()
            and "__pycache__" not in p.parts and p.suffix != ".pyc"}
assert packaged == expected, f"Unpackaged vendor assets: {expected - packaged}"
print(f"Verified {len(packaged)} vendored source/license/asset paths; no cache directories")
