import json
from pathlib import Path
import subprocess
import sys
import threading
import unittest

from .server_text_helper import completion_requests, endpoint, ServerClient


class ServerTextHelperTests(unittest.TestCase):
    def test_only_an_authenticated_loopback_endpoint_is_accepted(self):
        self.assertEqual(endpoint('http://127.0.0.1:9876/'), 'http://127.0.0.1:9876')
        for value in ['https://127.0.0.1:9876', 'http://example.com:9876',
                      'http://127.0.0.1:9876/else', 'http://secret@127.0.0.1:9876',
                      'http://127.0.0.1:9876?proxy=1']:
            with self.subTest(value=value), self.assertRaises(ValueError):
                endpoint(value)
        with self.assertRaises(ValueError):
            ServerClient('http://127.0.0.1:9876', '', '/tmp/model')

    def test_native_sampling_disables_server_only_defaults(self):
        jobs, context = completion_requests(dict(type='generate_batch', prompts=['a', 'b'],
            context_size=8192, model_path='/tmp/model', max_tokens=1024,
            temperature=.2, top_k=20, top_p=.9, repeat_penalty=1.,
            penalty_last_n=128,
            stop_tokens=['END']), '/tmp/model')
        self.assertEqual(context, 4096)
        self.assertEqual([job['prompt'] for job in jobs], ['a', 'b'])
        self.assertEqual(jobs[0]['min_p'], 0)
        self.assertEqual(jobs[0]['samplers'], ['penalties', 'top_k', 'top_p', 'temperature'])
        self.assertEqual(jobs[0]['repeat_last_n'], 128)
        self.assertEqual(jobs[0]['presence_penalty'], 0.)
        self.assertEqual(jobs[0]['frequency_penalty'], 0.)
        self.assertEqual(jobs[0]['n_predict'], 1024)
        self.assertEqual(jobs[0]['stop'], ['END'])

    def test_prompt_inclusive_penalties_are_not_substituted_for_native_policy(self):
        with self.assertRaisesRegex(ValueError, 'generation-only penalties'):
            completion_requests(dict(type='generate', prompt='a', penalty_last_n=128,
                                     repeat_penalty=1.05), '/tmp/model')

    def test_invalid_or_foreign_requests_do_not_reach_inference(self):
        for change in [dict(model_path='/other'), dict(context_size=999999),
                       dict(max_tokens=-1), dict(prompts=[]), dict(stop_tokens=[None])]:
            with self.subTest(change=change), self.assertRaises(ValueError):
                completion_requests({**dict(type='generate_batch', prompts=['a']), **change}, '/tmp/model')

    def test_sequences_execute_concurrently_but_keep_their_output_order(self):
        client = ServerClient('http://127.0.0.1:9876', 'x' * 32, '/tmp/model')
        barrier = threading.Barrier(2)

        def post(route, payload):
            if route == '/tokenize':
                return dict(tokens=[1])
            barrier.wait(timeout=2)
            return dict(content='answer ' + payload['prompt'])

        client.post = post
        result = client.generate(dict(type='generate_batch', prompts=['first', 'second'], context_size=8192))
        self.assertEqual(result, dict(type='batch_response', texts=['answer first', 'answer second'], error=None))

    def test_token_budget_violation_prevents_generation(self):
        client = ServerClient('http://127.0.0.1:9876', 'x' * 32, '/tmp/model')
        calls = []

        def post(route, payload):
            calls.append(route)
            return dict(tokens=[1] * 1800)

        client.post = post
        with self.assertRaisesRegex(ValueError, 'context budget'):
            client.generate(dict(type='generate', prompt='too long', context_size=2048, max_tokens=512))
        self.assertEqual(calls, ['/tokenize'])

    def test_native_loop_policy_is_preserved_in_returned_output(self):
        client = ServerClient('http://127.0.0.1:9876', 'x' * 32, '/tmp/model')
        client.post = lambda route, payload: (dict(tokens=[1]) if route == '/tokenize'
            else dict(content='Task 1: Review\nSomething else\nTask 2: Review\n'))
        result = client.generate(dict(type='generate', prompt='a', stop_on_repeated_line=True))
        self.assertEqual(result['text'], 'Task 1: Review\nSomething else')

    def test_control_messages_do_not_require_a_server(self):
        script = Path(__file__).with_name('server_text_helper.py')
        result = subprocess.run([sys.executable, str(script)],
            input='{"type":"ping"}\n{"type":"shutdown"}\n',
            text=True, capture_output=True, check=True, timeout=5)
        self.assertEqual([json.loads(line) for line in result.stdout.splitlines()],
                         [dict(type='pong'), dict(type='goodbye')])


if __name__ == '__main__':
    unittest.main()
