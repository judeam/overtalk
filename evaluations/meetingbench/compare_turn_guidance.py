"""Replay calibration audio with cached diarization and the candidate ASR worker.

Never changes baseline results, reference text, the corpus, or the app library.
This command runs real local ASR; run it under your own GPU-sharing launcher if
you use one (such as OVERTALK_GPU_LAUNCHER). Cached diarization makes
the comparison isolate the ASR/diarization handoff, not a new diarizer run.
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
RUNTIME = ROOT / "evaluations/meetingbench/.runtime"
sys.path.insert(0, str(ROOT / "speech-worker"))

from .gemini import atomic_json
from .metrics import normalize, transcript_score


def text_assignment(rows):
    total = sum(len(normalize(row["text"]).replace(" ", "")) for row in rows)
    unknown = sum(len(normalize(row["text"]).replace(" ", "")) for row in rows
                  if not (row.get("speaker") or row.get("speaker_id")))
    return {"passages": len(rows), "unassigned_passages": sum(
        not (row.get("speaker") or row.get("speaker_id")) for row in rows),
        "recognized_characters": total, "unassigned_characters": unknown,
        "unassigned_character_fraction": unknown / total if total else None}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ids", nargs="+", required=True,
                        help="Explicit calibration IDs; never select cases from their candidate scores")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.output.exists():
        parser.error("Output exists: choose a new path to preserve previous experiments")
    inputs = []
    for key in args.ids:
        if Path(key).name != key or key in {".", ".."}:
            parser.error("Invalid case ID")
        reference = json.loads((RUNTIME / "corpus" / key / "reference.json").read_text())
        baseline = json.loads((RUNTIME / "results" / key / "transcript.json").read_text())
        inputs.append((key, reference, baseline))
    settings = json.loads((Path.home() / ".local/share/com.judeam.sonefold/speech-runtime.json").read_text())
    os.environ.update(HF_HUB_OFFLINE="1", TRANSFORMERS_OFFLINE="1", HF_HUB_DISABLE_TELEMETRY="1",
                      SONEFOLD_QWEN3_ASR_1_7B_DIR=settings["asr_model_dir"],
                      SONEFOLD_QWEN3_ALIGNER_DIR=settings["aligner_model_dir"])
    import torch
    from sonefold_speech.asr import ASREngine
    from sonefold_speech.diarization import assign_speakers
    from sonefold_speech.__main__ import split_speaker_turns
    torch.set_num_threads(4)
    engine = ASREngine("cuda")
    args.output.mkdir(parents=True)
    code = ROOT / "speech-worker/sonefold_speech"
    atomic_json(args.output / "configuration.json", {
        "ids": args.ids, "model": "qwen3-asr-1.7b", "device": "cuda",
        "baseline": "immutable installed pipeline output; cached diarization reused",
        "code_sha256": {name: hashlib.sha256((code / name).read_bytes()).hexdigest()
                        for name in ("asr.py", "diarization.py", "__main__.py")},
        "limitations": ["Calibration replay, not held-out validation",
                        "Assignment coverage does not establish correct identity",
                        "ASR-only runtime excludes cached diarization and summary"]})
    try:
        for key, reference, baseline in inputs:
            turns = [dict(speaker=speaker, start=start, end=end)
                     for speaker, start, end in baseline["turns"]]
            torch.cuda.reset_peak_memory_stats()
            started = time.perf_counter()
            candidate = engine.transcribe(reference["audio_path"], speaker_turns=turns)
            candidate["segments"] = split_speaker_turns(
                assign_speakers(candidate["segments"], turns), candidate.get("language"))
            elapsed = time.perf_counter() - started
            expected = "\n".join(turn["text"] for turn in reference["turns"])
            score = {
                "id": key, "difficulty": reference["difficulty"], "languages": reference["languages"],
                "script_sha256": reference["script_sha256"],
                "baseline_transcript": transcript_score(expected, "\n".join(
                    row["text"] for row in baseline["rows"]), reference["languages"]),
                "candidate_transcript": transcript_score(expected, "\n".join(
                    row["text"] for row in candidate["segments"]), reference["languages"]),
                "baseline_assignment": text_assignment(baseline["rows"]),
                "candidate_assignment": text_assignment(candidate["segments"]),
                "candidate_asr_seconds": elapsed,
                "peak_allocated_vram_bytes": torch.cuda.max_memory_allocated(),
            }
            atomic_json(args.output / f"{key}-prediction.json", candidate)
            atomic_json(args.output / f"{key}-score.json", score)
            print(json.dumps(score), flush=True)
    finally:
        engine.unload()


if __name__ == "__main__":
    main()
