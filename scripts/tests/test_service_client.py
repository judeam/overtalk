from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from service_client import Service  # noqa: E402

# Answers every request with an event first and the reply second, which is the
# order a fast background job (an import that fails at once) can produce.
FAKE_SERVICE = """
import json, sys
def send(value):
    sys.stdout.write("OTRPC1 " + json.dumps(value) + "\\n")
    sys.stdout.flush()
send({"kind": "ready", "commands": []})
for line in sys.stdin:
    request = json.loads(line[7:])
    if request["command"] == "shutdown":
        break
    send({"kind": "event", "event": "job-finished", "payload": {"for": request["id"]}})
    send({"kind": "reply", "id": request["id"], "result": request["args"], "error": None})
"""


class ServiceClientTests(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name)
        binary = self.root / "fake-service"
        binary.write_text(f"#!{sys.executable}\n{FAKE_SERVICE}")
        binary.chmod(0o755)
        self.service = Service(binary, self.root)
        self.addCleanup(self.service.close)

    def test_event_emitted_before_the_reply_is_still_delivered(self):
        self.assertEqual(self.service.call("start_job", value=3), {"value": 3})
        event = self.service.wait_for_event({"job-finished"}, timeout=5)
        self.assertEqual(event["payload"], {"for": 1})
        self.assertFalse(self.service.events)

    def test_waiting_for_an_event_that_never_comes_times_out(self):
        self.service.call("start_job")
        with self.assertRaises(TimeoutError):
            self.service.wait_for_event({"something-else"}, timeout=0.2)


if __name__ == "__main__":
    unittest.main()
