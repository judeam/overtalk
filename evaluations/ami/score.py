"""Whole-meeting word error rate for AMI hypotheses, with the S/D/I split.

Reference: every transcribed AMI word of the meeting, utterances in start-time order
(see reference.py). Hypothesis: every transcript segment in start-time order. Both
sides pass through the same Whisper English normaliser before one global alignment.
"""
from __future__ import annotations

import argparse
from concurrent.futures import ProcessPoolExecutor
import json
import os
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT))
from evaluations.ami.reference import TEST_MEETINGS  # noqa: E402
from evaluations.ami.vendor.whisper_normalizer import EnglishTextNormalizer  # noqa: E402
from evaluations.meetingbench.metrics import distance  # noqa: E402

NORMALISATION = ("Whisper EnglishTextNormalizer (openai-whisper 20250625, vendored): lowercase; "
                 "bracketed text removed; fillers hmm/mm/mhm/mmm/uh/um removed; contractions expanded; "
                 "spelled numbers to digits; British to American spelling; punctuation and symbols removed. "
                 "Applied per reference utterance and per hypothesis segment.")
DIAGONAL, DELETION, INSERTION = 0, 1, 2
# An utterance counts as overlapped when other participants speak words during
# more than this share of it (AMI word timings).
OVERLAP_THRESHOLD = 0.2
_normaliser = None


def normalise(texts) -> list[str]:
    global _normaliser
    if _normaliser is None:
        _normaliser = EnglishTextNormalizer()
    words = []
    for text in texts:
        words.extend(_normaliser(text).split())
    return words


def align(reference: list[str], hypothesis: list[str]) -> dict:
    return alignment(reference, hypothesis)[0]


def alignment(reference: list[str], hypothesis: list[str]) -> tuple[dict, list[str]]:
    """Minimum-edit alignment; ties prefer a substitution, then a deletion.

    Also returns, for every reference word, "C" (correct), "S" or "D".
    """
    vocabulary: dict[str, int] = {}
    ref = [vocabulary.setdefault(word, len(vocabulary)) for word in reference]
    hyp = [vocabulary.setdefault(word, len(vocabulary)) for word in hypothesis]
    width = len(hyp)
    previous = list(range(width + 1))
    moves = []
    for i, token in enumerate(ref, 1):
        current = [i] * (width + 1)
        move = bytearray(width + 1)
        move[0] = DELETION
        left = i
        for j in range(1, width + 1):
            diagonal = previous[j - 1] + (hyp[j - 1] != token)
            up = previous[j] + 1
            left += 1
            if diagonal <= up and diagonal <= left:
                left = diagonal
            elif up <= left:
                left = up
                move[j] = DELETION
            else:
                move[j] = INSERTION
            current[j] = left
        moves.append(move)
        previous = current
    substitutions = deletions = insertions = 0
    operations = ["D"] * len(ref)
    i, j = len(ref), width
    while i and j:
        step = moves[i - 1][j]
        if step == DIAGONAL:
            wrong = ref[i - 1] != hyp[j - 1]
            substitutions += wrong
            operations[i - 1] = "S" if wrong else "C"
            i, j = i - 1, j - 1
        elif step == DELETION:
            deletions, i = deletions + 1, i - 1
        else:
            insertions, j = insertions + 1, j - 1
    deletions += i
    insertions += j
    errors = substitutions + deletions + insertions
    if errors != previous[width]:
        raise AssertionError("Backtrace disagrees with the edit distance")
    return {"reference_words": len(ref), "hypothesis_words": width, "substitutions": substitutions,
            "deletions": deletions, "insertions": insertions, "errors": errors,
            "wer": errors / len(ref) if ref else None}, operations


def reference_texts(reference: dict, keep_fragments: bool = True) -> list[str]:
    texts = []
    for utterance in reference["utterances"]:
        words = [word for word, truncated in zip(utterance["words"], utterance["truncated"])
                 if keep_fragments or not truncated]
        texts.append(" ".join(words))
    return texts


