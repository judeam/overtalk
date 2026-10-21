"""Smoke-test the private native engine with an isolated, disposable database.

Linux resolves the engine's folders from XDG_*, and macOS from HOME, so both
point into a temporary directory. Windows resolves %APPDATA% through the Known
Folder API, which ignores environment overrides: there the check refuses to run
beside an existing Overtalk profile (hosted CI runners start without one) and
removes the profile it created afterwards.
"""
import argparse
import json
import os
from pathlib import Path
import queue
import shutil
import subprocess
import sys
import tempfile
import threading

root = Path(__file__).resolve().parents[1]
# Tauri's application identifier; the engine keeps its profile in a folder of this name.
IDENTIFIER = "com.judeam.sonefold"


def isolated_environment(temporary, platform=sys.platform, base=None):
    env = dict(os.environ if base is None else base)
    env.update(XDG_DATA_HOME=f"{temporary}/data", XDG_CONFIG_HOME=f"{temporary}/config",
               XDG_CACHE_HOME=f"{temporary}/cache")
    if platform == "darwin":
        home = Path(temporary) / "home"
        home.mkdir(exist_ok=True)
        env["HOME"] = str(home)
    elif platform.startswith("linux"):
        env.setdefault("DISPLAY", ":1")
        env.setdefault("DBUS_SESSION_BUS_ADDRESS", "unix:path=/run/user/1000/bus")
    return env


def windows_profiles(env):
    """The folders a Windows engine would create for its profile."""
    return [Path(env[name]) / IDENTIFIER for name in ("APPDATA", "LOCALAPPDATA") if env.get(name)]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", nargs="?", type=Path, default=root / "target/release/sonefold-service")
    args = parser.parse_args()
    if not args.binary.is_file():
        raise SystemExit(f"No engine at {args.binary}")
    created = []
    if os.name == "nt":
        existing = [path for path in windows_profiles(os.environ) if path.exists()]
        if existing:
            raise SystemExit("Refusing to run beside an existing Overtalk profile on Windows: "
                             + ", ".join(map(str, existing)))
        created = windows_profiles(os.environ)
    try:
        with tempfile.TemporaryDirectory(prefix="overtalk-service-") as temporary:
            check(args.binary, isolated_environment(temporary), temporary)
    finally:
        for path in created:
            shutil.rmtree(path, ignore_errors=True)


def check(binary, env, temporary):
    messages = queue.Queue()
    with open(Path(temporary) / "service.log", "w") as log:
        process = subprocess.Popen([str(binary), "--stdio"], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                   stderr=log, env=env)

        def collect():
            for line in process.stdout:
                if line.startswith(b"OTRPC1 "):
                    messages.put(json.loads(line[7:]))

        threading.Thread(target=collect, daemon=True).start()

        def call(identifier, command, args=None):
            process.stdin.write(b"OTRPC1 " + json.dumps({"id": identifier, "command": command, "args": args or {}}).encode() + b"\n")
            process.stdin.flush()
            while True:
                response = messages.get(timeout=20)
                if response.get("id") == identifier:
                    return response

        try:
            ready = messages.get(timeout=60)
            assert ready["kind"] == "ready", ready
            response = call(1, "api_get_meetings")
            assert response["error"] is None and response["result"] == [], response
            response = call(2, "unsupported_command")
            assert response["error"] == "Unsupported native command", response
            response = call(3, "api_get_meeting_metadata", {"meetingId": 42})
            assert "Invalid or missing argument" in response["error"], response
            process.stdin.write(b'OTRPC1 {"id":4,"command":"shutdown","args":{}}\n')
            process.stdin.flush()
            assert process.wait(timeout=30) == 0
            print("Native service: fresh database, bounded typed commands, rejection and clean shutdown passed")
        except BaseException:
            log.flush()
            print((Path(temporary) / "service.log").read_text(errors="replace")[-4000:], file=sys.stderr)
            raise
        finally:
            if process.poll() is None:
                process.kill()
                process.wait()


if __name__ == "__main__":
    main()
