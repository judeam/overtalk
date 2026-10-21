"""Two-source separation calibration on existing synthetic clips, CPU only.

The controlled mixtures here are diagnostics, not additional benchmark meetings.
They use clean source stems; results cannot establish real meeting performance.
No original recording or transcript is modified, and no weights are downloaded.
"""
import argparse
import hashlib
import itertools
import json
from pathlib import Path
import time

from .gemini import atomic_json
from .run_evaluation import RUNTIME
from .resources import peak_rss_kib


def write_separation_audio(directory, key, mixture, result, permutation, rate=8000):
    """Preserve the input and normalize output gain without PCM saturation."""
    import numpy as np
    import soundfile as sf
    if (result.ndim != 2 or result.shape[0] != len(mixture)
            or not np.isfinite(result).all() or not np.isfinite(mixture).all()):
        raise ValueError('Invalid separation audio')
    peaks = np.maximum(np.max(np.abs(result), axis=0), 1e-8)
    normalized = result / peaks[None]
    sf.write(directory/f'{key}-mixture.wav', mixture, rate, subtype='FLOAT')
    for index, channel in enumerate(permutation):
        sf.write(directory/f'{key}-source-{index}.wav', normalized[:, channel], rate, subtype='FLOAT')
    return peaks


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--model-dir', required=True, type=Path)
    parser.add_argument('--ids', required=True, nargs='+')
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--single-speaker-control', action='store_true')
    args = parser.parse_args()
    if args.output.exists():
        parser.error('Use a fresh output directory')
    files = {'encoder.ckpt': '740d082e75b92370bc12f176d5b5554f6383cf8bc607da8ac8a550255987ed4a',
             'decoder.ckpt': '05340634088efb00ec448f548af107d5d932057ea214dd64497f34106247e184',
             'masknet.ckpt': 'f54762647082a8fe75a1cacc6c3d463b967223fc77c836cc4fb8ca2a272d9a75'}
    for name, digest in files.items():
        if hashlib.sha256((args.model_dir/name).read_bytes()).hexdigest() != digest:
            raise ValueError('Separation checkpoint mismatch: '+name)
    import numpy as np
    import soundfile as sf
    import torch
    from scipy.signal import resample_poly
    from math import gcd
    from speechbrain.lobes.models.dual_path import Encoder, Decoder, SBTransformerBlock, Dual_Path_Model
    torch.set_num_threads(4)
    encoder = Encoder(kernel_size=16, out_channels=256)
    intra = SBTransformerBlock(8, 256, 8, d_ffn=1024, dropout=0,
                               use_positional_encoding=True, norm_before=True)
    inter = SBTransformerBlock(8, 256, 8, d_ffn=1024, dropout=0,
                               use_positional_encoding=True, norm_before=True)
    masknet = Dual_Path_Model(256, 256, intra, inter, num_layers=2, K=250, norm='ln',
                              linear_layer_after_inter_intra=False, skip_around_intra=True, num_spks=2)
    decoder = Decoder(in_channels=256, out_channels=1, kernel_size=16, stride=8, bias=False)
    for name, module in [('encoder', encoder), ('masknet', masknet), ('decoder', decoder)]:
        module.load_state_dict(torch.load(args.model_dir/f'{name}.ckpt', weights_only=True,
                                         map_location='cpu', mmap=True), strict=True)
        module.eval()
    args.output.mkdir(parents=True)
    atomic_json(args.output/'configuration.json', dict(
        model='speechbrain/sepformer-libri2mix', revision='eb43c5bfbb2aa654630adbf849373bcec0a20ed4',
        checkpoint_sha256=files, ids=args.ids, sample_rate=8000, device='cpu',
        script_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
        single_speaker_control=args.single_speaker_control,
        output_audio='Float WAV; each separated stream peak-normalized as in SpeechBrain separate_file',
        caveat='Artificial two-source clean calibration, not the 200-meeting score. No transcription claim.'))

    def si_sdr(reference, hypothesis):
        reference, hypothesis = reference-reference.mean(), hypothesis-hypothesis.mean()
        target = reference * (np.dot(reference, hypothesis)/max(1e-12, np.dot(reference, reference)))
        return float(10*np.log10(max(1e-12, np.dot(target, target))/max(1e-12, np.sum((hypothesis-target)**2))))

    with torch.inference_mode():
        for key in args.ids:
            if Path(key).name != key or key in {'.', '..'}:
                raise ValueError('Invalid case ID')
            ref = json.loads((RUNTIME/'corpus'/key/'reference.json').read_text())
            speakers, clips, clip_ids = set(), [], []
            for turn in ref['turns']:
                if turn['speaker'] in speakers:
                    continue
                speakers.add(turn['speaker'])
                wave, rate = sf.read(turn['clip'], dtype='float32', always_2d=True)
                wave = wave.mean(1)
                divisor = gcd(rate, 8000)
                wave = resample_poly(wave, 8000//divisor, rate//divisor).astype('float32')
                if len(wave) > 12*8000:
                    raise ValueError('Choose clips of at most twelve seconds')
                clips.append(wave)
                clip_ids.append(turn['id'])
                if len(clips) == 2:
                    break
            if len(clips) != 2:
                raise ValueError('Two distinct source speakers required')
            size = max(len(clips[0]), len(clips[1])+8000)
            sources = np.zeros((size, 2), dtype='float32')
            sources[:len(clips[0]), 0] = clips[0]
            sources[8000:8000+len(clips[1]), 1] = clips[1]
            if args.single_speaker_control:
                sources[:, 1] = 0
            mixture = sources.sum(1)
            started = time.perf_counter()
            encoded = encoder(torch.from_numpy(mixture)[None])
            masked = masknet(encoded) * encoded[None]
            result = torch.stack([decoder(masked[i])[0] for i in range(2)], dim=1).numpy()
            result = np.pad(result, ((0, max(0, size-len(result))), (0, 0)))[:size]
            elapsed = time.perf_counter()-started
            if args.single_speaker_control:
                active = int(np.argmax(np.sum(result**2, axis=0)))
                permutation = (active, 1-active)
            else:
                permutation = max(itertools.permutations(range(2)), key=lambda p: sum(si_sdr(sources[:, i], result[:, p[i]]) for i in range(2)))
            baseline = [si_sdr(sources[:, i], mixture) for i in range(2)]
            separated = [si_sdr(sources[:, i], result[:, permutation[i]]) for i in range(2)]
            # Match SpeechBrain separate_file normalization. Network outputs can
            # have high arbitrary gain; writing them directly as PCM16 clips
            # speech and invalidates downstream recognition comparisons.
            peaks = write_separation_audio(args.output, key, mixture, result, permutation)
            score = dict(id=key, source_turns=clip_ids, baseline_si_sdr=baseline, separated_si_sdr=separated,
                         improvement_db=[b-a for a,b in zip(baseline,separated)], seconds=elapsed,
                         audio_seconds=size/8000, peak_rss_kib=peak_rss_kib())
            if args.single_speaker_control:
                score = dict(id=key, single_speaker_control=True, source_turns=[clip_ids[0]],
                             retained_si_sdr=si_sdr(sources[:, 0], result[:, permutation[0]]),
                             residual_energy_ratio=float(np.sum(result[:, permutation[1]]**2) / max(1e-12, np.sum(sources[:, 0]**2))),
                             seconds=elapsed, audio_seconds=size/8000,
                             peak_rss_kib=peak_rss_kib())
            score['raw_output_peaks'] = peaks.tolist()
            atomic_json(args.output/f'{key}-score.json', score)
            print(json.dumps(score), flush=True)


if __name__ == '__main__':
    main()