def utterance_kind(utterance: dict, segments: list[dict]) -> str:
    """no_saved_row: outside every transcript row the app saved; else overlapped or clear.

    The app saves no row for audio the VAD dropped or for a chunk the model returned empty,
    so no_saved_row covers both cases.
    """
    middle = (utterance["start"] + utterance["end"]) / 2
    if not any(segment["start"] is not None and segment["start"] <= middle < segment["end"]
               for segment in segments):
        return "no_saved_row"
    return "overlapped" if utterance["overlap_fraction"] > OVERLAP_THRESHOLD else "clear"


def breakdown(reference: dict, segments: list[dict], operations: list[str], owners: list[int]) -> dict:
    """Reference words, substitutions and deletions by the kind of utterance they belong to."""
    kinds = [utterance_kind(utterance, segments) for utterance in reference["utterances"]]
    result = {kind: {"reference_words": 0, "substitutions": 0, "deletions": 0}
              for kind in ("clear", "overlapped", "no_saved_row")}
    for operation, owner in zip(operations, owners):
        row = result[kinds[owner]]
        row["reference_words"] += 1
        row["substitutions"] += operation == "S"
        row["deletions"] += operation == "D"
    return result


def score_meeting(reference: dict, hypothesis: dict) -> dict:
    hyp = normalise(segment["text"] for segment in hypothesis["segments"])
    ref, owners = [], []
    for index, text in enumerate(reference_texts(reference)):
        words = normalise([text])
        ref += words
        owners += [index] * len(words)
    scored, operations = alignment(ref, hyp)
    # Independent check of the total against the bit-parallel distance used elsewhere.
    if distance(ref, hyp) != scored["errors"]:
        raise AssertionError(f"{reference['meeting']}: alignment and distance disagree")
    without = align(normalise(reference_texts(reference, keep_fragments=False)), hyp)
    scored.update(meeting=reference["meeting"], audio_seconds=hypothesis["audio_seconds"],
                  wall_seconds=hypothesis["wall_seconds"],
                  real_time_factor=hypothesis["wall_seconds"] / hypothesis["audio_seconds"],
                  segments=len(hypothesis["segments"]),
                  ami_truncated_words=reference["truncated_words"],
                  wer_fragments_removed_from_reference=without["wer"],
                  by_utterance_kind=breakdown(reference, hypothesis["segments"], operations, owners),
                  load_average_1_5_15=hypothesis.get("load_average_1_5_15"))
    return scored


def pooled(rows: list[dict]) -> dict:
    total = {key: sum(row[key] for row in rows) for key in
             ("reference_words", "hypothesis_words", "substitutions", "deletions", "insertions",
              "errors", "audio_seconds", "wall_seconds")}
    words = total["reference_words"]
    kinds = {}
    for row in rows:
        for kind, counts in row["by_utterance_kind"].items():
            target = kinds.setdefault(kind, dict.fromkeys(counts, 0))
            for key, value in counts.items():
                target[key] += value
    for counts in kinds.values():
        counts["share_of_reference_words"] = counts["reference_words"] / words
        counts["deletion_rate"] = counts["deletions"] / counts["reference_words"] if counts["reference_words"] else None
        counts["substitution_rate"] = (counts["substitutions"] / counts["reference_words"]
                                       if counts["reference_words"] else None)
    total["by_utterance_kind"] = kinds
    total.update(wer=total["errors"] / words,
                 substitution_rate=total["substitutions"] / words,
                 deletion_rate=total["deletions"] / words,
                 insertion_rate=total["insertions"] / words,
                 real_time_factor=total["wall_seconds"] / total["audio_seconds"],
                 mean_meeting_wer=sum(row["wer"] for row in rows) / len(rows))
    return total


def score_condition(references: dict, directory: Path, meetings, jobs: int) -> dict:
    pairs, missing, failed = [], [], []
    for meeting in meetings:
        path = directory / f"{meeting}.json"
        if not path.is_file():
            missing.append(meeting)
            continue
        hypothesis = json.loads(path.read_text())
        if hypothesis["status"] != "completed":
            failed.append({"meeting": meeting, "error": hypothesis.get("error")})
            continue
        pairs.append((references[meeting], hypothesis))
    with ProcessPoolExecutor(max_workers=jobs) as pool:
        rows = list(pool.map(score_meeting, *zip(*pairs))) if pairs else []
    result = {"complete": not missing and not failed, "missing": missing, "failed": failed,
              "meetings": rows}
    if rows:
        result["pooled"] = pooled(rows)
    return result


