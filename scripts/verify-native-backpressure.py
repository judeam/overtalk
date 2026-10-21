#!/usr/bin/env python3
"""Pause a disposable UI pipe and prove the service runtime remains responsive.

Two Tokio threads model a small machine. A bounded flood fills stdout and the
reply queue; an independent, authenticated loopback health request must still
complete. Draining resumes before shutdown. No user profile or models are used.
"""
import argparse
import http.client
import json
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import threading
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--report", type=Path, required=True)
    args = parser.parse_args()
    result = {}
    count = 4000
    with tempfile.TemporaryDirectory(prefix="overtalk-backpressure-") as temporary:
        root = Path(temporary)
        env = dict(os.environ, XDG_DATA_HOME=str(root / "data"),
                   XDG_CONFIG_HOME=str(root / "config"), XDG_CACHE_HOME=str(root / "cache"),
                   TOKIO_WORKER_THREADS="2")
        env.setdefault("DISPLAY", ":1")
        env.setdefault("DBUS_SESSION_BUS_ADDRESS", "unix:path=/run/user/1000/bus")
        with (root / "service.log").open("w") as log:
            child = subprocess.Popen([str(args.binary.resolve()), "--stdio"],
                                     stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                     stderr=log, env=env)
            watchdog = threading.Timer(45, child.kill)
            watchdog.start()
            replies, failures = set(), []
            drained = threading.Event()

            def receive():
                while True:
                    line = child.stdout.readline()
                    if not line:
                        raise EOFError("Service closed its pipe")
                    if line.startswith(b"OTRPC1 "):
                        return json.loads(line[7:])

            def send(identifier, command, arguments):
                value = dict(id=identifier, command=command, args=arguments)
                child.stdin.write(b"OTRPC1 " + json.dumps(value).encode() + b"\n")
                child.stdin.flush()

            def flood():
                try:
                    for identifier in range(2, count + 2):
                        send(identifier, "audit_probe_unknown", {})
                except (OSError, ValueError) as error:
                    failures.append(type(error).__name__)

            def drain():
                try:
                    while len(replies) < count:
                        message = receive()
                        if message.get("kind") == "reply":
                            identifier = message["id"]
                            assert 2 <= identifier < count + 2 and identifier not in replies
                            assert message.get("error") and message.get("result") is None
                            replies.add(identifier)
                    drained.set()
                except (OSError, ValueError, EOFError, AssertionError) as error:
                    failures.append(type(error).__name__)

            try:
                while receive().get("kind") != "ready":
                    pass
                with socket.socket() as probe:
                    probe.bind(("127.0.0.1", 0))
                    port = probe.getsockname()[1]
                send(1, "integrations_save_settings", dict(settings=dict(
                    api_port=port, api_enabled=True, read_meetings=True)))
                while True:
                    message = receive()
                    if message.get("kind") == "reply" and message.get("id") == 1:
                        break
                assert not message.get("error")
                token = message["result"]["settings"]["api_token"]

                def health():
                    connection = http.client.HTTPConnection("127.0.0.1", port, timeout=2)
                    try:
                        start = time.perf_counter()
                        connection.request("GET", "/health", headers={"Authorization": "Bearer " + token})
                        response = connection.getresponse()
                        response.read()
                        assert response.status == 200
                        return round((time.perf_counter() - start) * 1000, 3)
                    finally:
                        connection.close()

                result["before_ms"] = health()
                writer = threading.Thread(target=flood, daemon=True)
                writer.start()
                time.sleep(1)
                assert writer.is_alive(), "Request flood did not saturate the bounded pipe/queue"
                result["backpressure_observed"] = True
                try:
                    result["paused_reader_ms"] = health()
                    result["responsive"] = True
                except (TimeoutError, OSError) as error:
                    result["paused_reader_error"] = type(error).__name__
                    result["responsive"] = False
                reader = threading.Thread(target=drain, daemon=True)
                reader.start()
                writer.join(10)
                assert not writer.is_alive() and drained.wait(10), "Reply queue did not recover"
                reader.join(1)
                assert not failures, failures
                result["replies_after_recovery"] = len(replies)
                result["recovered_ms"] = health()
                send(count + 2, "shutdown", {})
                assert child.wait(timeout=10) == 0
                result["clean_shutdown"] = True
            finally:
                watchdog.cancel()
                if child.poll() is None:
                    child.kill()
                child.wait()
                child.stdin.close()
                child.stdout.close()
    args.report.parent.mkdir(parents=True, exist_ok=True)
    args.report.write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result, indent=2))
    raise SystemExit(0 if result.get("responsive") else 1)


if __name__ == "__main__":
    main()
