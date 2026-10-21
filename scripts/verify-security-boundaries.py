#!/usr/bin/env python3
"""Exercise local API authorization and protected CLI reads in a disposable library."""
from service_client import Service
import argparse
import http.client
import importlib.util
import json
import select
from pathlib import Path
import socket
import sqlite3
import tempfile
import time
import uuid

ROOT = Path(__file__).resolve().parents[1]


def module(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    value = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(value)
    return value


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--report", type=Path)
    args = parser.parse_args()
    companion = module("companion", ROOT / "integrations/sonefold.py")
    checks = {}
    with tempfile.TemporaryDirectory(prefix="overtalk-boundaries-") as temp:
        root = Path(temp)
        data = root / "data/com.judeam.sonefold"
        outbox = data / "webhook-outbox"
        outbox.mkdir(parents=True)
        stale = outbox / f"{uuid.uuid4()}.json"
        stale.write_text('{"summary_markdown":"synthetic obsolete payload"}')
        service = Service(args.binary.resolve(), root)
        try:
            assert not stale.exists(), "Obsolete webhook body was retained"
            checks["legacy_webhook_payload_cleanup"] = "passed"
            saved = service.call("api_save_transcript", meetingTitle="Synthetic boundary check",
                folderPath=None, transcripts=[dict(id="line", text="Synthetic boundary words",
                    timestamp="00:00", audio_start_time=0., audio_end_time=2., duration=2.)])
            meeting = saved["meeting_id"]
            with socket.socket() as port_probe:
                port_probe.bind(("127.0.0.1", 0))
                port = port_probe.getsockname()[1]
            settings = service.call("integrations_save_settings", settings=dict(
                api_port=port, cli_enabled=True, api_enabled=True,
                read_meetings=True, read_transcripts=True, read_summaries=True))
            token = settings["settings"]["cli_token"]

            def request(operation="get_meeting_details", headers=None):
                client = http.client.HTTPConnection("127.0.0.1", port, timeout=5)
                try:
                    client.request("POST", "/v1/call", json.dumps({"name":operation,
                        "arguments":{"meeting_id":meeting}}), headers={
                        "Content-Type":"application/json", **(headers or {})})
                    response = client.getresponse()
                    return response.status, json.loads(response.read(1024 * 1024))
                finally:
                    client.close()

            auth = {"Authorization":"Bearer " + token}
            assert request()[0] == 401
            assert request(headers={**auth, "Origin":"https://attacker.invalid"})[0] == 403
            assert request(headers={**auth, "Host":"attacker.invalid"})[0] == 403
            assert request(headers=auth)[0] == 200
            checks["token_origin_and_host_gates"] = "passed"
            # Hold sockets before sending any HTTP headers. Request-level
            # semaphores alone cannot bound this earlier allocation surface.
            idle = []
            try:
                for _ in range(40):
                    idle.append(socket.create_connection(("127.0.0.1", port), timeout=2))
                closed = set()
                deadline = time.monotonic() + 2
                while len(closed) < 8 and time.monotonic() < deadline:
                    readable, _, _ = select.select([s for s in idle if s not in closed], [], [], .05)
                    for sock in readable:
                        try:
                            if not sock.recv(1):
                                closed.add(sock)
                        except ConnectionResetError:
                            closed.add(sock)
                assert len(closed) >= 8, "Idle connections exceeded the 32-connection limit"
            finally:
                for sock in idle:
                    sock.close()
            # Closing clients releases their leases; the API remains usable.
            deadline = time.monotonic() + 3
            while True:
                try:
                    assert request(headers=auth)[0] == 200
                    break
                except (ConnectionError, http.client.HTTPException):
                    if time.monotonic() >= deadline:
                        raise
                    time.sleep(.05)
            checks["idle_connection_limit_and_recovery"] = "passed"
            database = data / "meeting_minutes.sqlite"
            # Only this disposable fixture bypasses the deployment setup UI to
            # isolate the audit boundary from the machine's disk-encryption state.
            with sqlite3.connect(database) as db:
                row = db.execute("SELECT policy FROM privacy_settings WHERE id=1").fetchone()
                policy = json.loads(row[0])
                policy["protected_mode"] = True
                db.execute("UPDATE privacy_settings SET policy=? WHERE id=1", (json.dumps(policy),))
            reader = companion.MeetingReader(Path(settings["config_path"]), "cli")
            assert reader.meetings()[0]["id"] == meeting
            assert reader.meeting(meeting)["transcripts"][0]["text"] == "Synthetic boundary words"
            with sqlite3.connect(database) as db:
                operations = [row[0] for row in db.execute("SELECT operation FROM privacy_events")]
                assert "requested:automation:list_meetings" in operations
                assert "automation:get_meeting_details" in operations
            checks["protected_cli_reads_are_audited"] = "passed"
            with sqlite3.connect(database) as db:
                db.executescript("CREATE TRIGGER deny_test_audit BEFORE INSERT ON privacy_events BEGIN SELECT RAISE(ABORT,'synthetic audit failure'); END;")
            status, denied = request(headers=auth)
            assert status != 200 and "result" not in denied
            assert "Synthetic boundary words" not in json.dumps(denied)
            checks["audit_failure_denies_content"] = "passed"
        finally:
            service.close()
    if args.report:
        args.report.parent.mkdir(parents=True, exist_ok=True)
        args.report.write_text(json.dumps(checks, indent=2) + "\n")
    print(json.dumps(checks, indent=2))


if __name__ == "__main__":
    main()
