"""Local speaker diarization and conservative timestamp reconciliation.

Model libraries are imported lazily; timestamp assignment needs no ML runtime.
No model is fetched during analysis. Install/download models explicitly first.
"""

from __future__ import annotations

from bisect import bisect_left
from collections import defaultdict
import math
import gc
import os
from pathlib import Path
from typing import Any


def _bounds(num_speakers, min_speakers, max_speakers):
    for value in (num_speakers, min_speakers, max_speakers):
        if value is not None and (isinstance(value, bool) or not isinstance(value, int) or value < 1):
            raise ValueError("Speaker counts must be positive integers")
    if min_speakers is not None and max_speakers is not None and min_speakers > max_speakers:
        raise ValueError("min_speakers cannot exceed max_speakers")
    if num_speakers is not None and (
        min_speakers is not None and num_speakers < min_speakers
        or max_speakers is not None and num_speakers > max_speakers
    ):
        raise ValueError("num_speakers is outside the requested bounds")


def _normalize_turns(turns: list[dict]) -> list[dict]:
    result = []
    for turn in turns:
        start, end = float(turn["start"]), float(turn["end"])
        speaker = turn.get("speaker") or turn.get("speaker_id")
        if not all(math.isfinite(value) for value in (start, end)) or start < 0 or end <= start:
            raise ValueError("Diarization returned an invalid timestamp interval")
        if not isinstance(speaker, str) or not speaker.strip():
            raise ValueError("Diarization returned an empty speaker ID")
        result.append({"start": start, "end": end, "speaker": speaker, "confidence": None})
    return sorted(result, key=lambda item: (item["start"], item["end"], item["speaker"]))


class _TurnIndex:
    def __init__(self, turns):
        self.turns = _normalize_turns(turns)
        self.starts = [turn["start"] for turn in self.turns]
        self.max_ends = []
        maximum = 0.0
        for turn in self.turns:
            maximum = max(maximum, turn["end"])
            self.max_ends.append(maximum)

    def intersecting(self, start, end):
        index = bisect_left(self.starts, end) - 1
        matches = []
        while index >= 0 and self.max_ends[index] > start:
            turn = self.turns[index]
            if turn["end"] > start:
                matches.append(turn)
            index -= 1
        return matches


def _union_duration(intervals):
    total, right = 0.0, -math.inf
    for start, end in sorted(intervals):
        total += max(0.0, end - max(right, start))
        right = max(right, end)
    return total


def speaker_change_boundaries(turns: list[dict]) -> list[float]:
    """Find safe cuts between consecutive, non-overlapping speaker turns.

    Keep same-speaker pauses together. Simultaneous speakers cannot be isolated
    by a time cut, so their audio must remain available as ambiguous speech.
    """
    ordered = _normalize_turns(turns)
    boundaries = []
    frontier = None
    for turn in ordered:
        if frontier is not None and frontier["end"] <= turn["start"]:
            if frontier["speaker"] != turn["speaker"]:
                boundaries.append((frontier["end"] + turn["start"]) / 2)
        if frontier is None or turn["end"] >= frontier["end"]:
            frontier = turn
    return boundaries


def _assignment(segment, regular_index, exclusive_index):
    start, end = float(segment["start"]), float(segment["end"])
    if not math.isfinite(start) or not math.isfinite(end) or start < 0 or end <= start:
        raise ValueError("Transcript timestamps must be finite, nonnegative, and increasing")
    regular = regular_index.intersecting(start, end)
    overlap = any(
        a["speaker"] != b["speaker"]
        and max(start, a["start"], b["start"]) < min(end, a["end"], b["end"])
        for index, a in enumerate(regular) for b in regular[index + 1:]
    )
    selected = exclusive_index.intersecting(start, end) if exclusive_index else regular
    intervals = defaultdict(list)
    for turn in selected:
        intervals[turn["speaker"]].append((max(start, turn["start"]), min(end, turn["end"])))
    scores = sorted(((_union_duration(spans) / (end - start), speaker)
                     for speaker, spans in intervals.items()), reverse=True)
    winner = scores[0] if scores else None
    runner_up = scores[1][0] if len(scores) > 1 else 0.0
    # Coverage is attribution support, not a calibrated identity probability.
    known = winner is not None and winner[0] >= 0.55 and winner[0] - runner_up >= 0.15
    return {
        "speaker": winner[1] if known else None,
        "speaker_confidence": min(winner[0], 1.0) if known else None,
        "overlap": overlap,
        "speaker_ambiguous": bool(scores) and not known,
    }


def assign_speakers(segments: list[dict], turns: list[dict], exclusive_turns: list[dict] | None = None) -> list[dict]:
    """Return enriched copies without dropping text, timestamps, words or IDs.

    Use genuine pyannote exclusive turns for attribution when available, while
    retaining overlap detected in regular turns. Silence stays unassigned.
    """
    regular = _TurnIndex(turns)
    exclusive = _TurnIndex(exclusive_turns) if exclusive_turns else None
    result = []
    for segment in segments:
        enriched = dict(segment)
        original_source = segment.get("speaker")
        if original_source in {"mic", "system", "microphone", "system_audio"}:
            enriched["source_speaker"] = original_source
        enriched.update(_assignment(segment, regular, exclusive))
        if "words" in segment:
            words = []
            for word in segment["words"]:
                enriched_word = dict(word)
                if word.get("start") is not None and word.get("end") is not None and word["end"] > word["start"]:
                    enriched_word.update(_assignment(word, regular, exclusive))
                else:
                    enriched_word.update({"speaker": None, "speaker_confidence": None, "overlap": False})
                words.append(enriched_word)
            enriched["words"] = words
        result.append(enriched)
    return result