def percent(value: float) -> str:
    return f"{100 * value:.2f}%"


def markdown(results: dict) -> str:
    lines = []
    for condition, data in results["conditions"].items():
        if "pooled" not in data:
            continue
        total = data["pooled"]
        lines += [f"**{condition}**: WER {percent(total['wer'])} over {total['reference_words']} reference words "
                  f"(substitutions {percent(total['substitution_rate'])}, deletions {percent(total['deletion_rate'])}, "
                  f"insertions {percent(total['insertion_rate'])}); real-time factor {total['real_time_factor']:.4f}"
                  + ("" if data["complete"] else " — INCOMPLETE: " + json.dumps(data["missing"] + data["failed"])),
                  "", "Reference words by utterance kind: " + "; ".join(
                      f"{kind} {percent(counts['share_of_reference_words'])} of words, deletions "
                      f"{percent(counts['deletion_rate'])}, substitutions {percent(counts['substitution_rate'])}"
                      for kind, counts in total["by_utterance_kind"].items() if counts["reference_words"]),
                  "", "| Meeting | Ref. words | WER | Sub. | Del. | Ins. | WER, fragments removed | RTF |",
                  "| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |"]
        for row in data["meetings"]:
            lines.append(f"| {row['meeting']} | {row['reference_words']} | {percent(row['wer'])} | {row['substitutions']} "
                         f"| {row['deletions']} | {row['insertions']} | {percent(row['wer_fragments_removed_from_reference'])} "
                         f"| {row['real_time_factor']:.4f} |")
        lines.append("")
    return "\n".join(lines)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--references", type=Path, required=True, help="Directory written by reference.py")
    parser.add_argument("--hypotheses", type=Path, required=True, help="Directory written by run_overtalk.py")
    parser.add_argument("--output", type=Path, required=True, help="results.json to write")
    parser.add_argument("--meetings", nargs="*", default=list(TEST_MEETINGS))
    parser.add_argument("--jobs", type=int, default=min(8, os.cpu_count() or 1))
    args = parser.parse_args()
    references = {meeting: json.loads((args.references / f"{meeting}.json").read_text())
                  for meeting in args.meetings}
    conditions = {}
    for directory in sorted(path for path in args.hypotheses.iterdir() if path.is_dir()):
        conditions[directory.name] = score_condition(references, directory, args.meetings, args.jobs)
    archives = {(row["archive"], row["archive_sha256"]) for row in references.values()}
    results = {
        "corpus": {"name": "AMI Meeting Corpus", "licence": "CC BY 4.0",
                   "attribution": "AMI Meeting Corpus, University of Edinburgh et al., CC BY 4.0",
                   "annotations": [{"file": name, "sha256": digest} for name, digest in sorted(archives)],
                   "partition": "AMI full-corpus ASR test set", "meetings": args.meetings},
        "scoring": {"normalisation": NORMALISATION,
                    "reference": "All <w> words except punctuation, truncated fragments included; "
                                 "AMI letter acronyms joined (T_V_ -> TV); utterances in start-time order.",
                    "hypothesis": "All transcript segments of the imported meeting in start-time order.",
                    "metric": "One global Levenshtein alignment per meeting; pooled WER = total errors / "
                              "total reference words. Speaker attribution is not scored.",
                    "utterance_kinds": "no_saved_row: the utterance midpoint lies outside every transcript row "
                                       "the app saved (the VAD dropped that audio or the model returned no text "
                                       "for the chunk); overlapped: other participants speak words during more "
                                       f"than {OVERLAP_THRESHOLD:.0%} of the utterance; clear: the rest. "
                                       "Insertions cannot be attributed to a reference utterance."},
        "run": json.loads((args.hypotheses / "run.json").read_text()),
        "conditions": conditions,
    }
    args.output.write_text(json.dumps(results, indent=1) + "\n")
    print(markdown(results))


if __name__ == "__main__":
    main()
