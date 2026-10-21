"""Check real native messaging framing and origin limits in a disposable Linux profile."""
import argparse
import json
import os
from pathlib import Path
import platform
import random
import struct
import subprocess
import tempfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--report", type=Path)
    parser.add_argument("--strace", action="store_true", help="Inject failed writes into a disposable child")
    args = parser.parse_args()
    if platform.system() != "Linux":
        raise SystemExit("This profile-isolation check requires Linux XDG directories")
    checks = {}
    with tempfile.TemporaryDirectory(prefix="overtalk-bridge-check-") as temporary:
        env = dict(os.environ, XDG_DATA_HOME=temporary)
        payload = json.dumps({"url": "https://meet.google.com/abc-defg-hij",
                              "title": "Synthetic meeting — नमस्ते", "active": True}).encode()
        result = subprocess.run([str(args.binary.resolve())], input=struct.pack("<I", len(payload)) + payload,
                                env=env, capture_output=True, timeout=10, check=True)
        size, = struct.unpack("<I", result.stdout[:4])
        assert len(result.stdout) == size + 4
        assert json.loads(result.stdout[4:]) == {"ok": True}
        stored = Path(temporary) / "com.judeam.sonefold/browser-meeting.json"
        saved = json.loads(stored.read_text())
        assert saved["title"] == "Synthetic meeting — नमस्ते" and saved["active"]
        before = stored.read_bytes()
        foreign = json.dumps({"url": "https://meet.google.com.attacker.invalid/"}).encode()
        rejected = subprocess.run([str(args.binary.resolve())], input=struct.pack("<I", len(foreign)) + foreign,
                                  env=env, capture_output=True, timeout=10)
        assert rejected.returncode != 0 and not rejected.stdout
        assert stored.read_bytes() == before
        oversized = subprocess.run([str(args.binary.resolve())], input=struct.pack("<I", 8193),
                                   env=env, capture_output=True, timeout=10)
        assert oversized.returncode != 0 and not oversized.stdout
        checks["valid_and_origin_boundaries"] = "passed"
        invalid = [b"\x01", b"\x01\x00", b"\x01\x00\x00",
                   struct.pack("<I", 0), struct.pack("<I", 2**32 - 1)]
        for value in [b"{", b"[]", b"null", b"true", b"123", b"\xff"]:
            invalid.append(struct.pack("<I", len(value)) + value)
        # Seeded truncations and over-declared lengths exercise the real reader.
        # This is a finite protocol corpus, not coverage-guided fuzzing.
        randomizer = random.Random(20261018)
        for _ in range(256):
            keep = randomizer.randrange(len(payload))
            invalid.append(struct.pack("<I", len(payload) + randomizer.randrange(16)) + payload[:keep])
        for index, packet in enumerate(invalid):
            rejected = subprocess.run([str(args.binary.resolve())], input=packet,
                                      env=env, capture_output=True, timeout=5)
            assert rejected.returncode != 0 and not rejected.stdout, f"Invalid frame {index} acknowledged or not rejected"
            assert stored.read_bytes() == before, f"Invalid frame {index} changed the saved record"
        checks["rejected_corpus_cases"] = len(invalid)

        if args.strace:
            traces = args.report.parent if args.report else Path(temporary)
            traces.mkdir(parents=True, exist_ok=True)
            for syscall, error in [("fsync", "EIO"), ("fsync", "ENOSPC"),
                                   ("renameat", "EIO"), ("write", "ENOSPC")]:
                trace = traces / f"bridge-{syscall}-{error}.trace"
                failed = subprocess.run([
                    "strace", "--kill-on-exit", "-o", str(trace),
                    "-e", "trace=fsync,rename,renameat,renameat2,write",
                    "-e", f"inject={syscall}:error={error}:when=1", str(args.binary.resolve())],
                    input=struct.pack("<I", len(payload)) + payload,
                    env=env, capture_output=True, timeout=10)
                assert "(INJECTED)" in trace.read_text(), "Fault injection did not reach the target"
                assert failed.returncode != 0 and not failed.stdout
                assert stored.read_bytes() == before, "Failed publication changed the previous record"
                assert sorted(p.name for p in stored.parent.iterdir()) == [stored.name], "Failed write left a temporary file"
                checks[f"{syscall}_{error}_preserves_record"] = "passed"
    if args.report:
        args.report.parent.mkdir(parents=True, exist_ok=True)
        args.report.write_text(json.dumps(checks, indent=2) + "\n")
    print(json.dumps(checks, indent=2))


if __name__ == "__main__":
    main()
