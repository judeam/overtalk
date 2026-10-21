"""Controlled comparison of native notes using an owned continuous-batching server.

This is an opt-in experiment, not the installed application or default eval.
Run the entire command under your own GPU-sharing launcher if you use one
(such as the one named by OVERTALK_GPU_LAUNCHER).
"""
import argparse
from concurrent.futures import ThreadPoolExecutor, as_completed
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import threading
import time

from .gemini import atomic_json
from .server_text_helper import OwnedServer


def run_server_notes(native_runner, names, env, output, root, binary, model, *, workers=8):
    if not names:
        return
    if not 1 <= workers <= 16:
        raise ValueError('Use one to 16 native notes workers')
    output = Path(output)
    output.mkdir(parents=True, exist_ok=True)
    server = OwnedServer(binary, model, output/'continuous-server.log', env=env)
    helper = Path(__file__).with_name('server_text_helper.py').resolve()
    shared = dict(env, SONEFOLD_LLAMA_HELPER=str(helper), TOKIO_WORKER_THREADS='2',
                  OVERTALK_EVAL_SERVER=server.address,
                  OVERTALK_EVAL_SERVER_TOKEN=server.token,
                  OVERTALK_EVAL_SERVER_MODEL=server.model,
                  OVERTALK_EVAL_SERVER_CONTEXT=str(server.slot_context))
    processes, lock, cancelled = [], threading.Lock(), threading.Event()

    def run(index):
        group = names[index::workers]
        if not group:
            return
        command = [str(native_runner), 'summary::eval_long_meetings::eval_long_meetings', '--exact', '--ignored', '--nocapture']
        with (output/f'notes-worker-{index}.log').open('a') as log:
            with lock:
                if cancelled.is_set():
                    return
                process = subprocess.Popen(command, cwd=root, stdout=log, stderr=log,
                    env=dict(shared, OVERTALK_EVAL_MEETINGS=','.join(group)))
                processes.append(process)
            code = process.wait()
            if code:
                raise subprocess.CalledProcessError(code, command)

    pool = ThreadPoolExecutor(max_workers=workers)
    try:
        futures = [pool.submit(run, index) for index in range(workers)]
        for future in as_completed(futures):
            future.result()
    except BaseException:
        cancelled.set()
        with lock:
            for process in processes:
                if process.poll() is None:
                    process.terminate()
        raise
    finally:
        pool.shutdown(wait=True, cancel_futures=True)
        server.close()


def digest(path):
    with Path(path).open('rb') as source:
        return hashlib.file_digest(source, 'sha256').hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--inputs', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--native-runner', type=Path, required=True)
    parser.add_argument('--server', type=Path, required=True)
    parser.add_argument('--model', type=Path, required=True)
    parser.add_argument('--cases', nargs='+', required=True)
    parser.add_argument('--workers', type=int, default=8)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[2]
    if args.output.exists():
        parser.error('Choose a new output directory')
    files = [args.inputs/f'meeting-{name}.json' for name in args.cases]
    if any(not path.is_file() for path in files):
        parser.error('A requested frozen input is missing')
    args.output.mkdir(parents=True)
    inputs = args.output/'inputs'
    inputs.mkdir()
    for path in files:
        shutil.copyfile(path, inputs/path.name)
    installation = json.loads((root/'evaluations/meetingbench/.runtime/evaluation-install.json').read_text())['path']
    env = dict(os.environ, OVERTALK_EVAL_APP_DIR=str(Path.home()/'.local/share/com.judeam.sonefold'),
        OVERTALK_EVAL_INPUT_DIR=str(inputs.resolve()), OVERTALK_EVAL_OUT=str((args.output/'notes').resolve()),
        OVERTALK_EVAL_MODELS='qwen3.5:4b', OVERTALK_EVAL_MODES='app',
        LD_LIBRARY_PATH='/tmp/overtalk-ort-test-libs:'+installation+'/lib')
    atomic_json(args.output/'configuration.json', dict(cases=args.cases, workers=args.workers,
        input_sha256={p.name: digest(p) for p in files}, model_sha256=digest(args.model),
        server_sha256=digest(args.server), native_runner_sha256=digest(args.native_runner),
        code_sha256={name: digest(Path(__file__).with_name(name+'.py')) for name in
                     ['continuous_notes', 'server_text_helper', 'vllm_text_helper']},
        model='qwen3.5:4b', backend='same-library-llama-server', independent=False))
    began = time.perf_counter()
    run_server_notes(args.native_runner.resolve(), args.cases, env, args.output, root,
                     args.server.resolve(), args.model.resolve(), workers=args.workers)
    notes = list((args.output/'notes').glob('*.md'))
    errors = [p.name for p in notes if p.read_text().startswith('ERROR:')]
    result = dict(seconds=time.perf_counter()-began, expected=len(args.cases), completed=len(notes),
                  errors=errors, success=len(notes)==len(args.cases) and not errors,
                  end_to_end_speedup_claim=False)
    atomic_json(args.output/'timing.json', result)
    print(json.dumps(result))


if __name__ == '__main__':
    main()
