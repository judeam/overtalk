"""Evaluation-only warm Nemotron runner using the unchanged worker algorithm."""
import argparse
import contextlib
import json
import os
from pathlib import Path
import re
import sys
import time

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / 'speech-worker'))
from .gemini import atomic_json


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--manifest', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--model', type=Path, required=True)
    args = parser.parse_args()
    entries = json.loads(args.manifest.read_text())
    for entry in entries:
        if not re.fullmatch(r'[a-zA-Z0-9_-]+', entry['id']):
            raise ValueError('Invalid case ID')
    os.environ.update(HF_HUB_OFFLINE='1', TRANSFORMERS_OFFLINE='1', HF_HUB_DISABLE_TELEMETRY='1')
    import soundfile
    from sonefold_speech.asr import decodable_audio, local_audio_path
    from sonefold_speech.diarization import diarize
    cached = []
    with contextlib.redirect_stdout(sys.stderr):
        from nemo.collections.asr.models import SortformerEncLabelModel
        original = SortformerEncLabelModel.restore_from

        def reuse(path, *values, **options):
            if not cached:
                cached.append(original(path, *values, **options))
            return cached[0]

        # The isolated process only serves this one pinned model. The existing
        # diarize function retains its preprocessing, configuration and output
        # normalization; only repeated deserialization is avoided.
        SortformerEncLabelModel.restore_from = staticmethod(reuse)
        args.output.mkdir(parents=True, exist_ok=True)
        for entry in entries:
            target = args.output / (entry['id'] + '.json')
            if target.exists():
                continue
            started = time.perf_counter()
            try:
                with decodable_audio(local_audio_path(entry['audio_path']), soundfile) as decoded:
                    result = diarize(str(decoded), model_dir=str(args.model), device='cuda', model='nemotron-3')
                result.update(id=entry['id'], status='completed')
            except Exception as exc:
                result = dict(id=entry['id'], status='failed', error=str(exc))
            result['seconds'] = time.perf_counter() - started
            atomic_json(target, result)
            print(json.dumps({k: result[k] for k in ['id','status','seconds']}), file=sys.__stdout__, flush=True)


if __name__ == '__main__':
    main()
