"""Measure cold loading and warm ASR latency without saving transcript content."""

import argparse
import contextlib
import json
from pathlib import Path
import sys
import time

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from sonefold_speech.asr import ASREngine, QWEN_MODELS, local_audio_path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--audio", type=Path, required=True)
    parser.add_argument("--model", choices=QWEN_MODELS, default="qwen3-asr-1.7b")
    parser.add_argument("--device", default="auto")
    parser.add_argument("--language", help="Optional language hint; defaults to automatic detection")
    parser.add_argument("--runs", type=int, choices=range(2, 11), default=3)
    parser.add_argument("--word-timestamps", action=argparse.BooleanOptionalAction, default=True)
    args = parser.parse_args()
    audio = local_audio_path(str(args.audio))
    engine, stages = ASREngine(args.device), []
    instrumented = set()

    def instrument(obj, method, label):
        key = (id(obj), method)
        if key in instrumented:
            return
        instrumented.add(key)
        original = getattr(obj, method)

        def measured(*values, **options):
            started = time.perf_counter()
            result = original(*values, **options)
            stages.append({"stage": label, "seconds": time.perf_counter() - started})
            return result

        setattr(obj, method, measured)

    original_aligner = engine._load_aligner

    def load_aligner():
        result = original_aligner()
        if result is not None:
            instrument(result, "align", "alignment")
        return result

    engine._load_aligner = load_aligner
    instrument(engine, "_load_aligner", "aligner_load_or_cache")
    instrument(engine, "_runtime", "torch_and_device")
    instrument(engine, "_load", "model_load_or_cache")
    try:
        started = time.perf_counter()
        with contextlib.redirect_stdout(sys.stderr):
            engine.load(args.model)
        torch = engine._torch

        def synchronize():
            if engine.device.startswith("cuda"):
                torch.cuda.synchronize(engine.device)

        synchronize()
        load_seconds = time.perf_counter() - started
        instrument(engine._model, "transcribe", "recognition")
        report = {"model": args.model, "device": engine.device, "dtype": str(engine.dtype),
                  "torch": torch.__version__, "load_seconds": load_seconds,
                  "language_hint": args.language,
                  "attention": getattr(engine._model.model.config, "_attn_implementation", None),
                  "word_timestamps": args.word_timestamps, "load_stages": list(stages),
                  "runs": [], "limitations": "Repeated clips measure startup and adapter latency, not meeting accuracy or end-to-end file processing."}
        if engine.device.startswith("cuda"):
            report["gpu"] = torch.cuda.get_device_name(engine.device)
            report["sm_count"] = torch.cuda.get_device_properties(engine.device).multi_processor_count
        for index in range(args.runs):
            stages.clear()
            synchronize()
            started = time.perf_counter()
            with contextlib.redirect_stdout(sys.stderr):
                result = engine.transcribe(str(audio), args.model, args.language,
                                           word_timestamps=args.word_timestamps)
            synchronize()
            elapsed = time.perf_counter() - started
            report["runs"].append({"index": index, "first_inference": index == 0,
                                   "seconds": elapsed, "audio_seconds": result["audio_seconds"],
                                   "real_time_factor": elapsed / result["audio_seconds"] if result["audio_seconds"] else None,
                                   "segments": len(result["segments"]), "aligner_loaded": engine._aligner is not None,
                                   "stages": list(stages)})
        print(json.dumps(report, indent=2, allow_nan=False))
    finally:
        engine.unload()


if __name__ == "__main__":
    main()
