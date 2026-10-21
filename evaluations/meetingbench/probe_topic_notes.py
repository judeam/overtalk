"""Compare opt-in topic reading with the real native summary pipeline.

Optionally run under your own GPU-sharing launcher (such as the one named by
OVERTALK_GPU_LAUNCHER). Inputs contain only transcripts/language, never
reference answers. Outputs remain ungraded until direct review. The caller may
provide a frozen baseline runner or the current test runner for an ablation.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import time

from .batched_notes import run_native_notes


def digest(path):
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--inputs', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--native-runner', type=Path, required=True)
    parser.add_argument('--helper', type=Path, required=True)
    parser.add_argument('--variant', choices=('current', 'topic', 'quoted-decisions'), required=True)
    parser.add_argument('--cases', nargs='+', required=True)
    parser.add_argument('--workers', type=int, default=4)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[2]
    if not 1 <= args.workers <= 8:
        parser.error('Use one to eight workers')
    if args.output.exists():
        parser.error('Choose a new output directory to preserve frozen comparisons')
    if len(set(args.cases)) != len(args.cases) or any(
            not name or any(c not in 'abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_-' for c in name)
            for name in args.cases):
        parser.error('Cases must be distinct safe input names')
    files = [args.inputs / f'meeting-{name}.json' for name in args.cases]
    if any(not path.is_file() for path in files) or not args.native_runner.is_file() or not args.helper.is_file():
        parser.error('Missing frozen input, native runner or helper')
    for path in files:
        value = json.loads(path.read_text())
        if set(value) != {'transcript', 'language'} or not all(isinstance(v, str) for v in value.values()):
            parser.error('Probe inputs must contain only transcript and language strings')
    output = args.output.resolve()
    output.mkdir(parents=True)
    inputs = output / 'inputs'
    inputs.mkdir()
    (output / 'notes').mkdir()
    for path in files:
        shutil.copyfile(path, inputs / path.name)
    installation = json.loads((root/'evaluations/meetingbench/.runtime/evaluation-install.json').read_text())['path']
    env = dict(os.environ, OVERTALK_EVAL_APP_DIR=str(Path.home()/'.local/share/com.judeam.sonefold'),
        OVERTALK_EVAL_INPUT_DIR=str(inputs), OVERTALK_EVAL_OUT=str(output/'notes'),
        OVERTALK_EVAL_DEBUG_DIR=str(output/'debug'), OVERTALK_EVAL_MODELS='qwen3.5:4b',
        OVERTALK_EVAL_MODES='app', OVERTALK_EVAL_MEETINGS=','.join(args.cases),
        OVERTALK_EVAL_TOPIC_PLAN='1' if args.variant == 'topic' else '0',
        OVERTALK_EVAL_QUOTED_DECISIONS='1' if args.variant == 'quoted-decisions' else '0',
        SONEFOLD_LLAMA_HELPER=str(args.helper.resolve()),
        LD_LIBRARY_PATH='/tmp/overtalk-ort-test-libs:'+installation+'/lib')
    config = dict(variant=args.variant, cases=args.cases, workers=args.workers,
        native_sha256=digest(args.native_runner), helper_sha256=digest(args.helper),
        model_sha256=digest(Path(env['OVERTALK_EVAL_APP_DIR'])/'models/summary/Qwen3.5-4B-Q4_K_M.gguf'),
        input_sha256={p.name: digest(p) for p in files}, probe_sha256=digest(__file__),
        reference_answers_supplied=False, independent=False, installed=False,
        note='Sampling follows the native app; repeated runs may differ. Direct review required.')
    (output/'configuration.json').write_text(json.dumps(config, indent=2))
    began = time.perf_counter()
    run_native_notes(args.native_runner.resolve(), args.cases, env, output, root, workers=args.workers)
    notes = list((output/'notes').glob('*.md'))
    errors = [p.name for p in notes if p.read_text().startswith('ERROR:')]
    result = dict(seconds=time.perf_counter()-began, completed=len(notes), expected=len(args.cases),
                  errors=errors, success=len(notes) == len(args.cases) and not errors, quality='ungraded')
    (output/'timing.json').write_text(json.dumps(result, indent=2))
    print(json.dumps(result))


if __name__ == '__main__':
    main()
