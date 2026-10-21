#!/usr/bin/env python3
"""Profile real offline companion reads on a disposable synthetic library.

Use --module for a saved baseline, and compare export_sha256 before latency.
Only the 300-read resource probe defers GC deliberately to expose retention;
exports use normal collection. No user library or provider is contacted.
"""
import argparse
from contextlib import closing
import cProfile
import gc
import hashlib
import importlib.util
import json
from pathlib import Path
import resource
import sqlite3
import sys
import tempfile
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--module', type=Path, default=Path(__file__).resolve().parents[1] / 'integrations/sonefold.py')
    parser.add_argument('--rows', type=int, default=30000)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--profile', type=Path)
    args = parser.parse_args()
    if not 1 <= args.rows <= 100000:
        parser.error('--rows must be between 1 and 100000')
    spec = importlib.util.spec_from_file_location('companion', args.module)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    with tempfile.TemporaryDirectory(prefix='overtalk-companion-profile-') as temporary:
        root = Path(temporary)
        with closing(sqlite3.connect(root / 'db')) as db:
            db.executescript('''
                CREATE TABLE meetings(id TEXT PRIMARY KEY,title TEXT,created_at TEXT,updated_at TEXT);
                CREATE TABLE transcripts(id TEXT,meeting_id TEXT,transcript TEXT,timestamp TEXT,audio_start_time REAL,audio_end_time REAL);
                CREATE INDEX transcripts_meeting_time ON transcripts(meeting_id,audio_start_time,id);
                CREATE TABLE summary_processes(meeting_id TEXT,result TEXT);
                INSERT INTO meetings VALUES('m','Synthetic profile','2026-10-18','2026-10-18');
            ''')
            db.executemany('INSERT INTO transcripts VALUES(?,?,?,?,?,?)', (
                (f't{i}', 'm', f'Synthetic passage {i}', str(i), i, i + 1) for i in range(args.rows)))
            db.commit()
        config = root / 'config.json'
        config.write_text(json.dumps(dict(database_path=str(root / 'db'), cli_enabled=True,
                                         read_meetings=True, read_transcripts=True, read_summaries=True)))
        reader = module.MeetingReader(config, 'cli')
        fds = lambda: len(list(Path('/proc/self/fd').iterdir())) if sys.platform == 'linux' else None
        gc.collect()
        initial_fds = fds()
        gc.disable()
        try:
            start = time.perf_counter()
            for _ in range(300):
                reader.meetings()
            reads_ms = (time.perf_counter() - start) * 1000
            retained_fds = fds() - initial_fds if initial_fds is not None else None
        finally:
            gc.enable()
            gc.collect()
        profiler = cProfile.Profile() if args.profile else None
        if profiler:
            profiler.enable()
        start = time.perf_counter()
        result = reader.markdown('m')
        export_ms = (time.perf_counter() - start) * 1000
        if profiler:
            profiler.disable()
            args.profile.parent.mkdir(parents=True, exist_ok=True)
            profiler.dump_stats(args.profile)
        if result.count('Synthetic passage ') != args.rows or f'Synthetic passage {args.rows - 1}\n' not in result:
            raise RuntimeError('Incomplete export')
        peak_rss = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
        report = dict(module_sha256=hashlib.sha256(args.module.read_bytes()).hexdigest(),
                      python=sys.version.split()[0], sqlite=sqlite3.sqlite_version, rows=args.rows,
                      deferred_gc_reads=300, reads_ms=reads_ms, retained_fds=retained_fds,
                      export_ms=export_ms, profiled=bool(profiler),
                      peak_rss_bytes=peak_rss if sys.platform == 'darwin' else peak_rss * 1024,
                      export_sha256=hashlib.sha256(result.encode()).hexdigest())
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(report, indent=2) + '\n')
        print(json.dumps(report))


if __name__ == '__main__':
    main()
