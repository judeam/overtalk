"""CPU-only, reference-assisted voice separability diagnostic; not diarization.

Uses known clean speaker intervals to isolate encoder errors from segmentation.
Requires an explicitly supplied, reviewed 3D-Speaker checkout and local weights.
Never downloads models, changes the corpus, or supplies identities to the app.
"""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import resource
import sys
import time

from .gemini import atomic_json
from .run_evaluation import RUNTIME


def clean_fragments(turns, seconds=3.0, margin=.2):
    """Nonoverlapping fixed crops, excluding any reference overlap and edges."""
    samples = []
    for turn in turns:
        intervals = [(turn['start_seconds'] + margin, turn['end_seconds'] - margin)]
        for other in turns:
            if other is turn or other['speaker'] == turn['speaker']:
                continue
            lo, hi = other['start_seconds'] - margin, other['end_seconds'] + margin
            next_intervals = []
            for a, b in intervals:
                if hi <= a or lo >= b:
                    next_intervals.append((a, b))
                else:
                    if a < lo:
                        next_intervals.append((a, min(lo, b)))
                    if hi < b:
                        next_intervals.append((max(hi, a), b))
            intervals = next_intervals
        for a, b in intervals:
            while a + seconds <= b:
                samples.append(dict(speaker=turn['speaker'], turn=turn['id'],
                                    start=a, end=a + seconds))
                a += seconds
    return samples


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--ids', nargs='+', required=True)
    parser.add_argument('--source-dir', type=Path, required=True)
    parser.add_argument('--model-dir', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    if args.output.exists():
        parser.error('Choose a fresh output directory to preserve previous results')
    for key in args.ids:
        if Path(key).name != key or key in {'.', '..'}:
            parser.error('Invalid case ID')
    source = args.source_dir.resolve()
    checkpoint = args.model_dir / 'pretrained_eres2netv2.ckpt'
    expected = '0eb4057106b2573dd7b132cf0c36273ab29afd192c1610f80baa9c556dbb963c'
    if hashlib.sha256(checkpoint.read_bytes()).hexdigest() != expected:
        raise ValueError('Checkpoint does not match reviewed ERes2NetV2 release')
    sys.path.insert(0, str(source))
    import numpy as np
    import soundfile as sf
    import torch
    import torchaudio.compliance.kaldi as kaldi
    from scipy.signal import resample_poly
    from speakerlab.models.eres2net.ERes2NetV2 import ERes2NetV2
    from math import gcd
    torch.set_num_threads(4)
    load_started = time.perf_counter()
    model = ERes2NetV2(feat_dim=80, embedding_size=192, baseWidth=26, scale=2, expansion=2)
    model.load_state_dict(torch.load(checkpoint, weights_only=True, mmap=True,
                                    map_location='cpu'), strict=True)
    model.eval()
    args.output.mkdir(parents=True)
    code_files = ['ERes2NetV2.py', 'fusion.py', 'pooling_layers.py']
    atomic_json(args.output / 'configuration.json', dict(
        ids=args.ids, checkpoint_sha256=expected, device='cpu', crop_seconds=3.0,
        source_sha256={f: hashlib.sha256((source / 'speakerlab/models/eres2net' / f).read_bytes()).hexdigest()
                       for f in code_files}, load_seconds=time.perf_counter() - load_started,
        caveats=['ORACLE reference speaker boundaries and identities are used.',
                 'Reference intervals do not guarantee active speech throughout each crop.',
                 'First clean crop enrolls a speaker; tests use different turns only.',
                 'This does not measure automatic speaker discovery or diarization.']))
    with torch.inference_mode():
        for key in args.ids:
            ref = json.loads((RUNTIME / 'corpus' / key / 'reference.json').read_text())
            wav, rate = sf.read(ref['audio_path'], dtype='float32', always_2d=True)
            wav = wav.mean(axis=1)
            if rate != 16000:
                divisor = gcd(rate, 16000)
                wav = resample_poly(wav, 16000 // divisor, rate // divisor).astype('float32')
            samples = clean_fragments(ref['turns'])
            vectors = []
            started = time.perf_counter()
            for sample in samples:
                audio = torch.from_numpy(wav[round(sample['start'] * 16000):round(sample['end'] * 16000)].copy())
                features = kaldi.fbank(audio[None], num_mel_bins=80, sample_frequency=16000, dither=0)
                features -= features.mean(0, keepdim=True)
                vector = model(features[None])[0].numpy()
                vectors.append(vector / max(np.linalg.norm(vector), 1e-10))
            vectors = np.asarray(vectors)
            elapsed = time.perf_counter() - started
            enroll = {}
            for i, sample in enumerate(samples):
                enroll.setdefault(sample['speaker'], i)
            speaker_ids = sorted(enroll)
            enrollment_turns = {samples[i]['turn'] for i in enroll.values()}
            tests = [i for i, sample in enumerate(samples) if sample['turn'] not in enrollment_turns]
            matrix = vectors @ vectors[[enroll[k] for k in speaker_ids]].T
            outcomes = []
            for i in tests:
                ranking = np.argsort(matrix[i])[::-1]
                first, second = int(ranking[0]), int(ranking[1])
                outcomes.append(dict(**samples[i], predicted=speaker_ids[first],
                                     correct=samples[i]['speaker'] == speaker_ids[first],
                                     similarity=float(matrix[i, first]),
                                     margin=float(matrix[i, first] - matrix[i, second])))
            np.savez_compressed(args.output / f'{key}-vectors.npz', vectors=vectors)
            atomic_json(args.output / f'{key}-samples.json', samples)
            result = dict(id=key, reference_people=len(ref['people']), enrolled=len(enroll),
                          test_fragments=len(outcomes), correct=sum(x['correct'] for x in outcomes),
                          encoder_seconds=elapsed, fragments=len(samples),
                          peak_rss_kib=resource.getrusage(resource.RUSAGE_SELF).ru_maxrss,
                          per_speaker={k: dict(total=sum(x['speaker'] == k for x in outcomes),
                                               correct=sum(x['speaker'] == k and x['correct'] for x in outcomes))
                                       for k in speaker_ids}, outcomes=outcomes)
            atomic_json(args.output / f'{key}-score.json', result)
            print(json.dumps({k: v for k, v in result.items() if k != 'outcomes'}), flush=True)


if __name__ == '__main__':
    main()
