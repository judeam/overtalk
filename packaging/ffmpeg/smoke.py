#!/usr/bin/env python3
"""Run a built FFmpeg through the commands Overtalk issues, on synthetic media.

    python packaging/ffmpeg/smoke.py <path to ffmpeg(.exe)>

Each case mirrors an invocation in frontend/src-tauri/src or crates/capture-helper
(the argument lists are copied, with -protocol_whitelist file,pipe as the app
passes it): recording checkpoints (f32le pipe to AAC in mp4 with +faststart),
joining checkpoints (concat demuxer, -c copy), clips (libopus in WebM with the
privacy filter), the privacy export, import conversion and duration probing for
every supported extension, the screen recorder's encoding and muxing recorded
audio into its video. MP3 has no encoder in an LGPL build without LAME, so it
is checked in the demuxer and decoder lists. Exits non-zero on the first failure.
"""
import array
import math
from pathlib import Path
import re
import subprocess
import sys
import tempfile

RATE = 48000
SECONDS = 2


def sine(seconds=SECONDS, rate=RATE):
    """Mono f32le samples of a 440 Hz tone at amplitude 0.5, as bytes."""
    samples = array.array("f", (0.5 * math.sin(2 * math.pi * 440 * n / rate) for n in range(int(seconds * rate))))
    if sys.byteorder != "little":
        samples.byteswap()
    return samples.tobytes()


