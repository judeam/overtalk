"""Finish an existing local benchmark, then grade it without overlapping GPU jobs.

This supervisor generates no paid audio. Start only once the renderer/evaluator
PIDs have been checked. The existing cost ledger and pinned evaluator stay intact.
"""
import argparse
import fcntl
import json
import os
from pathlib import Path
import subprocess
import sys
import time

from .gemini import atomic_json
from .run_evaluation import ROOT, RUNTIME


def is_module_process(pid, module, proc_root=Path('/proc')):
    try:
        return module.encode() in (proc_root / str(pid) / 'cmdline').read_bytes().split(b'\0')
    except (FileNotFoundError, ProcessLookupError):
        return False


def result_count():
    return len(list((RUNTIME / 'results').glob('*/result.json')))


def failed_count():
    return sum(json.loads(path.read_text()).get('status') == 'failed'
               for path in (RUNTIME / 'results').glob('*/result.json'))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--evaluator-pid', type=int, required=True)
    parser.add_argument('--renderer-pid', type=int, required=True)
    args = parser.parse_args()
    # Local GPU work runs under the launcher named by OVERTALK_GPU_LAUNCHER, if any.
    launcher = [os.environ['OVERTALK_GPU_LAUNCHER']] if os.environ.get('OVERTALK_GPU_LAUNCHER') else []
    RUNTIME.mkdir(parents=True, exist_ok=True)
    with (RUNTIME / 'finish.lock').open('a+') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        state = RUNTIME / 'finish-progress.json'
        while is_module_process(args.evaluator_pid, 'evaluations.meetingbench.run_evaluation'):
            atomic_json(state, dict(stage='waiting_for_evaluator', evaluated=result_count()))
            time.sleep(30)
        env = dict(os.environ, TRAIN_GPU_PERCENT='75')
        if result_count() < 200 or failed_count():
            atomic_json(state, dict(stage='finishing_evaluation', evaluated=result_count()))
            renderer = args.renderer_pid
            try:
                candidate = int(json.loads((RUNTIME/'corpus/render-process.json').read_text())['pid'])
                if is_module_process(candidate, 'evaluations.meetingbench.render_corpus'):
                    renderer = candidate
            except (FileNotFoundError, ValueError, KeyError, TypeError):
                pass
            subprocess.run([*launcher, sys.executable, '-m', 'evaluations.meetingbench.run_evaluation',
                            '--render-pid', str(renderer), '--retry-failed'], cwd=ROOT, env=env, check=True)
        # The evaluator closes its model process before returning. Grading starts
        # only after that exit, preserving memory and measured inference timing.
        atomic_json(state, dict(stage='grading', evaluated=result_count()))
        subprocess.run([*launcher, sys.executable, '-m', 'evaluations.meetingbench.judge_corpus'],
                       cwd=ROOT, env=env, check=True)
        count = result_count()
        failed = failed_count()
        stage = 'complete' if count == 200 and not failed else 'complete_with_failures' if count == 200 else 'partial'
        atomic_json(state, dict(stage=stage, evaluated=count, failed=failed,
                                expected=200, missing=200-count, report=str(ROOT/'reports/meetingbench/index.html')))


if __name__ == '__main__':
    main()
