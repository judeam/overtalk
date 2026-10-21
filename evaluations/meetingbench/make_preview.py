"""Synthesize and mix the authored six-person stress-test demonstration."""
from __future__ import annotations

from concurrent.futures import ThreadPoolExecutor, as_completed
import argparse
import json
import os
from pathlib import Path
import sys
import time

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT))
from evaluations.meetingbench.gemini import Gemini, atomic_json, config_from_env_file


def overlap_stats(intervals):
    events = sorted([(start, 1) for start, end in intervals] + [(end, -1) for start, end in intervals])
    active, peak, occupied, overlap, previous = 0, 0, 0.0, 0.0, 0.0
    overlap_run = longest_overlap = 0.0
    for at, change in events:
        if active:
            occupied += at - previous
        if active >= 2:
            overlap += at - previous
            overlap_run += at - previous
            longest_overlap = max(longest_overlap, overlap_run)
        else:
            overlap_run = 0.0
        active += change
        peak = max(peak, active)
        previous = at
    return {"overlap_fraction_of_speech_time": overlap / occupied if occupied else 0,
            "overlap_seconds": overlap, "longest_overlap_seconds": longest_overlap,
            "speech_timeline_seconds": occupied, "peak_speakers": peak}


def schedule(turns, clips, strength=.5, maximum=3, seed=20261015):
    """Mostly turn-taking; late, brief interruptions only in angry arguments."""
    import random
    rng = random.Random(seed)
    intervals, starts, own_end = [], [], {}
    cursor = 1.0
    for i, (row, clip) in enumerate(zip(turns, clips)):
        duration = len(clip) / 24000
        start = cursor if i == 0 else cursor + rng.uniform(.12, .48)
        previous = turns[i - 1] if i else {}
        heated = (i > 0 and row.get("group") and row.get("group") == previous.get("group")
                  and row.get("emotion") == previous.get("emotion") == "angry")
        if heated and row["speaker"] != previous["speaker"]:
            left, right = intervals[-1]
            prior_duration = right - left
            # Enter late in the current utterance. Short interjections can
            # overlap briefly; long statements are never stacked from onset.
            limit = min(1.15, prior_duration * (.75 if prior_duration < 1.8 else .22), duration * .3)
            overlap = min(limit, rng.uniform(.4, .95) * (.7 + .4 * strength))
            start = max(left + .15, right - overlap)
        start = max(start, own_end.get(row["speaker"], 0) + .08)
        end = start + duration
        active = [(left, right) for left, right in intervals if right > start and left < end]
        if overlap_stats(active + [(start, end)])["peak_speakers"] > maximum:
            start, end = cursor + .12, cursor + .12 + duration
        starts.append(start)
        intervals.append((start, end))
        own_end[row["speaker"]] = end
        cursor = max(cursor, end)
    return starts, intervals


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--script", type=Path, default=ROOT / "evaluations/meetingbench/very_hard_example.json")
    parser.add_argument("--output", default="very-hard-preview")
    parser.add_argument("--config", type=Path, default=os.environ.get("OVERTALK_TTS_ENV"),
                        help="File with the Gemini API key (KEY=value lines); defaults to $OVERTALK_TTS_ENV")
    args = parser.parse_args()
    if not args.config:
        parser.error("set --config or OVERTALK_TTS_ENV to the file holding the Gemini key")
    if Path(args.output).name != args.output:
        raise ValueError("Output must be a single case directory name")
    runtime = ROOT / "evaluations/meetingbench/.runtime"
    output = runtime / args.output
    output.mkdir(parents=True, exist_ok=True)
    script = json.loads(args.script.read_text())
    client = Gemini(config_from_env_file(args.config), runtime, request_timeout=240)
    cached_voice = json.loads(next((runtime / "voices").glob("*.json")).read_text())["id"]
    people = {person["id"]: person for person in script["people"]}
    generated = [None] * len(script["turns"])
    began = time.perf_counter()

    def synthesize(index, turn):
        person = people[turn["speaker"]]
        voice = cached_voice if person["voice"] == "custom-existing" else person["voice"]
        delivery = "Heated interruption, emotionally frustrated, quick natural speech." if turn.get("group") else "Focused natural conversation."
        if index >= len(script["turns"]) - 6:
            delivery = "Clear, steady readback of decisions."
        path, seconds = client.speak(turn["text"], voice, person["style"] + " " + delivery)
        return index, path, seconds

    with ThreadPoolExecutor(max_workers=16) as pool:
        futures = []
        for index, turn in enumerate(script["turns"]):
            due = began + index / 2
            time.sleep(max(0, due - time.perf_counter()))
            futures.append(pool.submit(synthesize, index, turn))
        for future in as_completed(futures):
            index, path, seconds = future.result()
            generated[index] = {"path": str(path), "seconds": seconds}
            print(json.dumps({"turn": script["turns"][index]["id"], "seconds": seconds}), flush=True)
            atomic_json(output / "generation.json", {"clips": generated, "cost": client.ledger.report()})

    render(script, generated, output)
    print(json.dumps({"audio": str(output / "meeting.wav"), "cost": client.ledger.report()}), flush=True)


