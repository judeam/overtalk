"""Measure real Gemini throughput; these synthetic probe clips are NOT eval data.

Reuses the corpus cost ledger and existing synthetic voice. Never switches models,
creates cloud resources, or changes project quotas. Cached clips are excluded from
new throughput measurements. Failed requests remain reserved in the shared ledger.
"""
from __future__ import annotations

import argparse
from concurrent.futures import ThreadPoolExecutor, as_completed
import hashlib
import json
import os
from pathlib import Path
import statistics
import sys
import time

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT))
from evaluations.meetingbench.gemini import Gemini, atomic_json, config_from_env_file

TOPICS = [
    ("warehouse move", "loading bay", "delivery drivers", "access map"),
    ("school timetable", "science laboratory", "teaching assistants", "room allocation"),
    ("community festival", "outdoor stage", "volunteers", "weather contingency"),
    ("software release", "test environment", "support engineers", "rollback plan"),
    ("kitchen refurbishment", "cold storage", "restaurant staff", "temporary menu"),
    ("research workshop", "recording equipment", "visiting researchers", "consent forms"),
    ("construction handover", "fire doors", "site inspectors", "inspection checklist"),
    ("sports tournament", "changing rooms", "coaches", "registration list"),
]


def probe_text(index: int, words: int) -> str:
    topic, place, people, document = TOPICS[index % len(TOPICS)]
    paragraphs = [
        f"For the {topic}, I want to check the practical details before we confirm anything. "
        f"The {place} is available, but the {people} have not seen the updated {document}. "
        f"Please distinguish a provisional arrangement from a decision we have actually approved.",
        f"There were {12 + index} outstanding questions in the last review. We have resolved the first three, "
        "and two more need a named owner. The deadline has not moved just because the document was edited. "
        "I would rather send a short, accurate update than promise a date that nobody has checked.",
        f"On the budget, the current estimate is {840 + index * 11} euros, including delivery. "
        "That replaces the older estimate in the spreadsheet. Please keep the earlier number in the change history, "
        "but make sure the approval request shows the current amount and explains the difference.",
        "Before we close, let us read the actions back. The operations lead will confirm access on Thursday. "
        "The coordinator will update everyone on Friday morning. If either check fails, we will pause the announcement "
        "and discuss an alternative; we are not authorizing an automatic exception.",
    ]
    return " ".join(" ".join(paragraphs).split()[:words])


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--concurrency", type=int, nargs="+", default=[1, 8, 32])
    parser.add_argument("--words", type=int, default=160)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--request-timeout", type=int, default=120)
    parser.add_argument("--reservation-headroom-usd", type=float, default=15)
    parser.add_argument("--requests-per-second", type=float, default=0,
                        help="Pace admissions; zero measures an unpaced burst")
    parser.add_argument("--runtime", type=Path, default=ROOT / "evaluations/meetingbench/.runtime")
    parser.add_argument("--config", type=Path, default=os.environ.get("OVERTALK_TTS_ENV"),
                        help="File with the Gemini API key (KEY=value lines); defaults to $OVERTALK_TTS_ENV")
    args = parser.parse_args()
    if not args.config:
        parser.error("set --config or OVERTALK_TTS_ENV to the file holding the Gemini key")
    if not all(1 <= n <= 64 for n in args.concurrency) or not 30 <= args.words <= 200:
        parser.error("concurrency must be 1–64 and words 30–200")
    if not 1 <= args.reservation_headroom_usd <= 25:
        parser.error("Probe reservation headroom must be 1–25 USD within the shared budget")
    if not 0 <= args.requests_per_second <= 4:
        parser.error("Admission rate must be between zero (burst) and four requests/second")
    client = Gemini(config_from_env_file(args.config), args.runtime, request_timeout=args.request_timeout)
    initial = client.ledger.report()["accounted_usd"]
    # Keep this exploration bounded even if requests fail and reservations stay.
    client.ledger.limit = min(client.ledger.limit, int((initial + args.reservation_headroom_usd) * 1_000_000))
    voices = sorted((args.runtime / "voices").glob("*.json"))
    if not voices:
        raise RuntimeError("Run the custom-voice pilot first")
    voice = json.loads(voices[0].read_text())["id"]
    output = {"not_corpus_data": True, "model": "gemini-3.8-flash-tts", "phases": []}
    serial = 0
    for concurrency in args.concurrency:
        jobs = []
        for _ in range(concurrency):
            index = serial
            serial += 1
            text = probe_text(index, args.words)
            # Unique, legitimate delivery instructions prevent cached requests
            # from appearing to be new synthesis. No spoken measurement IDs.
            style = f"Conversational meeting update, steady natural pace. Tone: {['calm','thoughtful','concerned','firm'][index % 4]}. Keep a natural breath after sentence {index + 1}."
            body = {"contents": [{"role": "user", "parts": [{"text": text, "speechMetadata": {"style": style}}]}],
                    "generationConfig": {"responseModalities": ["AUDIO"], "speechConfig": {"voiceConfig": {"voice": voice}}}}
            key = hashlib.sha256(json.dumps(body, sort_keys=True, ensure_ascii=False).encode()).hexdigest()
            if (args.runtime / "clips" / (key + ".wav")).exists():
                raise RuntimeError("Probe would use cached audio; change --words for a fresh measurement")
            jobs.append((index, text, style))
        start = time.perf_counter()
        rows = []

        def run(job):
            index, text, style = job
            began = time.perf_counter()
            try:
                path, duration = client.speak(text, voice, style)
                return {"index": index, "audio_seconds": duration, "wall_seconds": time.perf_counter() - began,
                        "path": str(path), "reference_text": text, "status": "ok"}
            except Exception as error:
                return {"index": index, "wall_seconds": time.perf_counter() - began,
                        "status": "error", "error": str(error)}

        with ThreadPoolExecutor(max_workers=concurrency) as pool:
            futures = []
            for index, job in enumerate(jobs):
                if args.requests_per_second:
                    due = start + index / args.requests_per_second
                    time.sleep(max(0, due - time.perf_counter()))
                futures.append(pool.submit(run, job))
            for future in as_completed(futures):
                row = future.result()
                rows.append(row)
                print(json.dumps({k: v for k, v in row.items() if k not in {"path", "reference_text"}}), flush=True)
        elapsed = time.perf_counter() - start
        success = [row for row in rows if row["status"] == "ok"]
        audio = sum(row["audio_seconds"] for row in success)
        phase = {"concurrency": concurrency, "requests_per_second": args.requests_per_second,
                 "wall_seconds": elapsed, "audio_seconds": audio,
                 "aggregate_audio_seconds_per_second": audio / elapsed,
                 "completed": len(success), "failed": len(rows) - len(success),
                 "median_request_seconds": statistics.median(row["wall_seconds"] for row in rows),
                 "clips": sorted(rows, key=lambda row: row["index"])}
        output["phases"].append(phase)
        output["cost"] = client.ledger.report()
        atomic_json(args.output or args.runtime / "tts-throughput.json", output)
        print(json.dumps({k: v for k, v in phase.items() if k != "clips"}), flush=True)
        # A capacity error is evidence, not a reason to escalate load or spend.
        if phase["failed"]:
            break


if __name__ == "__main__":
    main()
