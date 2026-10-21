import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

from .continuous_notes import run_server_notes


class ContinuousNotesTests(unittest.TestCase):
    def test_worker_failure_stops_other_workers_and_owned_server(self):
        class FakeServer:
            address = 'http://127.0.0.1:9876'
            token = 'x' * 32
            model = '/tmp/model'
            slot_context = 8192
            closed = False

            def close(self):
                self.closed = True

        server = FakeServer()
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            native = root/'native'
            native.write_text('#!' + sys.executable + '\nimport os,sys,time\n'
                'if os.environ["OVERTALK_EVAL_MEETINGS"] == "bad":sys.exit(1)\n'
                'time.sleep(30)\n')
            native.chmod(0o700)
            started = time.monotonic()
            with patch('evaluations.meetingbench.continuous_notes.OwnedServer', return_value=server):
                with self.assertRaises(subprocess.CalledProcessError):
                    run_server_notes(native, ['slow', 'bad'], dict(os.environ), root, root,
                                     root/'server', root/'model', workers=2)
            self.assertTrue(server.closed)
            self.assertLess(time.monotonic()-started, 5)


if __name__ == '__main__':
    unittest.main()
