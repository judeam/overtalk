"""Regression checks for interrupted benchmark engines and safe resumption."""
import json
from pathlib import Path
import queue
import tempfile
import unittest
from unittest.mock import Mock, patch

from . import run_evaluation as runner


class RecoveryTests(unittest.TestCase):
    def test_unresponsive_engine_does_not_receive_more_work(self):
        client = object.__new__(runner.NativeClient)
        client.serial = 0
        client.process = Mock()
        client.process.poll.return_value = None
        client.messages = Mock()
        client.messages.get.side_effect = queue.Empty()
        with self.assertRaises(runner.NativeEngineExited):
            client.call('api_get_summary')

    def test_dead_engine_is_a_fatal_transport_error(self):
        client = object.__new__(runner.NativeClient)
        client.serial = 0
        client.process = Mock()
        client.process.poll.return_value = -9
        client.messages = queue.Queue()
        client.messages.put({'engine_exited': -9})
        with self.assertRaises(runner.NativeEngineExited):
            client.call('api_get_summary')

    def test_disconnect_stops_before_marking_other_cases_failed(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            for key in ['normal-001', 'normal-002']:
                path = root / 'corpus' / key
                path.mkdir(parents=True)
                (path / 'reference.json').write_text(json.dumps(dict(
                    id=key, duration_seconds=30, difficulty='normal',
                    languages=['en'], turns=[dict(text='A test meeting.')]
                )))
            with patch.object(runner, 'RUNTIME', root), \
                 patch.object(runner, 'NativeClient') as factory, \
                 patch.object(runner, 'evaluate', side_effect=runner.NativeEngineExited('lost')), \
                 patch.object(runner, 'write_report'), \
                 patch('sys.argv', ['run_evaluation', '--limit', '2']):
                with self.assertRaises(runner.NativeEngineExited):
                    runner.main()
            self.assertEqual(len(list((root / 'results').glob('*/result.json'))), 1)
            factory.return_value.close.assert_called_once()

    def test_retry_preserves_failed_attempt_and_skips_successes(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            bad = root / 'results' / 'normal-001'
            good = root / 'results' / 'normal-002'
            bad.mkdir(parents=True)
            good.mkdir()
            failure = {'id': 'normal-001', 'status': 'failed', 'error': 'lost'}
            (bad / 'result.json').write_text(json.dumps(failure))
            (bad / 'transcript.json').write_text('{"partial": true}')
            (good / 'result.json').write_text('{"id":"normal-002","status":"completed"}')
            retried = runner.archive_failed_results(root)
            self.assertEqual(retried, {'normal-001'})
            archived = list((root / 'attempts' / 'normal-001').glob('*/result.json'))
            self.assertEqual(len(archived), 1)
            self.assertEqual(json.loads(archived[0].read_text()), failure)
            self.assertTrue((archived[0].parent / 'transcript.json').exists())
            self.assertFalse(bad.exists())
            self.assertTrue(good.exists())


if __name__ == '__main__':
    unittest.main()
