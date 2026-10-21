#!/usr/bin/env python3
"""Profile real service search with synthetic data only (wall time, Callgrind or Massif)."""
from service_client import Service
import argparse
import json
from pathlib import Path
import shlex
import sqlite3
import statistics
import subprocess
import tempfile
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--tool", choices=["wall", "callgrind", "massif"], default="wall")
    parser.add_argument("--valgrind", type=Path)
    parser.add_argument("--rows", type=int, default=50000)
    args = parser.parse_args()
    if not 100 <= args.rows <= 100000:
        parser.error("rows must be between 100 and 100000")
    args.output = args.output.resolve()
    args.output.parent.mkdir(parents=True, exist_ok=True)
    report = {"tool": args.tool, "rows": args.rows, "queries": {}}
    with tempfile.TemporaryDirectory(prefix="overtalk-search-profile-") as temp:
        root = Path(temp)
        binary = args.binary.resolve()
        if args.tool != "wall":
            if not args.valgrind:
                parser.error("--valgrind is required for instrumentation")
            command = [str(args.valgrind.resolve()), f"--tool={args.tool}"]
            if args.tool == "callgrind":
                command += ["--instr-atstart=no", f"--callgrind-out-file={args.output}.callgrind"]
            else:
                command += ["--time-unit=B", f"--massif-out-file={args.output}.massif"]
            wrapper = root / "profile-service"
            wrapper.write_text("#!/bin/sh\nexec " + shlex.join([*command, str(binary)]) + ' "$@"\n')
            wrapper.chmod(0o700)
            binary = wrapper
        start = time.perf_counter()
        service = Service(binary, root)
        report["startup_ms"] = (time.perf_counter() - start) * 1000
        try:
            database = root / "data/com.judeam.sonefold/meeting_minutes.sqlite"
            with sqlite3.connect(database) as db:
                db.executemany("INSERT INTO meetings(id,title,created_at,updated_at) VALUES(?,?,?,?)",
                               [(f"m{i}", f"Synthetic meeting {i}", "2026-10-18", "2026-10-18") for i in range(200)])
                db.executemany("INSERT INTO transcripts(id,meeting_id,transcript,timestamp,audio_start_time,audio_end_time) VALUES(?,?,?,'00:00',?,?)",
                               [(f"t{i:06}", f"m{i % 200}", "Synthetic project review. " * (1 + i % 7) + ("Unique zebra." if i % 1000 == 0 else ""),
                                 i // 200 * 2, i // 200 * 2 + 2) for i in range(args.rows)])
            def control(*options):
                subprocess.run([str(args.valgrind.resolve().with_name("callgrind_control")),
                                *options, str(service.process.pid)], check=True,
                               stdout=subprocess.DEVNULL, timeout=30)
            def rss():
                return int(next(line.split()[1] for line in Path(f"/proc/{service.process.pid}/status").read_text().splitlines() if line.startswith("VmRSS:")))
            report["rss_before_kib"] = rss()
            if args.tool == "callgrind":
                control("-i", "on")
                control("-z")
            for label, request in [
                ("recent", dict(query="", semantic=False, limit=24)),
                ("broad", dict(query="project", semantic=False, limit=24)),
                ("selective", dict(query="zebra", semantic=False, limit=24)),
                ("scoped", dict(query="project", meeting_id="m42", semantic=False, limit=24)),
            ]:
                samples, ids = [], None
                for _ in range(7 if args.tool == "wall" else 3):
                    start = time.perf_counter()
                    result = service.call("workspace_search", request=request)
                    samples.append((time.perf_counter() - start) * 1000)
                    current = [row["id"] for row in result["sources"]]
                    assert current and (ids is None or current == ids)
                    ids = current
                report["queries"][label] = {"median_ms": statistics.median(samples), "ids": ids}
                if args.tool == "callgrind":
                    control(f"--dump={label}")
                    control("-z")
            report["rss_after_kib"] = rss()
            if args.tool == "callgrind":
                control("-i", "off")
        finally:
            service.close()
            # Preserve only synthetic profiler diagnostics, not the database.
            args.output.with_suffix(".log").write_text((root / "service.log").read_text())
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps({key: value for key, value in report.items() if key != "queries"}))
    for label, result in report["queries"].items():
        print(f"{label}: {result['median_ms']:.3f} ms ({args.tool})")


if __name__ == "__main__":
    main()
