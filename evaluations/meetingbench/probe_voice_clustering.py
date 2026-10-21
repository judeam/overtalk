"""CPU embedding clustering ablation using predicted speech activity only.

Unlike the enrollment probe, no reference speaker boundaries or names enter
feature extraction or clustering. Fixed threshold candidates are diagnostics;
none is automatically selected or installed based on their reference scores.
"""
import argparse
import hashlib
import json
from pathlib import Path
import resource
import sys
import time

from .gemini import atomic_json
from .review_preview import interval_diagnostics
from .run_evaluation import RUNTIME


def activity_windows(turns, window=2.5, step=1.25):
    union = []
    for _, start, end in sorted(turns, key=lambda t: t[1]):
        if union and start <= union[-1][1] + .2:
            union[-1][1] = max(union[-1][1], end)
        else:
            union.append([start, end])
    samples = []
    for region, (start, end) in enumerate(union):
        if end-start < 1:
            continue
        centers = [start + min(window, end-start)/2]
        while centers[-1] + step < end - window/2:
            centers.append(centers[-1]+step)
        last = max(start+window/2, end-window/2)
        if last > centers[-1] + .2 and end-start > window:
            centers.append(last)
        for i, center in enumerate(centers):
            samples.append(dict(start=max(start, center-window/2), end=min(end, center+window/2),
                                region=region, output_start=start if i == 0 else (centers[i-1]+center)/2,
                                output_end=end if i == len(centers)-1 else (center+centers[i+1])/2))
    return samples


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--ids', nargs='+', required=True)
    parser.add_argument('--source-dir', type=Path, required=True)
    parser.add_argument('--model-dir', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    if args.output.exists():
        parser.error('Use a new output directory')
    expected = '0eb4057106b2573dd7b132cf0c36273ab29afd192c1610f80baa9c556dbb963c'
    checkpoint = args.model_dir / 'pretrained_eres2netv2.ckpt'
    if hashlib.sha256(checkpoint.read_bytes()).hexdigest() != expected:
        raise ValueError('Checkpoint differs from reviewed release')
    sys.path.insert(0, str(args.source_dir.resolve()))
    import numpy as np
    import soundfile as sf
    from scipy.cluster.hierarchy import linkage, fcluster
    from scipy.signal import resample_poly
    from math import gcd
    import torch
    import torchaudio.compliance.kaldi as kaldi
    from speakerlab.models.eres2net.ERes2NetV2 import ERes2NetV2
    torch.set_num_threads(4)
    model = ERes2NetV2(feat_dim=80, embedding_size=192, baseWidth=26, scale=2, expansion=2)
    model.load_state_dict(torch.load(checkpoint, map_location='cpu', weights_only=True, mmap=True), strict=True)
    model.eval()
    args.output.mkdir(parents=True)
    atomic_json(args.output/'configuration.json', dict(
        ids=args.ids, encoder_sha256=expected, thresholds=[.3, .4, .5, .6],
        source='Predicted activity union from frozen Nemotron baseline; original audio',
        caveat='Development grid, no automatic winner; no reference labels in model input.'))
    with torch.inference_mode():
        for key in args.ids:
            if Path(key).name != key or key in {'.', '..'}:
                raise ValueError('Invalid case ID')
            ref = json.loads((RUNTIME/'corpus'/key/'reference.json').read_text())
            if ref['duration_seconds'] > 300:
                raise ValueError('Probe is bounded to five minutes')
            base = json.loads((RUNTIME/'results'/key/'transcript.json').read_text())
            samples = activity_windows(base['turns'])
            if len(samples) < 2 or len(samples) > 500:
                raise ValueError('Expected 2–500 speech windows')
            wav, rate = sf.read(ref['audio_path'], dtype='float32', always_2d=True)
            wav = wav.mean(1)
            if rate != 16000:
                divisor = gcd(rate, 16000)
                wav = resample_poly(wav, 16000//divisor, rate//divisor).astype('float32')
            vectors = []
            started = time.perf_counter()
            for row in samples:
                audio = torch.from_numpy(wav[round(row['start']*16000):round(row['end']*16000)].copy())
                features = kaldi.fbank(audio[None], num_mel_bins=80, sample_frequency=16000, dither=0)
                features -= features.mean(0, keepdim=True)
                v = model(features[None])[0].numpy()
                vectors.append(v/max(float(np.linalg.norm(v)), 1e-10))
            vectors = np.asarray(vectors)
            encode_seconds = time.perf_counter()-started
            np.savez_compressed(args.output/f'{key}-vectors.npz', vectors=vectors)
            atomic_json(args.output/f'{key}-windows.json', samples)
            hierarchy = linkage(vectors, method='average', metric='cosine')
            for threshold in [.3, .4, .5, .6]:
                labels = fcluster(hierarchy, threshold, criterion='distance')
                turns = [(str(label), w['output_start'], w['output_end']) for label, w in zip(labels, samples)]
                score = interval_diagnostics([(t['speaker'], t['start_seconds'], t['end_seconds']) for t in ref['turns']], turns)
                score.update(id=key, cosine_distance_threshold=threshold, clusters=len(set(labels)),
                             reference_people=len(ref['people']), windows=len(samples),
                             encoder_seconds=encode_seconds, peak_rss_kib=resource.getrusage(resource.RUSAGE_SELF).ru_maxrss)
                atomic_json(args.output/f'{key}-{threshold}-score.json', score)
                atomic_json(args.output/f'{key}-{threshold}-turns.json', turns)
                print(json.dumps({k:score[k] for k in ['id','cosine_distance_threshold','clusters',
                      'approximate_diarization_error_rate','encoder_seconds']}), flush=True)


if __name__ == '__main__':
    main()
