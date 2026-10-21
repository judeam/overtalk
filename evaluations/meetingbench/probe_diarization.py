"""Bounded, isolated comparison of complete-context and streaming diarization.

These are development probes, not production settings. Reference intervals are
used only by the scorer; the model receives only the original audio.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import time

from .gemini import atomic_json
from .review_preview import interval_diagnostics
from .run_evaluation import RUNTIME


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--ids', nargs='+', required=True)
    parser.add_argument('--mode', choices=['full', 'streaming'], required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    if args.output.exists():
        parser.error('Use a new output directory')
    references = []
    for key in args.ids:
        if Path(key).name != key or key in {'.', '..'}:
            parser.error('Invalid case ID')
        ref = json.loads((RUNTIME / 'corpus' / key / 'reference.json').read_text())
        if ref['duration_seconds'] > 300:
            parser.error('Full-context diagnostic is limited to five minutes')
        references.append(ref)
    os.environ.update(HF_HUB_OFFLINE='1', TRANSFORMERS_OFFLINE='1', HF_HUB_DISABLE_TELEMETRY='1')
    import torch
    from nemo.collections.asr.models import SortformerEncLabelModel
    torch.set_num_threads(4)
    settings = json.loads((Path.home() / '.local/share/com.judeam.sonefold/speech-runtime.json').read_text())
    checkpoint = Path(settings['diarization_model_path'])
    with checkpoint.open('rb') as file:
        digest = hashlib.file_digest(file, 'sha256').hexdigest()
    started = time.perf_counter()
    model = SortformerEncLabelModel.restore_from(str(checkpoint), map_location='cuda', strict=False)
    model.eval().to('cuda')
    model.streaming_mode = args.mode == 'streaming'
    config = model.sortformer_modules
    config.spkcache_len, config.fifo_len = 264, 40
    config.chunk_len, config.chunk_right_context, config.spkcache_update_period = 340, 40, 300
    model._check_streaming_parameters()
    args.output.mkdir(parents=True)
    atomic_json(args.output / 'configuration.json', dict(
        mode=args.mode, ids=args.ids, checkpoint_sha256=digest, loading_seconds=time.perf_counter()-started,
        script_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
        caveat='Development cases; script boundaries are not validated acoustic ground truth.'))
    with torch.inference_mode():
        for ref in references:
            torch.cuda.reset_peak_memory_stats()
            started = time.perf_counter()
            segments, probabilities = model.diarize(audio=[ref['audio_path']], batch_size=1, include_tensor_outputs=True)
            turns = [(str(v[2]), float(v[0]), float(v[1])) for v in [s.split() for s in segments[0]]]
            score = interval_diagnostics([(t['speaker'], t['start_seconds'], t['end_seconds']) for t in ref['turns']], turns)
            score.update(id=ref['id'], mode=args.mode, true_voices=len(ref['people']),
                         predicted_voices=len({s for s, _, _ in turns}), seconds=time.perf_counter()-started,
                         peak_gpu_bytes=torch.cuda.max_memory_allocated())
            atomic_json(args.output / f'{ref["id"]}-score.json', score)
            atomic_json(args.output / f'{ref["id"]}-turns.json', turns)
            torch.save(probabilities[0].detach().cpu(), args.output / f'{ref["id"]}-probabilities.pt')
            print(json.dumps(score), flush=True)


if __name__ == '__main__':
    main()