class Smoke:
    def __init__(self, ffmpeg, folder):
        self.ffmpeg = str(ffmpeg)
        self.folder = Path(folder)
        self.count = 0

    def run(self, *args, data=None):
        """Run ffmpeg quietly with the app's protocol whitelist; return the process."""
        command = [self.ffmpeg, "-nostdin", "-v", "error", "-y",
                   "-protocol_whitelist", "file,pipe", *map(str, args)]
        done = subprocess.run(command, input=data, capture_output=True, timeout=120)
        if done.returncode != 0:
            raise SystemExit(f"FAILED ({done.returncode}): {' '.join(command)}\n{done.stderr.decode(errors='replace')}")
        return done

    def decode(self, path, rate=16000):
        """The file as mono f32le samples at `rate`, the way audio/decoder.rs reads it."""
        done = self.run("-i", path, "-vn", "-ac", "1", "-ar", rate, "-f", "f32le", "pipe:1")
        samples = array.array("f")
        samples.frombytes(done.stdout[: len(done.stdout) // 4 * 4])
        if sys.byteorder != "little":
            samples.byteswap()
        return samples

    def probe(self, path):
        """The duration import.rs reads from `-i … -t 0 -f null -` (stderr, not an error)."""
        done = subprocess.run([self.ffmpeg, "-nostdin", "-hide_banner", "-protocol_whitelist", "file,pipe",
                               "-i", str(path), "-t", "0", "-f", "null", "-"], capture_output=True, timeout=60)
        match = re.search(r"Duration: (\d+):(\d+):([\d.]+)", done.stderr.decode(errors="replace"))
        if done.returncode != 0 or not match:
            raise SystemExit(f"FAILED duration probe of {path}\n{done.stderr.decode(errors='replace')}")
        hours, minutes, seconds = match.groups()
        return int(hours) * 3600 + int(minutes) * 60 + float(seconds)

    def ok(self, name):
        self.count += 1
        print(f"ok  {name}", flush=True)


def rms(samples, start, end, rate=16000):
    part = samples[int(start * rate):int(end * rate)]
    return math.sqrt(sum(s * s for s in part) / len(part))


def expect(condition, message):
    if not condition:
        raise SystemExit(f"FAILED: {message}")


def main():
    if len(sys.argv) != 2:
        raise SystemExit(__doc__)
    ffmpeg = Path(sys.argv[1]).resolve(strict=True)
    with tempfile.TemporaryDirectory(prefix="overtalk-ffmpeg-smoke-") as temporary:
        smoke = Smoke(ffmpeg, temporary)
        folder = smoke.folder

        # Build configuration: LGPL, no network, the pieces Overtalk names.
        text = subprocess.run([str(ffmpeg), "-hide_banner", "-version"], capture_output=True, text=True, check=True).stdout
        expect(not re.search(r"--enable-(gpl|nonfree|version3)", text), "the configuration enables GPL or non-free parts")
        expect("--enable-libopus" in text, "libopus is not enabled")
        lists = {}
        for flag in ("-encoders", "-decoders", "-muxers", "-demuxers", "-filters", "-protocols"):
            lists[flag] = subprocess.run([str(ffmpeg), "-hide_banner", flag], capture_output=True, text=True, check=True).stdout
        for flag, names in {"-encoders": ["aac", "libopus", "pcm_s16le", "flac", "wmav2", "mpeg4"],
                            "-decoders": ["aac", "mp3float", "flac", "vorbis", "opus", "wmav2", "pcm_s16le", "h264"],
                            "-muxers": ["mp4", "ipod", "webm", "matroska", "ogg", "adts", "wav", "flac", "asf", "null"],
                            "-demuxers": ["mov", "mp3", "matroska", "ogg", "flac", "aac", "wav", "asf", "concat", "lavfi"],
                            "-filters": ["volume", "asetpts", "aresample", "scale"]}.items():
            for name in names:
                expect(re.search(rf"\s{re.escape(name)}[\s,]", lists[flag]), f"{name} is missing from {flag}")
        expect(not re.search(r"\b(http|https|tcp|udp|tls|rtmp)\b", lists["-protocols"]), "a network protocol is built in")
        smoke.ok("licence, libopus and the codec, format, filter and protocol lists")

        # audio/encode.rs and audio/incremental_saver.rs: f32le from a pipe to AAC-LC in mp4.
        recording = folder / "recording.mp4"
        smoke.run("-threads", 2, "-f", "f32le", "-ar", RATE, "-ac", 1, "-i", "pipe:0", "-c:a", "aac", "-b:a", "192k",
                  "-profile:a", "aac_low", "-movflags", "+faststart", "-f", "mp4", recording, data=sine())
        head = recording.read_bytes()
        expect(head[4:8] == b"ftyp", "the recording is not an mp4")
        expect(0 < head.find(b"moov") < head.find(b"mdat"), "+faststart did not move moov before mdat")
        expect(abs(smoke.probe(recording) - SECONDS) < 0.2, "the recording has the wrong duration")
        expect(rms(smoke.decode(recording), 0.5, 1.5) > 0.2, "the recording is silent")
        smoke.ok("f32le pipe to AAC-LC mp4 with +faststart, duration probe, decode")

        # audio/incremental_saver.rs: join checkpoints with the concat demuxer, no re-encoding.
        parts = []
        for index in range(2):
            part = folder / f"part{index}.mp4"
            smoke.run("-f", "f32le", "-ar", RATE, "-ac", 1, "-i", "pipe:0", "-c:a", "aac", "-f", "mp4", part, data=sine(1))
            parts.append(part)
        listing = folder / "parts.txt"
        listing.write_text("".join(f"file '{part.as_posix()}'\n" for part in parts))
        joined = folder / "joined.mp4"
        smoke.run("-f", "concat", "-safe", 0, "-i", listing, "-c", "copy", joined)
        expect(abs(smoke.probe(joined) - 2) < 0.2, "the joined checkpoints have the wrong duration")
        smoke.ok("concat demuxer, -c copy")

        # Import: a recording in every extension the app accepts (constants.rs), converted to PCM
        # (decoder.rs) and probed for duration (import.rs). mp3 cannot be synthesized here.
        wav = folder / "source.wav"
        smoke.run("-f", "f32le", "-ar", RATE, "-ac", 1, "-i", "pipe:0", "-c:a", "pcm_s16le", wav, data=sine())
        formats = {"wav": ["-c:a", "pcm_s16le", "-f", "wav"], "flac": ["-c:a", "flac", "-f", "flac"],
                   "ogg": ["-c:a", "libopus", "-f", "ogg"], "aac": ["-c:a", "aac", "-f", "adts"],
                   "m4a": ["-c:a", "aac", "-f", "ipod"], "mp4": ["-c:a", "aac", "-f", "mp4"],
                   "mkv": ["-c:a", "flac", "-f", "matroska"], "webm": ["-c:a", "libopus", "-f", "webm"],
                   "wma": ["-c:a", "wmav2", "-f", "asf"]}
        for extension, options in formats.items():
            path = folder / f"import.{extension}"
            smoke.run("-i", wav, "-vn", *options, path)
            expect(abs(smoke.probe(path) - SECONDS) < 0.3, f"{extension}: wrong duration")
            expect(rms(smoke.decode(path), 0.5, 1.5) > 0.2, f"{extension}: decodes to silence")
            converted = folder / f"converted-{extension}.wav"
            smoke.run("-i", path, "-vn", "-acodec", "pcm_s16le", converted)
            expect(converted.stat().st_size > RATE, f"{extension}: conversion to WAV is empty")
        vorbis = folder / "import-vorbis.ogg"
        smoke.run("-i", wav, "-vn", "-ac", 2, "-c:a", "vorbis", "-strict", "-2", "-f", "ogg", vorbis)
        expect(rms(smoke.decode(vorbis), 0.5, 1.5) > 0.2, "ogg vorbis decodes to silence")
        smoke.ok(f"import {', '.join([*formats, 'ogg (vorbis)'])}: probe, decode, convert to WAV (mp3 by list only)")

        # extensions/privacy.rs: silence spans of a recording into a WAV.
        redacted = folder / "redacted.wav"
        smoke.run("-i", wav, "-vn", "-af", "volume=0:enable='between(t,0.400,0.900)'", "-c:a", "pcm_s16le", "-f", "wav", redacted)
        samples = smoke.decode(redacted)
        expect(rms(samples, 0.5, 0.8) < 0.005, "the privacy filter left the span audible")
        expect(rms(samples, 1.1, 1.8) > 0.2, "the privacy filter silenced too much")
        smoke.ok("privacy filter, volume=0:enable='between(…)' to WAV")

        # extensions/share.rs: a clip as mono Opus in WebM, with the span silenced.
        clip = folder / "clip.webm"
        smoke.run("-ss", "0.500", "-t", "1.500", "-i", wav, "-map", "0:a:0", "-vn", "-ac", 1, "-af",
                  "asetpts=PTS-STARTPTS,volume=0:enable='between(t,0.500,1.000)'", "-c:a", "libopus", "-b:a", "24000",
                  "-application", "voip", "-f", "webm", clip)
        expect(clip.read_bytes().startswith(bytes([0x1A, 0x45, 0xDF, 0xA3])), "the clip is not WebM")
        expect(clip.stat().st_size < 16 * 1024, "the clip is larger than expected")
        samples = smoke.decode(clip)
        expect(abs(len(samples) / 16000 - 1.5) < 0.1, "the clip has the wrong length")
        expect(rms(samples, 0.6, 0.9) < 0.01 and rms(samples, 1.1, 1.4) > 0.2, "the clip's silenced span is wrong")
        smoke.ok("libopus voip clip in WebM with asetpts and the privacy filter")

        # crates/capture-helper: raw BGRA frames from a pipe, scaled and encoded with FFmpeg's
        # own MPEG-4 encoder (libx264 is GPL) into fragmented mp4.
        video = folder / "screen.mp4"
        smoke.run("-f", "rawvideo", "-pixel_format", "bgra", "-video_size", "64x64", "-framerate", 15,
                  "-i", "pipe:0", "-an", "-vf", "scale='min(1280,iw)':-2:flags=fast_bilinear",
                  "-c:v", "mpeg4", "-q:v", 5, "-threads", 2, "-pix_fmt", "yuv420p",
                  "-movflags", "+frag_keyframe+empty_moov+default_base_moof", "-f", "mp4", video,
                  data=bytes(64 * 64 * 4) * 30)
        smoke.ok("screen recorder: BGRA frames from a pipe to MPEG-4 in fragmented mp4")

        # extensions/video.rs: recorded audio muxed into the screen recording, video copied.
        muxed = folder / "screen-audio.mp4"
        smoke.run("-i", video, "-ss", "0.000", "-i", wav, "-map", "0:v", "-map", "1:a", "-c:v", "copy", "-c:a", "aac",
                  "-shortest", muxed)
        shown = subprocess.run([str(ffmpeg), "-hide_banner", "-i", str(muxed)], capture_output=True, text=True).stderr
        expect("Video: mpeg4" in shown and "Audio: aac" in shown, "the muxed recording lacks its video or audio")
        smoke.ok("video copy and AAC audio mux")
        smoke.count += 1
    print(f"FFmpeg smoke test passed ({smoke.count} groups)")


if __name__ == "__main__":
    main()
