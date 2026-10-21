"""Measure local Qwen batching, separately from corpus accuracy evaluation.

Repeat probe inputs to fill the batch, label that repetition, and retain every
prediction. Timings include inference but exclude loading and preprocessing,
which are measured separately. Uses installed weights without downloading.
"""
from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import sys
import time

if os.environ.get('OVERTALK_EVAL_TRACE_STALLS') == '1':
    import faulthandler
    faulthandler.dump_traceback_later(60, repeat=True)

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT))
sys.path.insert(0, str(ROOT / 'speech-worker'))
from evaluations.meetingbench.gemini import atomic_json


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--batches", type=int, nargs="+", default=[1, 4, 16, 32])
    parser.add_argument("--backend", choices=["transformers", "vllm"], default="transformers")
    parser.add_argument("--max-gpu-fraction", type=float, default=.65)
    parser.add_argument("--eager", action="store_true", help="Disable vLLM CUDA graphs for a controlled comparison")
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    if not all(1 <= n <= 64 for n in args.batches):
        parser.error("batch sizes must be 1–64")
    import numpy as np
    import soundfile as sf
    import torch
    from scipy.signal import resample_poly
    if args.backend == 'transformers':
        from sonefold_speech.vendor.qwen_asr import Qwen3ASRModel
    else:
        # The optional vLLM experiment has its own separately managed runtime.
        from qwen_asr import Qwen3ASRModel

    runtime = ROOT / "evaluations/meetingbench/.runtime"
    configuration = json.loads((Path.home() / ".local/share/com.judeam.sonefold/speech-runtime.json").read_text())
    checkpoint = configuration["asr_model_dir"]
    if not Path(checkpoint).is_dir():
        raise RuntimeError("Installed Qwen checkpoint is missing")
    rows = json.loads((runtime / "speech-smoke.json").read_text())["clips"]
    throughput = runtime / "tts-throughput.json"
    if throughput.exists():
        measured = json.loads(throughput.read_text())
        rows = [clip for phase in measured["phases"] for clip in phase["clips"] if clip["status"] == "ok"] or rows
    inputs = []
    decode_started = time.perf_counter()
    for row in rows[:8]:
        path = Path(row.get("path", row.get("audio")))
        if not path.is_absolute():
            path = ROOT / path
        samples, rate = sf.read(path, dtype="float32", always_2d=True)
        samples = samples.mean(axis=1)
        if rate != 16000:
            from math import gcd
            divisor = gcd(rate, 16000)
            samples = resample_poly(samples, 16000 // divisor, rate // divisor)
        # Use the same 30-second window limit as the application's ASR path.
        samples = np.ascontiguousarray(samples[:30 * 16000])
        inputs.append((samples, 16000))
    preprocessing_seconds = time.perf_counter() - decode_started
    started = time.perf_counter()
    torch.set_num_threads(4)
    if args.backend == "vllm":
        from .vllm_setup import local_engine_options
        model = Qwen3ASRModel.LLM(gpu_memory_utilization=args.max_gpu_fraction,
                                 max_inference_batch_size=max(args.batches), max_new_tokens=1024,
                                 max_model_len=2048, max_num_seqs=max(args.batches),
                                 enforce_eager=args.eager, disable_log_stats=True,**local_engine_options(checkpoint,eager=args.eager))
    else:
        model = Qwen3ASRModel.from_pretrained(checkpoint, dtype=torch.bfloat16, device_map="cuda",
                                            local_files_only=True, max_inference_batch_size=max(args.batches),
                                            max_new_tokens=1024, attn_implementation="sdpa")
    loading_seconds = time.perf_counter() - started
    model.transcribe(audio=inputs[0], language=None, return_time_stamps=False)
    results = {"backend": args.backend, "model": "qwen3-asr-1.7b", "eager": args.eager, "batch_fill_repeats_inputs": True,
               "caveat": "Repeated pilot inputs may benefit from backend caches; this is not unique-corpus or end-to-end throughput.",
               "unique_probe_inputs": len(inputs), "preprocessing_seconds": preprocessing_seconds,
               "loading_seconds": loading_seconds, "phases": []}
    for count in args.batches:
        batch = [inputs[index % len(inputs)] for index in range(count)]
        torch.cuda.reset_peak_memory_stats()
        torch.cuda.synchronize()
        began = time.perf_counter()
        predictions = model.transcribe(audio=batch, language=None, return_time_stamps=False)
        torch.cuda.synchronize()
        elapsed = time.perf_counter() - began
        audio_seconds = sum(len(samples) / rate for samples, rate in batch)
        phase = {"batch": count, "audio_seconds": audio_seconds, "wall_seconds": elapsed,
                 "aggregate_audio_seconds_per_second": audio_seconds / elapsed,
                 "peak_cuda_allocated_bytes": torch.cuda.max_memory_allocated() if args.backend=='transformers' else None,
                 "predictions": [{"text": p.text, "language": p.language} for p in predictions]}
        results["phases"].append(phase)
        atomic_json(args.output or runtime / f"asr-throughput-{args.backend}.json", results)
        print(json.dumps({k: v for k, v in phase.items() if k != "predictions"}), flush=True)
    if args.backend=='vllm':
        model.model.llm_engine.engine_core.shutdown()


if __name__ == "__main__":
    main()
