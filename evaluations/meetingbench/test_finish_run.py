import tempfile
import unittest
from pathlib import Path

from .finish_run import is_module_process


class ProcessIdentityTests(unittest.TestCase):
    def test_reused_or_missing_pid_does_not_count_as_evaluator(self):
        with tempfile.TemporaryDirectory() as directory:
            proc = Path(directory)
            self.assertFalse(is_module_process(123, 'evaluations.meetingbench.run_evaluation', proc))
            (proc/'123').mkdir()
            command = proc/'123/cmdline'
            command.write_bytes(b'python\0-m\0evaluations.meetingbench.run_evaluation\0')
            self.assertTrue(is_module_process(123, 'evaluations.meetingbench.run_evaluation', proc))
            command.write_bytes(b'python\0other_process.py\0')
            self.assertFalse(is_module_process(123, 'evaluations.meetingbench.run_evaluation', proc))


if __name__ == '__main__':
    unittest.main()
