"""Controlled ASR vocabulary ablation, without changing installed defaults.

Project-context conditions simulate a supplied agenda/roster. They are labeled
assisted; no reference transcript, facts, quantities or expected answers enter
the model prompt. The irrelevant condition tests false contextual insertions.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import sys
import time

from .gemini import atomic_json
from .metrics import transcript_score
from .run_evaluation import RUNTIME, ROOT


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--ids', nargs='+', required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    if args.output.exists():
        parser.error('Use a new output directory')
    refs = []
    for key in args.ids:
        if Path(key).name != key or key in {'.', '..'}:
            parser.error('Invalid case ID')
        ref = json.loads((RUNTIME / 'corpus' / key / 'reference.json').read_text())
        if ref['duration_seconds'] > 300:
            parser.error('Use short development cases, up to five minutes')
        refs.append(ref)
    sys.path.insert(0, str(ROOT / 'speech-worker'))
    from sonefold_speech.asr import ASREngine, SAMPLE_RATE, audio_windows
    import torch
    torch.set_num_threads(4)
    settings = json.loads((Path.home() / '.local/share/com.judeam.sonefold/speech-runtime.json').read_text())
    os.environ.update(SONEFOLD_QWEN3_ASR_1_7B_DIR=settings['asr_model_dir'],
                      HF_HUB_OFFLINE='1', TRANSFORMERS_OFFLINE='1', HF_HUB_DISABLE_TELEMETRY='1')
    engine = ASREngine('cuda')
    engine.load('qwen3-asr-1.7b')
    engine._model.max_inference_batch_size = 4
    args.output.mkdir(parents=True)
    atomic_json(args.output / 'configuration.json', dict(
        ids=args.ids, model='qwen3-asr-1.7b', device='cuda', batch=4,
        script_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
        caveats=['Development ablation, not held-out evaluation.',
                 'Project context assumes the agenda and roster were supplied by the user.',
                 'Reference text is used only after inference for scoring.',
                 'Timing is under shared machine load; word timing and diarization are not evaluated.']))
    try:
        for ref in refs:
            windows = list(audio_windows(Path(ref['audio_path'])))
            vocab = [a['topic'] for a in ref['agendas']] + [p['name'] for p in ref['people']]
            contexts = dict(no_context='', project_context='Key terms: ' + ', '.join(vocab),
                            irrelevant_context='Key terms: Beacon 99, Agnes, Tobias, 7300 dollars')
            for condition, context in contexts.items():
                began = time.perf_counter()
                segments = []
                torch.cuda.reset_peak_memory_stats()
                for offset in range(0, len(windows), 4):
                    batch = windows[offset:offset + 4]
                    predictions = engine._model.transcribe(
                        audio=[(w[0], SAMPLE_RATE) for w in batch], context=context,
                        language=None, return_time_stamps=False)
                    for (_, start, duration), result in zip(batch, predictions):
                        segments.append(dict(text=result.text.strip(), start=start,
                                             end=start+duration, language=result.language))
                elapsed = time.perf_counter() - began
                score = transcript_score('\n'.join(t['text'] for t in ref['turns']),
                                         '\n'.join(s['text'] for s in segments), ref['languages'])
                atomic_json(args.output / f'{ref["id"]}-{condition}.json', dict(
                    id=ref['id'], condition=condition, context=context, segments=segments,
                    metrics=score, seconds=elapsed, peak_gpu_bytes=torch.cuda.max_memory_allocated()))
                print(json.dumps(dict(id=ref['id'], condition=condition, seconds=elapsed,
                                      primary=score[score['primary_metric']])), flush=True)
    finally:
        engine.unload()


if __name__ == '__main__':
    main()
