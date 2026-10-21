"""Record a package-manager-installed FFmpeg for the build; never download one.

Only run after verifying/installing the platform package through its trusted
package manager. This records provenance for the build, not an upstream signature.
Release packaging must include any dynamic libraries that executable needs.
"""
import argparse
import hashlib
import os
import shutil
from pathlib import Path

p = argparse.ArgumentParser(description=__doc__)
p.add_argument("--github-env", type=Path)
p.add_argument("--binary", type=Path)
args = p.parse_args()
path = (args.binary or Path(shutil.which("ffmpeg") or "missing-ffmpeg")).resolve(strict=True)
if os.name == "nt" and args.binary is None and path.stat().st_size < 1024 * 1024:
    # Chocolatey's PATH entry is a machine-specific shim, not distributable
    # FFmpeg. Select the real binary from the just-installed package instead.
    package = Path(os.environ.get("PROGRAMDATA", r"C:\ProgramData")) / "chocolatey/lib/ffmpeg"
    candidates = [p.resolve() for p in package.glob("**/bin/ffmpeg.exe") if p.is_file()]
    if len(candidates) != 1:
        raise SystemExit("Set --binary to the real FFmpeg executable, not a package-manager shim")
    path = candidates[0]
if not path.is_file() or path.stat().st_size > 512 * 1024 * 1024:
    raise SystemExit("FFmpeg must be a regular file below 512 MiB")
with path.open("rb") as f:
    sha = hashlib.file_digest(f, "sha256").hexdigest()
text = f"OVERTALK_FFMPEG_PATH={path}\nOVERTALK_FFMPEG_SHA256={sha}\n"
if args.github_env:
    with args.github_env.open("a") as f:
        f.write(text)
else:
    print(text, end="")