def render(script, generated, output):
    """Mix already generated clips; useful to pipeline many meetings globally."""
    import numpy as np
    import soundfile as sf
    from scipy.signal import butter, sosfilt

    profile = script.get("audio_profile", {})
    people = {person["id"]: person for person in script["people"]}
    clips = []
    for row in generated:
        samples, rate = sf.read(row["path"], dtype="float32")
        assert rate == 24000 and samples.ndim == 1
        active = np.flatnonzero(np.abs(samples) > .008)
        if not len(active):
            raise RuntimeError("TTS produced a silent turn")
        left, right = max(0, active[0] - 1200), min(len(samples), active[-1] + 1800)
        samples = samples[left:right].copy()
        rms = float(np.sqrt(np.mean(samples * samples)))
        samples *= min(4.0, .10 / max(rms, .0001))
        clips.append(samples)
    maximum = min(len(people), script.get("max_concurrent_speakers", 4))
    starts, intervals = schedule(script["turns"], clips, profile.get("interruption_intensity", .5), maximum, script.get("seed", 20261015))
    total = max(end for start, end in intervals) + 1
    clean = np.zeros(int(np.ceil(total * 24000)), dtype="float32")
    mixture = np.zeros_like(clean)
    rng = np.random.default_rng(script.get("seed", 20261015))
    loss_spans = []
    for index, (turn, samples, start) in enumerate(zip(script["turns"], clips, starts)):
        offset = round(start * 24000)
        clean[offset:offset + len(samples)] += samples
        modified = samples.copy()
        speaker = turn["speaker"]
        if speaker in profile.get("remote_speakers", ["hina", "leah"]):
            modified = sosfilt(butter(4, [350, 3300], btype="bandpass", fs=24000, output="sos"), modified).astype("float32")
            modified = np.round(np.clip(modified * 1.7, -.72, .72) * 90) / 90
            block = 480
            # Twenty-millisecond lost packets, with occasional longer connection gaps.
            for at in range(0, len(modified) - block, block):
                if rng.random() < profile.get("packet_loss", .075):
                    modified[at:at + block] *= .03
                    loss_spans.append({"turn": turn["id"], "start": start + at / 24000, "end": start + (at + block) / 24000})
            if index in {8, 31, 40} and len(modified) > 24000:
                at = len(modified) // 3
                modified[at:at + 4800] *= .02
                loss_spans.append({"turn": turn["id"], "start": start + at / 24000, "end": start + (at + 4800) / 24000})
        if speaker in profile.get("echo_speakers", ["marco"]):
            delay = 1800
            modified[delay:] += .12 * modified[:-delay]
        if speaker == "nikhil" and turn.get("group"):
            modified *= .65
        mixture[offset:offset + len(modified)] += modified
        turn.update(start_seconds=start, end_seconds=start + len(samples) / 24000,
                    clip=generated[index]["path"], speaker_name=people[speaker]["name"])
    noise = rng.normal(size=len(mixture)).astype("float32")
    noise = sosfilt(butter(2, [100, 6500], btype="bandpass", fs=24000, output="sos"), noise).astype("float32")
    speech_rms = float(np.sqrt(np.mean(mixture * mixture)))
    snr = profile.get("snr_db", 11)
    noise *= speech_rms / (10 ** (snr / 20)) / float(np.sqrt(np.mean(noise * noise)))
    mixture += noise
    peak = float(np.max(np.abs(mixture)))
    mixture *= min(1, .94 / max(peak, .001))
    clean *= min(1, .94 / max(float(np.max(np.abs(clean))), .001))
    sf.write(output / "meeting.wav", mixture, 24000, subtype="PCM_16")
    sf.write(output / "clean-reference.wav", clean, 24000, subtype="PCM_16")
    script.update(audio_path=str(output / "meeting.wav"), duration_seconds=total,
                  construction={**overlap_stats(intervals), "overlap_policy": "brief_staggered_heated_arguments_only", "requested_snr_db": snr,
                                "remote_speakers": profile.get("remote_speakers", ["hina", "leah"]), "packet_losses": loss_spans,
                                "voice_policy": "Consistent synthetic voice per participant; no cloned real person.",
                                "speaker_timing": "Trimmed TTS clip intervals, not word-aligned timestamps."})
    atomic_json(output / "reference.json", script)
    return script


if __name__ == "__main__":
    main()
