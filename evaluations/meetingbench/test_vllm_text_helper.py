import json
from pathlib import Path
import subprocess
import sys
import unittest

from .vllm_text_helper import requests_and_options, trim_repeated_line


class TextHelperTests(unittest.TestCase):
    def test_sampling_and_total_context_budget_follow_native_request(self):
        prompts, options, context = requests_and_options(dict(type='generate_batch', prompts=['a','b'],
            model_path='/tmp/model', max_tokens=1024, context_size=8192,
            temperature=.2, top_k=20, top_p=.9, penalty_last_n=0,
            presence_penalty=.3, stop_tokens=['DONE']), '/tmp/model')
        self.assertEqual(prompts,['a','b'])
        self.assertEqual(context,4096)
        self.assertEqual(options,dict(temperature=.2,top_k=20,top_p=.9,max_tokens=1024,stop=['DONE']))

    def test_unsupported_penalty_window_is_never_silently_changed(self):
        with self.assertRaisesRegex(ValueError,'Sliding-window'):
            requests_and_options(dict(type='generate',prompt='a',penalty_last_n=256,
                                      repeat_penalty=1.05),'/tmp/model')

    def test_foreign_model_and_invalid_budgets_are_rejected(self):
        for change in [dict(model_path='/other'),dict(context_size=999999),dict(max_tokens=-1),
                       dict(prompts=[]),dict(stop_tokens=[None])]:
            with self.subTest(change=change),self.assertRaises(ValueError):
                requests_and_options({**dict(type='generate_batch',prompts=['a']),**change},'/tmp/model')

    def test_repeated_completed_lines_ignore_ascii_digits(self):
        text='Task 1: Review\nDifferent task\nTask 2: Review\nTrailing text'
        self.assertEqual(trim_repeated_line(text),'Task 1: Review\nDifferent task')
        self.assertEqual(trim_repeated_line('First\nFirst'),'First\nFirst')
        self.assertEqual(trim_repeated_line('First\nFirst\n'),'First')

    def test_control_transport_does_not_import_or_load_a_model(self):
        script=Path(__file__).with_name('vllm_text_helper.py')
        result=subprocess.run([sys.executable,str(script)],input='{"type":"ping"}\n{"type":"shutdown"}\n',
                              text=True,capture_output=True,check=True,timeout=5)
        self.assertEqual([json.loads(line) for line in result.stdout.splitlines()],
                         [dict(type='pong'),dict(type='goodbye')])


if __name__ == '__main__':
    unittest.main()
