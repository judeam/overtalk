"""Bounded-memory Qwen3-ASR 1.7B evaluation using the app's audio windowing.

Run with the existing Qwen environment, optionally under your own GPU-sharing
launcher (such as the one named by OVERTALK_GPU_LAUNCHER). The JSONL manifest
contains id, audio_path, reference text, and languages. Predictions never replace
references. Segment timestamps are chunk boundaries, not forced word alignment.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import sys
import time

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT))
sys.path.insert(0, str(ROOT / "speech-worker"))
from evaluations.meetingbench.gemini import atomic_json
from evaluations.meetingbench.metrics import transcript_score


def bounded_inference(predict, items, batch_size, clear_memory=lambda: None):
    """Keep order, split memory failures, and retain other failures per input."""
    try:
        results = predict(items)
        if len(results) != len(items):
            raise RuntimeError("ASR returned the wrong number of predictions")
        return [(result, None) for result in results]
    except Exception as error:
        if len(items) == 1:
            return [(None, str(error))]
        memory_failure = "out of memory" in str(error).lower()
    # Leave the exception handler before retrying so the failed operation's
    # traceback no longer retains its temporary GPU tensors.
    if memory_failure:
        clear_memory()
    middle = min(batch_size, max(1, len(items) // 2))
    return (bounded_inference(predict, items[:middle], middle, clear_memory)
            + bounded_inference(predict, items[middle:], middle, clear_memory))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--batch-size", type=int, default=32)
    args = parser.parse_args()
    if not 1 <= args.batch_size <= 64:
        parser.error("batch size must be 1–64")
    if args.output.exists():
        parser.error("Output already exists; choose a new path to preserve earlier results")
    from sonefold_speech.asr import ASREngine, SAMPLE_RATE, audio_windows
    import torch
    torch.set_num_threads(4)

    settings = json.loads((Path.home() / ".local/share/com.judeam.sonefold/speech-runtime.json").read_text())
    os.environ["SONEFOLD_QWEN3_ASR_1_7B_DIR"] = settings["asr_model_dir"]
    entries = [json.loads(line) for line in args.manifest.read_text().splitlines() if line.strip()]
    ids = [row["id"] for row in entries]
    if len(set(ids)) != len(ids):
        raise ValueError("Duplicate meeting IDs in manifest")
    for row in entries:
        if not isinstance(row["text"], str) or not isinstance(row["languages"], list):
            raise ValueError("Every meeting needs independently authored reference text and languages")
    engine = ASREngine("cuda")
    started = time.perf_counter()
    engine.load("qwen3-asr-1.7b")
    loading_seconds = time.perf_counter() - started
    model = engine._model
    model.max_inference_batch_size = args.batch_size
    states = [{"entry": entry, "segments": [], "errors": [], "audio_seconds": 0.0} for entry in entries]
    pending = []
    inference_seconds = 0.0
    batch_count = 0
    torch.cuda.reset_peak_memory_stats()

    def flush():
        nonlocal inference_seconds, batch_count
        if not pending:
            return
        torch.cuda.synchronize()
        began = time.perf_counter()
        inputs = [(samples, SAMPLE_RATE) for _, samples, _, _ in pending]
        results = bounded_inference(
            lambda batch: model.transcribe(audio=batch, language=None, return_time_stamps=False),
            inputs, args.batch_size, torch.cuda.empty_cache,
        )
        torch.cuda.synchronize()
        inference_seconds += time.perf_counter() - began
        batch_count += 1
        for (state, _, offset, duration), (prediction, error) in zip(pending, results):
            if error:
                state["errors"].append({"start": offset, "end": offset + duration, "error": error})
            else:
                state["segments"].append({"start": offset, "end": offset + duration,
                                          "text": prediction.text.strip(), "language": prediction.language,
                                          "speaker": None, "timing": "chunk"})
        pending.clear()
        print(json.dumps({"batches": batch_count, "inference_seconds": inference_seconds}), flush=True)

    try:
        for state in states:
            try:
                for samples, offset, duration in audio_windows(Path(state["entry"]["audio_path"])):
                    state["audio_seconds"] = offset + duration
                    if not samples.any():
                        continue
                    pending.append((state, samples, offset, duration))
                    if len(pending) == args.batch_size:
                        flush()
            except Exception as error:
                state["errors"].append({"error": str(error), "stage": "decode"})
        flush()
        peak_bytes = torch.cuda.max_memory_allocated()
    finally:
        engine.unload()
    elapsed = time.perf_counter() - started
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with args.output.open("x", encoding="utf-8") as output:
        for state in states:
            entry = state["entry"]
            partial_text = "\n".join(segment["text"] for segment in state["segments"] if segment["text"])
            prediction = partial_text if not state["errors"] else None
            row = {"id": entry["id"], "model": "qwen3-asr-1.7b", "prediction": prediction,
                   "status": "failed" if state["errors"] else "completed", "errors": state["errors"],
                   "segments": state["segments"], "audio_seconds": state["audio_seconds"],
                   "word_alignment": False, "speaker_attribution": False,
                   "metrics": transcript_score(entry["text"], prediction, entry["languages"])}
            if state["errors"]:
                row["partial_prediction_for_diagnosis_only"] = partial_text
            output.write(json.dumps(row, ensure_ascii=False, allow_nan=False) + "\n")
    audio_seconds = sum(state["audio_seconds"] for state in states)
    atomic_json(args.output.with_suffix(".provenance.json"), {
        "manifest_sha256": hashlib.sha256(args.manifest.read_bytes()).hexdigest(),
        "model": "qwen3-asr-1.7b", "batch_size": args.batch_size, "meetings": len(states),
        "loading_seconds": loading_seconds, "wall_seconds": elapsed, "inference_seconds": inference_seconds,
        "audio_seconds": audio_seconds, "real_time_factor_including_loading_and_decode": elapsed / audio_seconds if audio_seconds else None,
        "peak_cuda_allocated_bytes": peak_bytes,
        "limitations": "Automatic language detection; production audio_windows; chunk timestamps. No speaker or word-alignment score is claimed.",
    })
    print(json.dumps({"meetings": len(states), "wall_seconds": elapsed, "output": str(args.output)}))


if __name__ == "__main__":
    main()
