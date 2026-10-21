"""Use an independent local acoustic model to score vocabulary-assisted ASR.

Experimental development diagnostic. CTC likelihood is not transcript accuracy;
shorter text can receive higher likelihood, so every chosen transcript is scored
against the unchanged reference afterward. Nothing is applied to the app.
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
    parser.add_argument('--predictions', type=Path, required=True)
    parser.add_argument('--model-dir', type=Path, required=True)
    parser.add_argument('--ids', nargs='+', required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    if args.output.exists():
        parser.error('Use a new output directory')
    sys.path.insert(0, str(ROOT / 'speech-worker'))
    from sonefold_speech.omni_alignment import OmniAlignment, normalized_alignment_text
    from sonefold_speech.asr import audio_windows
    from sonefold_speech.ctc import sequence_log_probability
    import torch
    torch.set_num_threads(4)
    os.environ.update(HF_HUB_OFFLINE='1', TRANSFORMERS_OFFLINE='1', HF_HUB_DISABLE_TELEMETRY='1')
    model = OmniAlignment(args.model_dir)
    args.output.mkdir(parents=True)
    atomic_json(args.output/'configuration.json', dict(
        ids=args.ids, minimum_total_log_probability_margin=1.0, allowed_length_ratio=[.85, 1.15],
        selection_inputs=['audio', 'no-context prediction', 'project-context prediction'],
        script_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
        limitations=['Development probe, no calibrated correctness probability.',
                     'Length bounds are heuristics; no reference labels enter selection.']))
    with torch.inference_mode():
        for key in args.ids:
            if Path(key).name != key or key in {'.', '..'}:
                raise ValueError('Invalid case ID')
            ref = json.loads((RUNTIME/'corpus'/key/'reference.json').read_text())
            if ref['duration_seconds'] > 300:
                raise ValueError('Probe is limited to five minutes')
            predictions = {name: json.loads((args.predictions/f'{key}-{name}.json').read_text())['segments']
                           for name in ['no_context', 'project_context']}
            windows = list(audio_windows(Path(ref['audio_path'])))
            if any(len(rows) != len(windows) for rows in predictions.values()):
                raise ValueError('Candidate windows differ from the source windowing')
            selected, diagnostics = [], []
            began = time.perf_counter()
            for index, (samples, start, duration) in enumerate(windows):
                texts = {name: rows[index]['text'] for name, rows in predictions.items()}
                if any(abs(rows[index]['start'] - start) > .01 or
                       abs(rows[index]['end'] - (start+duration)) > .01 for rows in predictions.values()):
                    raise ValueError('Candidate time bounds differ')
                normalized = {name: normalized_alignment_text(text) for name, text in texts.items()}
                winner, scores = 'no_context', {}
                if len(set(normalized.values())) > 1:
                    audio = torch.from_numpy(samples.copy())[None]
                    audio = torch.nn.functional.layer_norm(audio, audio.shape[-1:])
                    emissions = model.model(audio).logits[0].float().log_softmax(-1).numpy()
                    for name, text in normalized.items():
                        ids = model.tokenizer.encode(text, out_type=int)
                        if not ids or any(token in {0, 1, 2, 3} for token in ids) or model.tokenizer.decode(ids) != text:
                            continue
                        score = sequence_log_probability(emissions, ids)
                        if score != float('-inf'):
                            scores[name] = score
                    length_ratio = len(normalized['project_context']) / max(1, len(normalized['no_context']))
                    if (len(scores) == 2 and .85 <= length_ratio <= 1.15 and
                            scores['project_context'] > scores['no_context'] + 1.0):
                        winner = 'project_context'
                selected.append(texts[winner])
                diagnostics.append(dict(start=start, duration=duration, selected=winner, acoustic_scores=scores))
            result = dict(id=key, windows=diagnostics, text='\n'.join(selected), seconds=time.perf_counter()-began,
                          metric=transcript_score('\n'.join(t['text'] for t in ref['turns']),
                                                  '\n'.join(selected), ref['languages']))
            atomic_json(args.output/f'{key}.json', result)
            print(json.dumps(dict(id=key, seconds=result['seconds'],
                                  project_windows=sum(x['selected']=='project_context' for x in diagnostics),
                                  primary=result['metric'][result['metric']['primary_metric']])), flush=True)


if __name__ == '__main__':
    main()
