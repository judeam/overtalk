"""Local held-out ASR evaluation. Outputs private predictions and simple WER/CER."""

import argparse
import hashlib
import importlib.metadata
import platform
import contextlib
import json
from pathlib import Path
import sys
import time
import unicodedata

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from sonefold_speech.asr import ASREngine, SUPPORTED_MODELS


def normalized(text):
    text = unicodedata.normalize("NFKC", text).casefold()
    return " ".join("".join(" " if unicodedata.category(char).startswith("P") else char
                            for char in text).split())


def edit_distance(reference, hypothesis):
    """Exact bit-parallel Levenshtein distance for long meetings, no dependencies."""
    if len(reference) > len(hypothesis):
        reference, hypothesis = hypothesis, reference
    length = len(reference)
    if not length:
        return len(hypothesis)
    matches = {}
    for index, token in enumerate(reference):
        matches[token] = matches.get(token, 0) | (1 << index)
    mask, highest = (1 << length) - 1, 1 << (length - 1)
    positive, negative, distance = mask, 0, length
    for token in hypothesis:
        equal = matches.get(token, 0)
        vertical = equal | negative
        horizontal = (((equal & positive) + positive) ^ positive) | equal
        positive_horizontal = negative | ~(horizontal | positive)
        negative_horizontal = positive & horizontal
        distance += bool(positive_horizontal & highest) - bool(negative_horizontal & highest)
        positive_horizontal = (positive_horizontal << 1) | 1
        negative_horizontal <<= 1
        positive = (negative_horizontal | ~(vertical | positive_horizontal)) & mask
        negative = positive_horizontal & vertical
    return distance


def error_rates(reference, hypothesis):
    ref, hyp = normalized(reference), normalized(hypothesis)
    ref_words, hyp_words = ref.split(), hyp.split()
    ref_chars, hyp_chars = ref.replace(" ", ""), hyp.replace(" ", "")
    words, chars = edit_distance(ref_words, hyp_words), edit_distance(ref_chars, hyp_chars)
    return {"word_errors": words, "reference_words": len(ref_words),
            "wer": words / len(ref_words) if ref_words else None,
            "character_errors": chars, "reference_characters": len(ref_chars),
            "cer": chars / len(ref_chars) if ref_chars else None,
            "silence_hallucination_words": len(hyp_words) if not ref_words else 0}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", type=Path, required=True,
                        help="JSONL {audio_path,text,language}; use held-out consenting local recordings")
    parser.add_argument("--model", choices=SUPPORTED_MODELS, required=True)
    parser.add_argument("--device", default="auto")
    parser.add_argument("--output", type=Path, required=True, help="Private JSONL result file; do not commit")
    parser.add_argument("--continue-on-error", action="store_true", help="Record failures and finish the pilot; never hide missing predictions")
    args = parser.parse_args()
    engine = ASREngine(args.device)
    count = 0
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with args.manifest.open(encoding="utf-8") as manifest, args.output.open("x", encoding="utf-8") as output:
        for line in manifest:
            if not line.strip():
                continue
            item = json.loads(line)
            started = time.perf_counter()
            try:
                with contextlib.redirect_stdout(sys.stderr):
                    result = engine.transcribe(item["audio_path"], args.model, item.get("language"))
            except Exception as error:
                if not args.continue_on_error: raise
                row = {"model": args.model, "audio_path": item["audio_path"], "session_id": item.get("session_id"),
                       "status":"failed", "error":str(error), "prediction":None, "elapsed_seconds":time.perf_counter()-started}
                output.write(json.dumps(row, ensure_ascii=False) + "\n"); output.flush(); count += 1
                print(f"Recording {count} failed validation", file=sys.stderr)
                continue
            elapsed = time.perf_counter() - started
            duration = result["audio_seconds"]
            row = {"status":"completed", "model": args.model, "audio_path": item["audio_path"], "language": result["language"],
                   "session_id":item.get("session_id"), "segments":result.get("segments",[]),
                   "elapsed_seconds": elapsed, "audio_seconds": duration,
                   "real_time_factor": elapsed / duration if duration else None,
                   "includes_cold_load": count == 0, "normalization": "nfkc-casefold-punctuation-space-v1",
                   "prediction": result["text"], "warnings": result["warnings"],
                   **error_rates(item["text"], result["text"])}
            output.write(json.dumps(row, ensure_ascii=False, allow_nan=False) + "\n")
            output.flush()
            count += 1
            print(f"Evaluated recording {count}", file=sys.stderr)
    engine.unload()
    versions = {}
    for package in ("torch", "transformers", "qwen-asr", "nemo_toolkit"):
        try: versions[package] = importlib.metadata.version(package)
        except importlib.metadata.PackageNotFoundError: pass
    args.output.with_suffix(".provenance.json").write_text(json.dumps({"manifest_sha256":hashlib.sha256(args.manifest.read_bytes()).hexdigest(),"model":args.model,"device":args.device,"python":platform.python_version(),"packages":versions,"rows":count,"normalization":"nfkc-casefold-punctuation-space-v1","limitations":"Utterance pilots do not measure continuous diarization, speaker attribution or long-meeting reliability."},indent=2)+"\n")
    print(f"Saved {count} local evaluations to {args.output}")


if __name__ == "__main__":
    main()
