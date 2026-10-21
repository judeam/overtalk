import json
import os
from pathlib import Path
import tempfile
import unittest
from urllib.request import Request, ProxyHandler, build_opener

from .privatemode_compare import Budget, Gateway, local_endpoint, read_key


class PrivatemodeTests(unittest.TestCase):
    def test_only_local_urls_without_credentials_are_allowed(self):
        self.assertEqual(local_endpoint('http://localhost:8080'), 'http://127.0.0.1:8080/v1')
        self.assertEqual(local_endpoint('http://[::1]:8080/v1/'), 'http://[::1]:8080/v1')
        for url in ['https://api.privatemode.ai/v1', 'http://localhost.example/v1',
                    'http://key@127.0.0.1/v1', 'http://127.0.0.1/v1?key=secret',
                    'http://127.0.0.1/v1#secret', 'file:///tmp/key', 'http://127.0.0.1/wrong']:
            with self.assertRaises(ValueError):
                local_endpoint(url)

    def test_ceiling_is_reserved_before_request_and_gpt_oss_counts_twice(self):
        body = dict(model='gpt-oss-120b', max_completion_tokens=100, messages=[])
        budget = Budget(10_000)
        reserved, multiplier = budget.reserve(body)
        self.assertEqual(multiplier, 2)
        self.assertGreater(reserved, 2048)
        budget.settle(reserved, multiplier, dict(prompt_tokens=10, completion_tokens=20))
        self.assertEqual(budget.used, 60)
        budget = Budget(5)
        with self.assertRaises(ValueError):
            budget.reserve(body)
        self.assertEqual(budget.used, 0)
        self.assertTrue(budget.stopped)

    def test_missing_usage_keeps_full_reservation(self):
        budget = Budget(100_000)
        reserved, multiplier = budget.reserve(dict(model='glm-5.3', max_completion_tokens=100, messages=[]))
        budget.settle(reserved, multiplier, {})
        self.assertEqual(budget.used, reserved)
        for body in [dict(model='unknown', max_completion_tokens=100),
                     dict(model='glm-5.3', max_completion_tokens=0),
                     dict(model='glm-5.3', max_completion_tokens=True)]:
            with self.assertRaises(ValueError):
                budget.reserve(body)

    @unittest.skipUnless(os.name == 'posix', 'POSIX key-file permissions')
    def test_key_requires_private_regular_file(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)/'key'
            path.write_text('synthetic-test-key')
            path.chmod(0o600)
            self.assertEqual(read_key(path), 'synthetic-test-key')
            path.chmod(0o644)
            with self.assertRaises(ValueError):
                read_key(path)
            path.chmod(0o600)
            link = Path(directory)/'link'
            link.symlink_to(path)
            with self.assertRaises(ValueError):
                read_key(link)

    def test_gateway_blocks_overspend_without_forwarding_or_leaking_key(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)/'usage.json'
            with Gateway('http://127.0.0.1:1', 'never-log-this-key', output, 1) as gateway:
                request = Request(f'http://127.0.0.1:{gateway.server.server_port}/v1/chat/completions',
                    data=json.dumps(dict(model='glm-5.3', max_completion_tokens=100, messages=[])).encode(),
                    headers={'Content-Type': 'application/json'})
                with self.assertRaises(Exception) as error:
                    build_opener(ProxyHandler({})).open(request)
                self.assertEqual(error.exception.code, 503)
                self.assertEqual(gateway.budget.used, 0)
            self.assertNotIn('never-log-this-key', output.read_text())


if __name__ == '__main__':
    unittest.main()