def diarize(audio_path: str, model_dir: str | None = None, device: str = "cpu", model: str = "nemotron-3",
            num_speakers: int | None = None, min_speakers: int | None = None,
            max_speakers: int | None = None) -> dict[str, Any]:
    """Run a predownloaded local checkpoint; never fall back to a cloud API."""
    _bounds(num_speakers, min_speakers, max_speakers)
    if model not in {"nemotron-3", "community-1"}:
        raise ValueError("diarization_model must be nemotron-3 or community-1")
    audio = Path(audio_path)
    if not audio.is_file():
        raise ValueError("Audio file does not exist")
    model_dir = model_dir or os.environ.get("SONEFOLD_DIARIZATION_MODEL_DIR")
    if not model_dir or not Path(model_dir).exists():
        raise RuntimeError("Download a local diarization checkpoint first and set SONEFOLD_DIARIZATION_MODEL_DIR. "
                           "Community-1 requires accepting its Hugging Face model terms before downloading.")
    os.environ["PYANNOTE_METRICS_ENABLED"] = "0"
    os.environ["HF_HUB_OFFLINE"] = "1"
    os.environ["HF_HUB_DISABLE_TELEMETRY"] = "1"
    os.environ["TRANSFORMERS_OFFLINE"] = "1"
    try:
        if model == "community-1":
            turns, exclusive = _community(audio, Path(model_dir), device, num_speakers, min_speakers, max_speakers)
        else:
            if any(value is not None for value in (num_speakers, min_speakers, max_speakers)):
                raise ValueError("Nemotron-3 estimates up to eight speakers; count constraints require Community-1")
            turns, exclusive = _nemotron(audio, Path(model_dir), device), []
    finally:
        # ASR and diarization run sequentially; return unused GPU memory to ASR.
        unload_diarization()
    turns, exclusive = _normalize_turns(turns), _normalize_turns(exclusive)
    order = list(dict.fromkeys(turn["speaker"] for turn in turns))
    return {"turns": turns, "exclusive_turns": exclusive, "model": model,
            "speakers": [{"id": speaker, "name": f"Speaker {index + 1}"} for index, speaker in enumerate(order)]}


def _community(audio, model_path, device, num_speakers, min_speakers, max_speakers):
    try:
        import torch
        from pyannote.audio import Pipeline
    except ImportError as error:
        raise RuntimeError("Install speech-worker diarization dependencies (pyannote.audio>=4) first") from error
    pipeline = Pipeline.from_pretrained(str(model_path))
    pipeline.to(torch.device(device))
    options = {key: value for key, value in {"num_speakers": num_speakers,
               "min_speakers": min_speakers, "max_speakers": max_speakers}.items() if value is not None}
    with torch.inference_mode():
        output = pipeline(str(audio), **options)
    def extract(annotation):
        return [{"start": turn.start, "end": turn.end, "speaker": speaker}
                for turn, _, speaker in annotation.itertracks(yield_label=True)]
    return extract(output.speaker_diarization), extract(output.exclusive_speaker_diarization)


def _nemotron(audio, model_path, device):
    from .nemo_runtime import prepare
    prepare()
    try:
        import torch
        from nemo.collections.asr.models import SortformerEncLabelModel
    except ImportError as error:
        raise RuntimeError("Install NeMo Speech 3 with ASR extras in Python 3.12+ for Nemotron-3 diarization") from error
    if model_path.is_dir():
        checkpoints = sorted(model_path.glob("*.nemo"))
        if len(checkpoints) != 1:
            raise RuntimeError("The Nemotron model directory must contain exactly one .nemo checkpoint")
        model_path = checkpoints[0]
    if model_path.suffix != ".nemo":
        raise RuntimeError("Nemotron-3 requires a predownloaded .nemo checkpoint")
    from .model_security import validate_nemo_checkpoint
    validate_nemo_checkpoint(model_path)
    engine = SortformerEncLabelModel.restore_from(str(model_path), map_location=device, strict=False)
    engine.eval()
    engine.to(torch.device(device))
    config = engine.sortformer_modules
    config.spkcache_len, config.fifo_len = 264, 40
    config.chunk_len, config.chunk_right_context, config.spkcache_update_period = 340, 40, 300
    engine._check_streaming_parameters()
    with torch.inference_mode():
        predicted = engine.diarize(audio=[str(audio)], batch_size=1)[0]
    result = []
    for item in predicted:
        fields = item.split() if isinstance(item, str) else item
        if len(fields) != 3:
            raise RuntimeError("Unexpected NeMo speaker interval response")
        speaker = str(fields[2])
        result.append({"start": float(fields[0]), "end": float(fields[1]), "speaker": speaker})
    # Canonical IDs stay in arrival order, independent of NeMo string spelling.
    arrival = sorted(result, key=lambda turn: (turn["start"], turn["end"]))
    mapping = {speaker: f"SPEAKER_{index:02d}" for index, speaker in
               enumerate(dict.fromkeys(turn["speaker"] for turn in arrival))}
    return [{**turn, "speaker": mapping[turn["speaker"]]} for turn in result]


def diarize_audio(audio_path: str, **kwargs) -> list[dict]:
    """Compatibility entry point for callers needing only regular turns."""
    return diarize(audio_path, **kwargs)["turns"]


def unload_diarization() -> None:
    """Release discarded model tensors without importing torch just for cleanup."""
    import sys
    gc.collect()
    torch = sys.modules.get("torch")
    if torch is not None and torch.cuda.is_available():
        torch.cuda.empty_cache()
