"""Reproduce diagnostics for the synthetic preview without modifying the app."""
import datetime
import json
from pathlib import Path
import re
import sqlite3

from evaluations.meetingbench.gemini import atomic_json
from evaluations.meetingbench.metrics import normalize, transcript_score


def interval_diagnostics(reference, hypothesis, collar=.25):
    """Exact interval sweep; approximate reference clip spans, not human labels."""
    import numpy as np
    from scipy.optimize import linear_sum_assignment
    ref_ids = sorted({s for s, start, end in reference})
    hyp_ids = sorted({s for s, start, end in hypothesis})
    from bisect import bisect_left
    from collections import Counter, defaultdict
    ref_edges = sorted({t for s, start, end in reference for t in (start, end)})
    events = defaultdict(list)
    for kind, rows in enumerate([reference, hypothesis]):
        for speaker, start, end in rows:
            events[start].append((kind, speaker, 1))
            events[end].append((kind, speaker, -1))
    points = set(events)
    points.update(max(0, t + d) for t in ref_edges for d in [-collar, collar])
    points = sorted(points)
    intersections = np.zeros((len(ref_ids), len(hyp_ids)))
    ref_index = {speaker:i for i,speaker in enumerate(ref_ids)}
    hyp_index = {speaker:i for i,speaker in enumerate(hyp_ids)}
    active = [Counter(), Counter()]
    intervals = []
    for left, right in zip(points, points[1:]):
        for kind, speaker, change in events[left]:
            active[kind][speaker] += change
            if active[kind][speaker] == 0:
                del active[kind][speaker]
        center = (left + right) / 2
        index = bisect_left(ref_edges, center)
        if any(abs(center - boundary) < collar for boundary in ref_edges[max(0,index-1):index+1]):
            continue
        r, h = set(active[0]), set(active[1])
        delta = right - left
        intervals.append((delta, r, h))
        for speaker in r:
            for other in h:
                intersections[ref_index[speaker], hyp_index[other]] += delta
    rr, hh = linear_sum_assignment(-intersections)
    mapping = {hyp_ids[h]: ref_ids[r] for r, h in zip(rr, hh)}
    missed = false_alarm = confused = total = overlap_ref = overlap_hyp = overlap_match = 0.0
    for delta, r, h in intervals:
        correct = len(r & {mapping.get(s) for s in h})
        total += delta * len(r)
        missed += delta * max(0, len(r) - len(h))
        false_alarm += delta * max(0, len(h) - len(r))
        confused += delta * (min(len(r), len(h)) - correct)
        overlap_ref += delta * (len(r) >= 2)
        overlap_hyp += delta * (len(h) >= 2)
        overlap_match += delta * (len(r) >= 2 and len(h) >= 2)
    return {
        "approximate_diarization_error_rate": (missed + false_alarm + confused) / total if total else None,
        "missed_speaker_seconds": missed, "false_alarm_speaker_seconds": false_alarm,
        "confused_speaker_seconds": confused, "reference_speaker_seconds": total,
        "overlap_detection_precision": overlap_match / overlap_hyp if overlap_hyp else None,
        "overlap_detection_recall": overlap_match / overlap_ref if overlap_ref else None,
        "optimal_anonymous_speaker_mapping": mapping, "reference_boundary_collar_seconds": collar,
        "limitation": "Diagnostic only: reference intervals are trimmed synthetic utterance clips, including internal pauses, not human-validated speech activity. Do not present as a formal benchmark DER.",
    }


