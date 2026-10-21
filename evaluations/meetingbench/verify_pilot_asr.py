"""Read-only model use on the three generated synthetic pilot clips."""
import json
import os
from pathlib import Path
import resource
import sys
import time

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "speech-worker"))
sys.path.insert(0, str(ROOT / "speech-worker/tools"))
from sonefold_speech.asr import ASREngine
from benchmark_asr import error_rates

runtime = ROOT / "evaluations/meetingbench/.runtime"
configuration = json.loads((Path.home() / ".local/share/com.judeam.sonefold/speech-runtime.json").read_text())
os.environ["SONEFOLD_QWEN3_ASR_1_7B_DIR"] = configuration["asr_model_dir"]
script = json.loads((ROOT / "evaluations/meetingbench/pilot_script.json").read_text())
references = {turn["id"]: turn["text"] for turn in script["turns"]}
clips = json.loads((runtime / "speech-smoke.json").read_text())["clips"]
engine = ASREngine("cuda")
results = []
try:
    for index, clip in enumerate(clips):
        started = time.perf_counter()
        result = engine.transcribe(str(ROOT / clip["audio"]), "qwen3-asr-1.7b", None, word_timestamps=False)
        elapsed = time.perf_counter() - started
        row = {"turn": clip["turn"], "reference": references[clip["turn"]],
               "prediction": result["text"], "language": result["language"],
               "seconds": elapsed, "includes_cold_load": index == 0,
               "real_time_factor": elapsed / clip["seconds"],
               **error_rates(references[clip["turn"]], result["text"])}
        results.append(row)
        print(json.dumps({key: row[key] for key in ("turn", "language", "seconds", "wer", "cer")}), flush=True)
finally:
    engine.unload()
(runtime / "pilot-asr.json").write_text(json.dumps({"results": results,
    "peak_rss_kib": resource.getrusage(resource.RUSAGE_SELF).ru_maxrss,
    "limitation": "Synthetic short-clip smoke test; not the 200-meeting benchmark."}, ensure_ascii=False, indent=2) + "\n")
