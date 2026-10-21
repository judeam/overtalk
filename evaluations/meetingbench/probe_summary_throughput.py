"""Benchmark installed summary-model batching; not a summary quality evaluation.

Uses synthetic repeated pilot excerpts, so timings are exploratory. Keeps the
real llama-helper engine, installed quantizations, and normal context budget.
"""
from __future__ import annotations

import argparse
import json
from pathlib import Path
import queue
import subprocess
import sys
import threading
import time

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT))
from evaluations.meetingbench.gemini import atomic_json


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--batches", type=int, nargs="+", default=[1, 4])
    parser.add_argument("--models", nargs="+", choices=["4b", "27b"], default=["4b"])
    args = parser.parse_args()
    if not all(1 <= n <= 4 for n in args.batches):
        parser.error("This probe uses batches 1–4 inside the installed context limits")
    runtime = ROOT / "evaluations/meetingbench/.runtime"
    folder = Path.home() / ".local/share/com.judeam.sonefold/models/summary"
    filenames = {"4b": "Qwen3.5-4B-Q4_K_M.gguf", "27b": "Qwen3.8-27B-AP-IQ4_XS.gguf"}
    pilot = json.loads((ROOT / "evaluations/meetingbench/pilot_script.json").read_text())
    transcript = "\n".join(f"[S{i+1}] {row['speaker']}: {row['text']}" for i, row in enumerate(pilot["turns"]))
    prompts = [
        "<|im_start|>system\nYou take accurate meeting notes. Retain confirmed decisions, action owners and deadlines, "
        "rejected proposals and unresolved issues. Cite transcript source IDs. Never invent a year or an owner.<|im_end|>\n"
        f"<|im_start|>user\nReview {i+1}. Write concise English notes for this transcript.\n{transcript}\n"
        "<|im_end|>\n<|im_start|>assistant\n<think>\n\n</think>\n\n"
        for i in range(max(args.batches))
    ]
    results = {"purpose": "Synthetic pilot engine throughput, not full production pipeline or corpus quality",
               "quantizations": filenames, "max_output_tokens_per_sequence": 512, "phases": []}
    for key in args.models:
        checkpoint = folder / filenames[key]
        if not checkpoint.is_file():
            raise RuntimeError(f"Installed model not found: {checkpoint.name}")
        log_path = runtime / f"summary-probe-{key}.log"
        with log_path.open("w") as log:
            process = subprocess.Popen([str(ROOT / "target/release/llama-helper")], stdin=subprocess.PIPE,
                                       stdout=subprocess.PIPE, stderr=log, text=True, bufsize=1)
            responses = queue.Queue()

            def receive():
                for line in process.stdout:
                    try:
                        responses.put(json.loads(line))
                    except ValueError:
                        pass
                responses.put({"type": "error", "message": "Helper process exited"})

            threading.Thread(target=receive, daemon=True).start()
            try:
                for batch in args.batches:
                    body = {"type": "generate_batch", "prompts": prompts[:batch], "max_tokens": 512,
                            "context_size": 16384, "model_path": str(checkpoint), "temperature": 0,
                            "top_k": 1, "top_p": 1, "stop_on_repeated_line": True}
                    started = time.perf_counter()
                    process.stdin.write(json.dumps(body) + "\n")
                    process.stdin.flush()
                    response = responses.get(timeout=240)
                    elapsed = time.perf_counter() - started
                    if response.get("error") or response.get("type") != "batch_response":
                        raise RuntimeError(str(response)[:1000])
                    texts = response["texts"]
                    assert len(texts) == batch
                    row = {"model": key, "batch": batch, "wall_seconds": elapsed,
                           "includes_cold_model_load": batch == args.batches[0],
                           "output_characters": [len(text) for text in texts], "outputs": texts}
                    results["phases"].append(row)
                    atomic_json(runtime / "summary-throughput.json", results)
                    print(json.dumps({k: v for k, v in row.items() if k != "outputs"}), flush=True)
            finally:
                try:
                    process.stdin.write('{"type":"shutdown"}\n')
                    process.stdin.flush()
                    process.wait(timeout=15)
                except Exception:
                    process.kill()
                    process.wait()


if __name__ == "__main__":
    main()