def main():
    folder = Path(__file__).resolve().parent / ".runtime/very-hard-preview"
    reference = json.loads((folder / "reference.json").read_text())
    transcript = json.loads((folder / "actual-transcript.json").read_text())
    summary = json.loads((folder / "actual-summary.json").read_text())
    oracle = json.loads((folder / "oracle-summary.json").read_text())
    installed = json.loads((folder / "installed.json").read_text())
    ref = "\n".join(turn["text"] for turn in reference["turns"])
    hyp = "\n".join(row["text"] for row in transcript)
    phrase = "Not the replacement credential."
    loop = next(row for row in transcript if phrase in row["text"])
    speaker_rows = []
    db_path = Path.home() / ".local/share/com.judeam.sonefold/meeting_minutes.sqlite"
    with sqlite3.connect(f"file:{db_path}?mode=ro", uri=True) as db:
        speaker_rows = db.execute("SELECT speaker_id,start_time,end_time FROM speaker_turns WHERE meeting_id=? ORDER BY start_time", (installed["meeting_id"],)).fetchall()
    atomic_json(folder / "diarization-turns.json", {"turns": speaker_rows})
    ref_intervals = [(t["speaker"], t["start_seconds"], t["end_seconds"]) for t in reference["turns"]]
    start = datetime.datetime.fromisoformat(summary["start"].replace("Z", "+00:00"))
    end = datetime.datetime.fromisoformat(summary["end"].replace("Z", "+00:00"))
    # Explicit case review, not an automatic semantic judge or a validated scale.
    manual = [
        ("D1", "covered", "covered", "Cancelled 20:00 release and conditional 10:00 target recovered."),
        ("D2", "missing", "partial", "ASR notes omit checkout pause/queue preservation; clean notes recover pause and preserve job IDs."),
        ("D3", "covered", "covered", "Both notes retain the internal EUR 320 limit."),
        ("F1", "missing", "covered", "46 orders / 64 events / 12 unconfirmed duplicate charges absent from ASR notes, present in clean notes."),
        ("A1", "missing", "covered", "Hina's checkout/export task and 18:20 deadline are recovered only with clean input."),
        ("A2", "covered", "covered", "Daniel, three workers, 18:30 retained."),
        ("A3", "partial", "partial", "Completed rotation is inconsistently listed as an open task; clean notes also miss its final completed status."),
        ("A4", "covered", "covered", "Marco and 09:30 tomorrow retained, but clean notes cite S35, which does not state that deadline."),
        ("A5", "partial", "covered", "ASR notes preserve Leah's client update but assign her finance follow-up to a Finance Representative."),
        ("O1", "partial", "partial", "Refund approval remains open, but the specific pending payment job is omitted."),
    ]
    assert {row[0] for row in manual} == {fact["id"] for fact in reference["facts"]}
    result = {
        "meeting_id": installed["meeting_id"], "reference_seconds": reference["duration_seconds"],
        "models": {"asr": "qwen3-asr-1.7b", "diarization": "nemotron-3", "summary": "qwen3.5:4b"},
        "transcript": {
            **transcript_score(ref, hyp, reference["languages"]),
            "hypothesis_words": len(normalize(hyp).split()),
            "wer_caveat": "Raw comparison against the intended script in turn-start order. Overlap permits alternative ordering; Hindi transliteration and possible TTS deviations affect this number. It is not a calibrated real-world accuracy percentage.",
            "hallucinated_repeated_phrase": phrase, "repetitions": loop["text"].count(phrase),
            "loop_start": loop["audio_start_time"], "loop_end": loop["audio_end_time"],
            "repeated_phrase_word_fraction": loop["text"].count(phrase) * len(normalize(phrase).split()) / len(normalize(hyp).split()),
        },
        "speakers": {
            "true_voices": len(reference["people"]), "detected_voices": len(installed["speaker_result"]),
            "transcript_blocks": len(transcript), "unlabelled_blocks": sum(not row.get("speaker_id") for row in transcript),
            "unlabelled_timeline_seconds": sum(row["duration"] for row in transcript if not row.get("speaker_id")),
            "voices_with_any_transcript_block": len({row["speaker_id"] for row in transcript if row.get("speaker_id")}),
            "diarization_turns": len(speaker_rows),
            **interval_diagnostics(ref_intervals, speaker_rows),
        },
        "summary": {
            "generation_seconds": (end - start).total_seconds(),
            "source_blocks_cited": sorted({int(n) for n in re.findall(r"\[S(\d+)\]", summary["data"]["markdown"])}),
            "manual_reference_item_review": [{"id": key, "from_asr": actual, "from_clean_script": clean, "finding": note} for key, actual, clean, note in manual],
            "review_caveat": "One manually reviewed synthetic case with composite reference items; covered/partial/missing are not a general numeric quality scale.",
            "clean_input_control_seconds": oracle["control"]["wall_seconds"],
        },
        "user_library": "Existing transcript and notes were read, not corrected or replaced during this review.",
    }
    atomic_json(folder / "performance-review.json", result)
    print(json.dumps(result, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
