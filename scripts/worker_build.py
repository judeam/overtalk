"""Build/stage the workspace worker and reject stale native install artifacts."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
INPUTS = (
    "Cargo.lock",
    "crates/worker-sandbox/Cargo.toml",
    "crates/worker-sandbox/build.rs",
    "crates/worker-sandbox/build_identity.rs",
    "crates/worker-sandbox/src/lib.rs",
    "crates/worker-sandbox/src/main.rs",
    "crates/worker-sandbox/src/windows.rs",
)


def source_fingerprint():
    digest = hashlib.sha256(b"overtalk-worker-source-v1\0")
    for name in INPUTS:
        digest.update(name.encode())
        digest.update(b"\0")
        digest.update((ROOT / name).read_bytes())
        digest.update(b"\0")
    return digest.hexdigest()


def verify_binary(binary):
    try:
        result = subprocess.run([str(binary), "--build-identity"], capture_output=True,
                                text=True, timeout=10, check=True)
    except (OSError, subprocess.SubprocessError) as error:
        raise RuntimeError("Sandbox worker lacks a valid build identity; rebuild overtalk-worker") from error
    if result.stdout.strip() != source_fingerprint():
        raise RuntimeError("Sandbox worker is stale; rebuild overtalk-worker from current sources")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--debug", action="store_true")
    parser.add_argument("--target", default=os.environ.get("TAURI_ENV_TARGET_TRIPLE"))
    args = parser.parse_args()
    version = subprocess.check_output(["rustc", "-vV"], text=True)
    host = next(line.split(": ", 1)[1] for line in version.splitlines() if line.startswith("host: "))
    target = args.target or host
    command = ["cargo", "build", "--locked", "-p", "overtalk-worker-sandbox",
               "--bin", "overtalk-worker", "--message-format=json"]
    if target != host or os.environ.get("CARGO_BUILD_TARGET"):
        command.extend(["--target", target])
    if not args.debug:
        command.append("--release")
    result = subprocess.run(command, cwd=ROOT, stdout=subprocess.PIPE, text=True, check=True)
    executable = None
    for line in result.stdout.splitlines():
        record = json.loads(line)
        if record.get("reason") == "compiler-artifact" and record.get("target", {}).get("name") == "overtalk-worker":
            executable = record.get("executable")
    if not executable:
        raise RuntimeError("Cargo returned no worker executable")
    if target == host:
        verify_binary(Path(executable))
    suffix = ".exe" if "windows" in target else ""
    destination = ROOT / "frontend/src-tauri/binaries" / f"overtalk-worker-{target}{suffix}"
    destinations = [destination]
    if target == host:
        # Tauri may omit Cargo's --target for a host build. Keep that output
        # layout usable without allowing tauri-build to copy an old seed back.
        metadata = json.loads(subprocess.check_output(
            ["cargo", "metadata", "--offline", "--no-deps", "--format-version=1"], cwd=ROOT, text=True))
        profile = "debug" if args.debug else "release"
        destinations.append(Path(metadata["target_directory"]) / profile / f"overtalk-worker{suffix}")
        destinations.append(Path(metadata["target_directory"]) / target / profile / f"overtalk-worker{suffix}")
    for destination in destinations:
        if destination.resolve() == Path(executable).resolve():
            continue
        destination.parent.mkdir(parents=True, exist_ok=True)
        with tempfile.NamedTemporaryFile(dir=destination.parent, delete=False) as temporary:
            staged = Path(temporary.name)
        try:
            shutil.copy2(executable, staged)
            os.replace(staged, destination)
        finally:
            staged.unlink(missing_ok=True)
    print(f"Staged current workspace worker for {target}")


if __name__ == "__main__":
    main()
