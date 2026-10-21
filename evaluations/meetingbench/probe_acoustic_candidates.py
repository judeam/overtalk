"""Rescore frozen ASR candidates with an independent local acoustic model.

This is an experiment, not an automatic change to the installed app. Selection
sees only audio and candidate text. Reference text is read for scoring afterward.
The length guard deliberately keeps the same conservative bounds as the earlier
context experiment; failures remain visible rather than tuning on these cases.
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


def group_partitions(parents, parts):
    groups, index = [], 0
    for parent in parents:
        start, end = parent['start'], parent['end']
        cursor, text = start, []
        while index < len(parts) and parts[index]['start'] < end - 1e-6:
            part = parts[index]
            if abs(part['start'] - cursor) > .001 or part['end'] > end + .001:
                raise ValueError('Candidate partitions do not preserve the source window')
            text.append(part['text'])
            cursor = part['end']
            index += 1
        if abs(cursor - end) > .001:
            raise ValueError('Candidate is missing source audio')
        groups.append(' '.join(text))
    if index != len(parts):
        raise ValueError('Candidate contains extra source audio')
    return groups


def choose_candidate(normalized, scores):
    baseline = scores.get('uniform')
    if baseline is None:
        return 'uniform'
    eligible = [name for name, score in scores.items()
                if .85 <= len(normalized[name]) / max(1, len(normalized['uniform'])) <= 1.15
                and score > baseline + 1.0]
    return max(eligible, key=lambda name: scores[name]) if eligible else 'uniform'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--ids', required=True, nargs='+')
    parser.add_argument('--context-dir', required=True, type=Path)
    parser.add_argument('--speaker-dir', required=True, type=Path)
    parser.add_argument('--model-dir', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path)
    args = parser.parse_args()
    if args.output.exists():
        parser.error('Use a new output directory')
    sys.path.insert(0, str(ROOT/'speech-worker'))
    from sonefold_speech.asr import audio_windows
    from sonefold_speech.omni_alignment import OmniAlignment, normalized_alignment_text
    from sonefold_speech.ctc import sequence_log_probability
    import torch
    torch.set_num_threads(4)
    os.environ.update(HF_HUB_OFFLINE='1', TRANSFORMERS_OFFLINE='1', HF_HUB_DISABLE_TELEMETRY='1')
    model = OmniAlignment(args.model_dir)
    args.output.mkdir(parents=True)
    atomic_json(args.output/'configuration.json', dict(
        ids=args.ids, minimum_margin=1.0, length_ratio=[.85, 1.15],
        script_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
        caveat='Development candidates only; not calibrated confidence or final acceptance.'))
    with torch.inference_mode():
        for key in args.ids:
            if Path(key).name != key or key in {'.', '..'}:
                raise ValueError('Invalid case ID')
            ref_path = RUNTIME/'corpus'/key/'reference.json'
            metadata = json.loads(ref_path.read_text())
            if metadata['duration_seconds'] > 300:
                raise ValueError('Only short development recordings are supported')
            paths = dict(uniform=args.speaker_dir/f'{key}-uniform.json',
                         speaker_bounded=args.speaker_dir/f'{key}-speaker_bounded.json',
                         context=args.context_dir/f'{key}-project_context.json')
            data = {name: json.loads(path.read_text())['segments'] for name, path in paths.items()}
            parents = data['uniform']
            texts = {name: group_partitions(parents, segments) for name, segments in data.items()}
            windows = list(audio_windows(Path(metadata['audio_path'])))
            if len(windows) != len(parents):
                raise ValueError('Audio windows changed since candidate generation')
            began, selected, diagnostics = time.perf_counter(), [], []
            for index, (samples, start, duration) in enumerate(windows):
                if abs(start - parents[index]['start']) > .001 or abs(start+duration-parents[index]['end']) > .001:
                    raise ValueError('Source audio windows changed')
                raw = {name: candidate[index] for name, candidate in texts.items()}
                normalized = {name: normalized_alignment_text(text) for name, text in raw.items()}
                scores = {}
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
                winner = choose_candidate(normalized, scores)
                selected.append(raw[winner])
                diagnostics.append(dict(start=start, duration=duration, selected=winner, scores=scores,
                                        character_counts={name: len(text) for name, text in normalized.items()}))
            # Gold text is consulted only after all selections for this case.
            reference = '\n'.join(turn['text'] for turn in metadata['turns'])
            result = dict(id=key, windows=diagnostics, text='\n'.join(selected),
                          seconds=time.perf_counter()-began,
                          input_sha256={name: hashlib.sha256(path.read_bytes()).hexdigest() for name, path in paths.items()},
                          metric=transcript_score(reference, '\n'.join(selected), metadata['languages']))
            atomic_json(args.output/f'{key}.json', result)
            print(json.dumps(dict(id=key, seconds=result['seconds'],
                                  primary=result['metric'][result['metric']['primary_metric']])), flush=True)


if __name__ == '__main__':
    main()
