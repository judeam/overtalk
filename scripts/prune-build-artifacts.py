#!/usr/bin/env python3
"""Prune obsolete project executables, never libraries, models or application data.

Dry-run by default. Run with --apply after builds finish. Retains the newest
hashed executable for each project target in each profile/directory.
"""
import argparse
from collections import defaultdict
from pathlib import Path
import re
import shutil
import time

TARGETS = {
    "overtalk", "app_lib", "overtalk_native", "sonefold", "sonefold_service",
    "overtalk_documents", "overtalk_voice_memory", "overtalk_protocol",
    "overtalk_browser_bridge", "overtalk_capture", "ui_interactions",
    "sidebar_layout", "motion_idle", "transcript_layout", "dialog_interaction",
    "verify_workflows", "verify_playback", "benchmark_ui", "visual_review",
    "llama_helper", "overtalk_worker", "overtalk_worker_sandbox", "isolation",
}


def candidates(root):
    for profile in ("debug", "release"):
        for kind in ("deps", "examples"):
            directory = root / profile / kind
            if not directory.is_dir() or directory.is_symlink():
                continue
            groups = defaultdict(list)
            for path in directory.iterdir():
                if path.is_symlink() or not path.is_file():
                    continue
                match = re.fullmatch(r"(.+)-[0-9a-f]{16}(?:\.exe)?", path.name)
                if match and match[1] in TARGETS:
                    groups[match[1]].append(path)
                elif path.name.endswith(".rcgu.o") and path.stat().st_mtime < time.time() - 86400:
                    # Old intermediate object files from completed rustc runs.
                    yield path
            for paths in groups.values():
                yield from sorted(paths, key=lambda p: p.stat().st_mtime_ns, reverse=True)[1:]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--apply", action="store_true")
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1] / "target"
    if not root.is_dir() or root.is_symlink():
        raise SystemExit("Expected the repository's own target directory")
    paths = list(candidates(root))
    size = sum(p.stat().st_size for p in paths)
    before = shutil.disk_usage(root).free
    if args.apply:
        for path in paths:
            path.unlink()
    gained = shutil.disk_usage(root).free - before
    print(f"{'Removed' if args.apply else 'Would remove'} {len(paths)} generated files; "
          f"logical size {size / 2**30:.2f} GiB; reclaimed {gained / 2**30:.2f} GiB.")
    print("Newest project executables, dependency libraries, recordings and models retained.")


if __name__ == "__main__":
    main()
