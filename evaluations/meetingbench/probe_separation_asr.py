"""Recognition after separation on controlled two-voice calibration clips.

The separated audio comes from probe_separation. Source identities are used only
to score a global permutation after inference. This is an oracle-stem diagnostic
on artificial mixtures, not an improvement claim for natural overlapping speech.
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
from .speaker_attribution import speaker_attributed_error


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--input', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    if args.output.exists():
        parser.error('Use a new output directory')
    sys.path.insert(0, str(ROOT/'speech-worker'))
    from sonefold_speech.asr import ASREngine, audio_windows, SAMPLE_RATE
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
        model='qwen3-asr-1.7b', device='cuda', script_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
        limitations=['Artificial clean two-source calibration only.',
                     'Oracle source permutation only for scoring, never an identity supplied to ASR.',
                     'Original mixture cpWER includes attribution errors; not plain transcript accuracy.']))
    try:
        for score_path in sorted(args.input.glob('*-score.json')):
            info = json.loads(score_path.read_text())
            key = info['id']
            if Path(key).name != key or key in {'.', '..'}:
                raise ValueError('Invalid case ID')
            ref = json.loads((RUNTIME/'corpus'/key/'reference.json').read_text())
            by_id = {turn['id']: turn for turn in ref['turns']}
            source_turns = [by_id[index] for index in info['source_turns']]
            paths = {'mixture': args.input/f'{key}-mixture.wav'}
            paths.update({f'separated-{i}': args.input/f'{key}-source-{i}.wav' for i in range(2)})
            paths.update({f'clean-{i}': Path(turn['clip']) for i, turn in enumerate(source_turns)})
            began, predictions = time.perf_counter(), {}
            for label, path in paths.items():
                windows = list(audio_windows(path))
                if len(windows) != 1:
                    raise ValueError('Use short calibration clips only')
                samples, _, _ = windows[0]
                predictions[label] = engine._model.transcribe(
                    audio=(samples, SAMPLE_RATE), language=None, return_time_stamps=False)[0].text.strip()
            gold = [(str(i), turn['text']) for i, turn in enumerate(source_turns)]
            result = dict(id=key, predictions=predictions, seconds=time.perf_counter()-began,
                          joint_mixture=speaker_attributed_error(gold, [('mixture', predictions['mixture'])]),
                          joint_separated=speaker_attributed_error(gold, [(str(i), predictions[f'separated-{i}']) for i in range(2)]),
                          clean=[transcript_score(turn['text'], predictions[f'clean-{i}'], ref['languages'])
                                 for i, turn in enumerate(source_turns)],
                          input_sha256={label: hashlib.file_digest(path.open('rb'), 'sha256').hexdigest()
                                        for label, path in paths.items()})
            atomic_json(args.output/f'{key}.json', result)
            print(json.dumps(dict(id=key, seconds=result['seconds'],
                                  mixture_cpwer=result['joint_mixture']['rate'],
                                  separated_cpwer=result['joint_separated']['rate'])), flush=True)
    finally:
        engine.unload()


if __name__ == '__main__':
    main()
