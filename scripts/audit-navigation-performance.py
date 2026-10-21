#!/usr/bin/env python3
"""Measure citation navigation and audio resume through a disposable real service.

No user library, provider call, credentials or microphone access. The generated
recording is silent; the host output device is required for native playback.
"""
from service_client import Service
import argparse
import json
import statistics
import sqlite3
import tempfile
import time
import wave
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    report = {}
    with tempfile.TemporaryDirectory(prefix="overtalk-navigation-") as temp:
        root = Path(temp)
        recording = root / "recording"
        recording.mkdir()
        with wave.open(str(recording / "audio.wav"), "wb") as audio:
            audio.setparams((1, 2, 16000, 0, "NONE", "NONE"))
            audio.writeframes(b"\0\0" * 16000 * 30)
        service = Service(args.binary.resolve(), root)
        try:
            def usage():
                status = Path(f"/proc/{service.process.pid}/status")
                if not status.exists():
                    return None
                values = dict(line.split(":", 1) for line in status.read_text().splitlines() if ":" in line)
                return dict(rss_mib=int(values["VmRSS"].split()[0]) / 1024,
                            threads=int(values["Threads"].strip()))

            report["service_initial"] = usage()
            db_path = root / "data/com.judeam.sonefold/meeting_minutes.sqlite"
            with sqlite3.connect(db_path) as db:
                db.execute("INSERT INTO meetings(id,title,created_at,updated_at,folder_path) "
                           "VALUES('navigation','Synthetic navigation audit','2026-10-18',"
                           "'2026-10-18',?)", (str(recording),))
                db.executemany("INSERT INTO transcripts(id,meeting_id,transcript,timestamp,"
                               "audio_start_time,audio_end_time) VALUES(?,'navigation',"
                               "'Synthetic passage','00:00',?,?)",
                               [(f"line-{i:06}", None if i < 5 else i // 3,
                                 i // 3 + 1) for i in range(30000)])
                ordered = [row[0] for row in db.execute("SELECT id FROM transcripts "
                           "WHERE meeting_id='navigation' ORDER BY audio_start_time,id")]
            for position in [0, 99, 100, 15000, 29999]:
                durations = []
                for _ in range(9):
                    start = time.perf_counter()
                    offset = service.call("workspace_transcript_offset",
                                          meetingId="navigation", segmentId=ordered[position])
                    durations.append((time.perf_counter() - start) * 1000)
                    assert offset == position // 100 * 100, (position, offset)
                report[f"citation_{position}"] = {"median_ms": statistics.median(durations),
                                                   "max_ms": max(durations)}
            report["service_after_navigation"] = usage()
            wave_times = []
            for _ in range(5):
                # Only files under this disposable benchmark profile.
                for cache in (root / "cache").rglob("waveforms/*"):
                    if cache.is_file():
                        cache.unlink()
                start = time.perf_counter()
                peaks = service.call("meeting_waveform", meetingId="navigation")
                wave_times.append((time.perf_counter() - start) * 1000)
                assert peaks and not any(peaks), "Silent audio must have zero-valued peaks"
            report["decode_30s_waveform"] = {"median_ms": statistics.median(wave_times),
                                            "max_ms": max(wave_times)}
            initial = time.perf_counter()
            service.call("native_audio", action="load", meetingId="navigation", value=0)
            report["initial_audio_load_ms"] = (time.perf_counter() - initial) * 1000
            durations = []
            for _ in range(9):
                time.sleep(.06)
                paused = service.call("native_audio", action="pause")
                start = time.perf_counter()
                resumed = service.call("native_audio", action="play", value=paused["position"])
                durations.append((time.perf_counter() - start) * 1000)
                assert resumed["playing"], resumed
                assert abs(resumed["position"] - paused["position"]) < .2
            report["resume"] = {"median_ms": statistics.median(durations),
                                "max_ms": max(durations)}
            report["service_during_playback"] = usage()
            paused = service.call("native_audio", action="pause")
            jumped = service.call("native_audio", action="play", value=15)
            assert jumped["playing"] and 14.9 <= jumped["position"] <= 15.2, jumped
            service.call("native_audio", action="stop")
            report["service_after_stop"] = usage()
        finally:
            service.close()
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
