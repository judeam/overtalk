#!/usr/bin/env python3
"""Measure synthetic native UI frame work, meeting-open latency and idle redraws.

These are CPU render submission timings, not display FPS. Uses a disposable
profile and the normal controller/renderer; never opens the user's library.
"""
import argparse
import json
import os
from pathlib import Path
import re
import statistics
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--binary', type=Path, default=ROOT/'target/release/examples/benchmark_ui')
p.add_argument('--output', type=Path, required=True)
p.add_argument('--sizes', nargs='+', default=['1280x720', '3840x2160'])
p.add_argument('--first-open-budget-ms', type=float, default=150)
args = p.parse_args()
reports = []
for dimensions in args.sizes:
    width, height = map(int, dimensions.split('x'))
    with tempfile.TemporaryDirectory(prefix='overtalk-performance-') as temporary:
        root = Path(temporary)
        env = dict(os.environ, OVERTALK_SERVICE=str(ROOT/'native/tests/fixtures/engine.py'),
                   OVERTALK_FIXTURE=str(ROOT/'native/tests/fixtures/visual.json'), OVERTALK_TRACE_RENDER='1',
                   XDG_DATA_HOME=str(root/'data'), XDG_CONFIG_HOME=str(root/'config'), XDG_CACHE_HOME=str(root/'cache'))
        env.setdefault('DISPLAY', ':1')
        peak_kib = 0
        with (root/'render.log').open('w+') as log:
            child = subprocess.Popen([str(args.binary.resolve()),str(width),str(height)],env=env,stdout=log,stderr=log)
            deadline = time.monotonic()+60
            while child.poll() is None:
                status = Path(f'/proc/{child.pid}/status')
                try:
                    match = re.search(r'^VmRSS:\s+(\d+)', status.read_text(),re.M)
                    if match: peak_kib = max(peak_kib,int(match[1]))
                except (OSError,ValueError): pass
                if time.monotonic()>deadline: child.kill(); child.wait(); raise RuntimeError('UI benchmark timed out')
                time.sleep(.02)
            log.seek(0); text = log.read()
        if child.returncode: raise RuntimeError(f'UI benchmark failed: {child.returncode}\n{text[-2000:]}')
        frames = [float(v) for v in re.findall(r'render_cpu_ms=([\d.]+)',text)]
        opens = [float(v) for v in re.findall(r'open_first_frame_ms=([\d.]+)',text)]
        if not frames or not opens: raise RuntimeError('Renderer timing hooks did not report samples')
        idle = text.split('NATIVE_BENCH_IDLE_BEGIN',1)
        idle_frames = idle[1].count('NATIVE_PERF render_cpu') if len(idle)==2 else None
        actual = re.search(r'NATIVE_BENCH_SIZE (\d+)x(\d+)',text)
        final = re.search(r'NATIVE_BENCH_FINAL_SIZE (\d+)x(\d+)',text)
        report = dict(size=[width,height],frames=len(frames),render_median_ms=statistics.median(frames),
                      render_p95_ms=sorted(frames)[int((len(frames)-1)*.95)],opens_ms=opens,
                      idle_frames=idle_frames,peak_ui_rss_mib=round(peak_kib/1024,1))
        report['actual_size'] = list(map(int,actual.groups())) if actual else None
        report['final_size'] = list(map(int,final.groups())) if final else None
        report['pass']=report['actual_size']==[width,height] and report['final_size']==[width,height] and opens[0]<=args.first_open_budget_ms and max(opens[1:],default=0)<50 and idle_frames is not None and idle_frames<=2
        reports.append(report)
args.output.parent.mkdir(parents=True,exist_ok=True)
args.output.write_text(json.dumps(dict(definition='CPU render submission and request-to-first-frame; synthetic data; not display FPS',results=reports),indent=2)+'\n')
for report in reports: print(json.dumps(report))
raise SystemExit(0 if all(r['pass'] for r in reports) else 1)
