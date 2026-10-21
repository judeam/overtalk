"""Compare uniform and predicted-speaker-bounded ASR windows on real audio.

No reference timing or speaker name enters inference. This isolates whether a
30-second mixed-speaker context is causing recognition errors. Only raw ASR is
scored; no word-alignment or speaker-identity improvement is claimed here.
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
from .run_evaluation import ROOT, RUNTIME


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--ids', nargs='+', required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--minimum-seconds', type=float, default=.5,
                        help='Minimum guided window; keep short turns with their neighbours')
    parser.add_argument('--max-seconds', type=float, default=300,
                        help='Explicit limit for longer development probes, at most 900 seconds')
    parser.add_argument('--chunk-seconds', type=int, default=30,
                        help='Control the maximum audio window for a language-switching ablation (4–30)')
    parser.add_argument('--guided-window-seconds', type=int, default=30,
                        help='Cap only the speaker-guided windows; preserve the uniform control (4–30)')
    args = parser.parse_args()
    if not .5 <= args.minimum_seconds <= 10:
        parser.error('Minimum window must be between 0.5 and 10 seconds')
    if not 0 < args.max_seconds <= 900:
        parser.error('Maximum duration must be between zero and 900 seconds')
    if not 4 <= args.chunk_seconds <= 30:
        parser.error('Audio chunk duration must be 4–30 seconds')
    if not 4 <= args.guided_window_seconds <= 30:
        parser.error('Guided window duration must be 4–30 seconds')
    if args.output.exists():
        parser.error('Use a new output directory')
    refs = []
    for key in args.ids:
        if Path(key).name != key or key in {'.', '..'}:
            parser.error('Invalid case ID')
        ref = json.loads((RUNTIME/'corpus'/key/'reference.json').read_text())
        if ref['duration_seconds'] > args.max_seconds:
            parser.error('Case exceeds the explicit duration limit')
        refs.append(ref)
    sys.path.insert(0, str(ROOT/'speech-worker'))
    from sonefold_speech.asr import ASREngine, SAMPLE_RATE, audio_windows, split_speaker_window
    from sonefold_speech.diarization import speaker_change_boundaries
    import torch
    torch.set_num_threads(4)
    settings = json.loads((Path.home()/'.local/share/com.judeam.sonefold/speech-runtime.json').read_text())
    os.environ.update(SONEFOLD_QWEN3_ASR_1_7B_DIR=settings['asr_model_dir'],
                      HF_HUB_OFFLINE='1', TRANSFORMERS_OFFLINE='1', HF_HUB_DISABLE_TELEMETRY='1')
    engine = ASREngine('cuda')
    engine.load('qwen3-asr-1.7b')
    engine._model.max_inference_batch_size = 4
    args.output.mkdir(parents=True)
    atomic_json(args.output/'configuration.json', dict(
        ids=args.ids, model='qwen3-asr-1.7b', batch=4, minimum_seconds=args.minimum_seconds,
        maximum_seconds=args.max_seconds,
        audio_chunk_seconds=args.chunk_seconds,
        guided_window_seconds=args.guided_window_seconds,
        script_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
        limitations=['Predicted diarization only; no oracle boundaries.',
                     'Raw ASR experiment, no new identity or alignment score.',
                     'Development cases, not final acceptance data.']))
    try:
        for ref in refs:
            base = json.loads((RUNTIME/'results'/ref['id']/'transcript.json').read_text())
            boundaries = speaker_change_boundaries([dict(speaker=s, start=a, end=b) for s,a,b in base['turns']])
            uniform = []
            for samples, offset, duration in audio_windows(Path(ref['audio_path'])):
                cuts = [offset + delta for delta in range(args.chunk_seconds, 30, args.chunk_seconds)
                        if delta < duration]
                uniform.extend(split_speaker_window(samples, offset, duration, cuts))
            guided = []
            for samples, offset, duration in uniform:
                kept, previous = [], offset
                cuts = sorted(set(boundaries + [offset + delta
                    for delta in range(args.guided_window_seconds, 30, args.guided_window_seconds)
                    if delta < duration]))
                for cut in cuts:
                    if cut - previous >= args.minimum_seconds and offset + duration - cut >= args.minimum_seconds:
                        kept.append(cut)
                        previous = cut
                guided.extend(split_speaker_window(samples, offset, duration, kept))
            if sum(len(w[0]) for w in uniform) != sum(len(w[0]) for w in guided):
                raise ValueError('Speaker partitioning changed the audio sample count')
            for label, windows in [('uniform', uniform), ('speaker_bounded', guided)]:
                began = time.perf_counter()
                segments = []
                for i in range(0, len(windows), 4):
                    batch = windows[i:i+4]
                    predicted = engine._model.transcribe(audio=[(w[0], SAMPLE_RATE) for w in batch],
                                                         language=None, return_time_stamps=False)
                    for (_, start, duration), value in zip(batch, predicted):
                        segments.append(dict(text=value.text.strip(), start=start, end=start+duration,
                                             language=value.language))
                score = transcript_score('\n'.join(t['text'] for t in ref['turns']),
                                         '\n'.join(x['text'] for x in segments), ref['languages'])
                result = dict(id=ref['id'], mode=label, segments=segments, metric=score,
                              seconds=time.perf_counter()-began, windows=len(windows))
                atomic_json(args.output/f'{ref["id"]}-{label}.json', result)
                print(json.dumps({k:result[k] for k in ['id','mode','seconds','windows']} |
                                 {'primary':score[score['primary_metric']]}), flush=True)
    finally:
        engine.unload()


if __name__ == '__main__':
    main()
