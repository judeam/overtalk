"""Install a verified local Slint build atomically, retaining the web fallback."""
import argparse
import datetime
import fcntl
import hashlib
import json
import os
from pathlib import Path
import shlex
import shutil
import sqlite3
import subprocess
import tempfile
from native_licenses import stage_licenses
from worker_build import source_fingerprint, verify_binary

ROOT = Path(__file__).resolve().parents[1]


def digest(path):
    with path.open("rb") as handle:
        return hashlib.file_digest(handle, "sha256").hexdigest()


def atomic_text(path, text, mode=0o755):
    with tempfile.NamedTemporaryFile(dir=path.parent, delete=False) as handle:
        handle.write(text.encode())
        handle.flush()
        os.fsync(handle.fileno())
        temporary = Path(handle.name)
    temporary.chmod(mode)
    os.replace(temporary, path)


def copy_runtime(source, destination):
    """Share immutable runtime libraries; resources that get restaged stay copies."""
    path = Path(source)
    if '.so' in path.name or path.suffix.lower() in {'.dll', '.dylib'}:
        try:
            os.link(source, destination)
            return destination
        except OSError:
            pass
    return shutil.copy2(source, destination)


def main():
    os.umask(0o077)
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--build-dir", type=Path, default=ROOT / "target/release")
    parser.add_argument("--runtime-from", type=Path,
                        default=Path.home() / ".local/opt/sonefold/slint/current")
    parser.add_argument("--helper", type=Path,
                        default=ROOT / "frontend/src-tauri/binaries/llama-helper-x86_64-unknown-linux-gnu",
                        help="Verified current helper binary; use target/release/llama-helper after rebuilding it")
    parser.add_argument("--gpu-launcher", type=Path, default=os.environ.get("OVERTALK_GPU_LAUNCHER") or None,
                        help="Optional launcher for GPU sharing that the app and its GPU workers run under")
    args = parser.parse_args()
    if args.gpu_launcher and not (args.gpu_launcher.is_absolute() and os.access(args.gpu_launcher, os.X_OK)):
        raise SystemExit(f"--gpu-launcher must be an absolute path to an executable: {args.gpu_launcher}")
    if not Path("/usr/bin/bwrap").is_file():
        raise SystemExit("Install bubblewrap before installing Overtalk: sudo apt install bubblewrap")
    base = Path.home() / ".local/opt/sonefold"
    channel = base / "slint"
    version = datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    binaries = {
        "overtalk": args.build_dir / "overtalk",
        "sonefold-service": args.build_dir / "sonefold-service",
        "overtalk-worker": args.build_dir / "overtalk-worker",
        "overtalk-capture": args.build_dir / "overtalk-capture",
        "overtalk-browser-bridge": args.build_dir / "overtalk-browser-bridge",
        "llama-helper": args.helper,
        "ffmpeg": ROOT / "frontend/src-tauri/binaries/ffmpeg-x86_64-unknown-linux-gnu",
    }
    for name, source in binaries.items():
        if not source.is_file() or not os.access(source, os.X_OK):
            raise SystemExit(f"Missing executable: {name}: {source}")
        with source.open("rb") as handle:
            if handle.read(4) != b"\x7fELF":
                raise SystemExit(f"Not a Linux executable: {source}")
    verify_binary(binaries["overtalk-worker"])
    if not (args.runtime_from / "lib/libonnxruntime.so").exists():
        raise SystemExit("--runtime-from must contain the verified ONNX runtime libraries")
    base.mkdir(parents=True, exist_ok=True)
    with (base / ".install.lock").open("a+") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        versions = channel / "versions"
        versions.mkdir(parents=True, exist_ok=True)
        destination = versions / version
        # A partial build never becomes current.
        with tempfile.TemporaryDirectory(prefix=".install-", dir=versions) as temporary:
            stage = Path(temporary) / "package"
            (stage / "bin").mkdir(parents=True)
            shutil.copytree(args.runtime_from / "lib", stage / "lib", symlinks=True, copy_function=copy_runtime)
            try:
                previous = json.loads((args.runtime_from / "install.json").read_text())
            except (OSError, ValueError):
                previous = {}
            for name, source in binaries.items():
                source_hash = digest(source)
                installed = args.runtime_from / "bin" / name
                target = stage / "bin" / name
                expected = previous.get("binaries", {}).get(name)
                build_hash = previous.get("build_binaries", {}).get(name)
                if installed.is_file() and source_hash in {expected, build_hash} and digest(installed) == expected:
                    try:
                        os.link(installed, target)
                        continue
                    except OSError:
                        pass
                shutil.copy2(source, target)
                if source_hash != digest(target):
                    raise RuntimeError(f"Copy checksum mismatch: {name}")
                # Only mutate newly copied files; unchanged installed binaries
                # and runtime libraries are shared between immutable releases.
                if shutil.which("strip"):
                    subprocess.run(["strip", "--strip-unneeded", str(target)], check=True)
            subprocess.run(["python3", str(ROOT / "scripts/stage-native-resources.py"), str(stage)], check=True)
            stage_licenses(stage / "share/licenses")
            icons = stage / "share/icons"
            icons.mkdir(parents=True)
            shutil.copy2(ROOT / "frontend/src-tauri/icons/icon.png", icons / "overtalk.png")
            launch = '''#!/bin/sh
set -eu
umask 077
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
unset OVERTALK_FIXTURE OVERTALK_TRACE_RENDER SLINT_DEBUG_PERFORMANCE
export OVERTALK_SERVICE="$here/bin/sonefold-service"
export SONEFOLD_LLAMA_HELPER="$here/bin/llama-helper"
export APPDIR="$here"
export LD_LIBRARY_PATH="$here/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export PATH="$here/bin:$PATH"
export SLINT_BACKEND="${SLINT_BACKEND:-winit-femtovg}"
if [ -z "${XDG_RUNTIME_DIR:-}" ] && [ -d "/run/user/$(id -u)" ]; then
    export XDG_RUNTIME_DIR="/run/user/$(id -u)"
fi
if [ -z "${PULSE_SERVER:-}" ] && [ -S "${XDG_RUNTIME_DIR:-}/pulse/native" ]; then
    export PULSE_SERVER="unix:$XDG_RUNTIME_DIR/pulse/native"
fi
logs="${XDG_STATE_HOME:-$HOME/.local/state}/overtalk-slint"
mkdir -p "$logs"
exec >>"$logs/app-$(date +%Y%m%dT%H%M%S).log" 2>&1
responsive=@GPU_LAUNCHER@
if [ -n "$responsive" ] && [ -x "$responsive" ]; then
    export OVERTALK_GPU_LAUNCHER="$responsive"
    TRAIN_CPU_NICE=0 exec "$responsive" "$here/bin/overtalk" "$@"
fi
exec "$here/bin/overtalk" "$@"
'''.replace("@GPU_LAUNCHER@", shlex.quote(str(args.gpu_launcher or "")))
            atomic_text(stage / "run", launch)
            backup = base / "backups" / f"before-native-{version}"
            backup.mkdir(parents=True, mode=0o700)
            data = Path.home() / ".local/share/com.judeam.sonefold"
            database = data / "meeting_minutes.sqlite"
            meetings = None
            if database.exists():
                with sqlite3.connect(f"file:{database}?mode=ro", uri=True) as source, sqlite3.connect(backup / database.name) as target:
                    source.backup(target)
                    meetings = target.execute("SELECT COUNT(*) FROM meetings").fetchone()[0]
                (backup / database.name).chmod(0o600)
                # Use the release's own SQLite. Older system/Python SQLite has
                # false-positive FTS5 integrity errors with secure deletion.
                # Verify the actual staged package with its own runtime. The
                # caller must not need a developer's LD_LIBRARY_PATH for this
                # otherwise self-contained installer to validate the backup.
                checked = subprocess.run([str(stage / 'bin/sonefold-service'), '--check-database', str(backup / database.name)],
                                         env={**os.environ, 'LD_LIBRARY_PATH': str(stage / 'lib')},
                                         check=True, capture_output=True, text=True)
                assert json.loads(checked.stdout)['valid']
            for settings in data.glob("*.json"):
                if settings.is_file() and settings.stat().st_size < 2 * 1024 * 1024:
                    shutil.copy2(settings, backup / settings.name)
            launchers = Path.home() / ".local/bin"
            desktops = Path.home() / ".local/share/applications"
            launchers.mkdir(parents=True, exist_ok=True)
            desktops.mkdir(parents=True, exist_ok=True)
            icon_theme = Path.home() / ".local/share/icons/hicolor"
            for size, filename in [(32, "32x32.png"), (64, "64x64.png"), (128, "128x128.png"), (256, "128x128@2x.png"), (512, "icon.png")]:
                icon_folder = icon_theme / f"{size}x{size}/apps"
                icon_folder.mkdir(parents=True, exist_ok=True)
                for name in ["overtalk.png", "sonefold.png"]:
                    shutil.copy2(ROOT / "frontend/src-tauri/icons" / filename, icon_folder / name)
            scalable = icon_theme / "scalable/apps"
            scalable.mkdir(parents=True, exist_ok=True)
            shutil.copy2(ROOT / "frontend/public/brand/icon.svg", scalable / "overtalk.svg")
            for path in [launchers / "sonefold", launchers / "overtalk-slint", desktops / "sonefold.desktop", desktops / "overtalk-slint.desktop"]:
                if path.exists():
                    shutil.copy2(path, backup / path.name)
            if (launchers / "sonefold").exists() and not (launchers / "sonefold-web").exists():
                shutil.copy2(launchers / "sonefold", launchers / "sonefold-web")
            manifest = {
                "installed_at": version,
                "source_commit": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
                "worker_source_id": source_fingerprint(),
                "source_dirty": bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT, text=True).strip()),
                "binaries": {name: digest(stage / "bin" / name) for name in binaries},
                "build_binaries": {name: digest(source) for name, source in binaries.items()},
                "database_backup": str(backup / database.name),
                "meetings_before_launch": meetings,
            }
            (stage / "install.json").write_text(json.dumps(manifest, indent=2) + "\n")
            os.rename(stage, destination)
        temporary_link = channel / f".current-{version}"
        temporary_link.symlink_to(Path("versions") / version)
        os.replace(temporary_link, channel / "current")
        for name in ["overtalk-slint", "sonefold"]:
            atomic_text(launchers / name, '#!/bin/sh\nexec "$HOME/.local/opt/sonefold/slint/current/run" "$@"\n')
        desktop = f'''[Desktop Entry]
Type=Application
Name=Overtalk
Comment=Local meeting notebook
Exec={launchers / "overtalk-slint"}
TryExec={launchers / "overtalk-slint"}
Icon=overtalk
Terminal=false
Categories=Office;AudioVideo;
StartupNotify=true
StartupWMClass=overtalk
'''
        atomic_text(desktops / "overtalk-slint.desktop", desktop + "NoDisplay=true\n", 0o644)
        # Retain the existing web bundle; both launchers now select the native UI.
        atomic_text(desktops / "sonefold.desktop", desktop, 0o644)
        if shutil.which("update-desktop-database"):
            subprocess.run(["update-desktop-database", str(desktops)], check=True)
        if shutil.which("gtk-update-icon-cache"):
            subprocess.run(["gtk-update-icon-cache", "--force", "--ignore-theme-index", str(icon_theme)], check=True)
        print(json.dumps({"installed": str(destination), "backup": str(backup), "meetings": meetings}, indent=2))


if __name__ == "__main__":
    main()
