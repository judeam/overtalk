import json
import os
from pathlib import Path
import stat
import subprocess
import sys
import tempfile
import time
import unittest

from .batched_notes import SharedHelper, compatible, distribute, merged_request, run_native_notes


class BatchedNotesTests(unittest.TestCase):
    def test_failed_native_worker_does_not_leave_its_other_workers_running(self):
        with tempfile.TemporaryDirectory() as folder:
            root=Path(folder)
            helper=root/'helper'
            helper.write_text('#!'+sys.executable+'\nimport sys\nfor line in sys.stdin:\n if "shutdown" in line:break\n')
            helper.chmod(0o700)
            native=root/'native'
            native.write_text('#!'+sys.executable+'\nimport os,sys,time\n'
                'if os.environ["OVERTALK_EVAL_MEETINGS"]=="bad":sys.exit(1)\n'
                'time.sleep(30)\n')
            native.chmod(0o700)
            began=time.monotonic()
            with self.assertRaises(subprocess.CalledProcessError):
                run_native_notes(native,['bad','slow'],dict(os.environ,SONEFOLD_LLAMA_HELPER=str(helper)),root,root,workers=2)
            self.assertLess(time.monotonic()-began,5)

    def test_mixed_single_and_batch_requests_keep_output_order_and_settings(self):
        requests=[dict(type='generate',prompt='a',max_tokens=16,model_path='model',temperature=0),
                  dict(type='generate_batch',prompts=['b','c'],max_tokens=16,model_path='model',temperature=0)]
        merged=merged_request(requests,65536)
        self.assertEqual(merged['prompts'],['a','b','c'])
        self.assertEqual(merged['max_tokens'],16)
        self.assertEqual(merged['context_size'],65536)
        output=distribute(requests,dict(type='batch_response',texts=['A','B','C'],error=None))
        self.assertEqual(output[0]['text'],'A')
        self.assertEqual(output[1]['texts'],['B','C'])
        self.assertFalse(compatible(requests[0],dict(requests[0],temperature=.5)))
        with self.assertRaises(RuntimeError):distribute(requests,dict(type='batch_response',texts=['A'],error=None))

    def test_private_real_socket_roundtrip_control_and_parallel_requests(self):
        with tempfile.TemporaryDirectory() as folder:
            helper=Path(folder)/'helper'
            helper.write_text('#!'+sys.executable+'\n'+'''import sys,json
for line in sys.stdin:
 r=json.loads(line)
 if r['type']=='shutdown':break
 print(json.dumps({'type':'batch_response','texts':[p.upper() for p in r['prompts']],'error':None}),flush=True)
''')
            helper.chmod(0o700)
            broker=SharedHelper(helper,os.environ,Path(folder)/'helper.log',context_size=65536)
            try:
                self.assertEqual(stat.S_IMODE(Path(broker.address).stat().st_mode),0o600)
                self.assertEqual(stat.S_IMODE(Path(broker.directory.name).stat().st_mode),0o700)
                env=dict(os.environ,OVERTALK_EVAL_HELPER_SOCKET=broker.address)
                proxy=Path(__file__).with_name('notes_proxy.py')
                children=[subprocess.Popen([sys.executable,str(proxy)],env=env,stdin=subprocess.PIPE,
                    stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True) for _ in range(2)]
                for index,child in enumerate(children):
                    child.stdin.write(json.dumps(dict(type='generate',prompt=str(index),max_tokens=10))+'\n')
                    child.stdin.flush()
                for index,child in enumerate(children):
                    self.assertEqual(json.loads(child.stdout.readline())['text'],str(index))
                    child.stdin.write('{"type":"shutdown"}\n');child.stdin.flush()
                    self.assertEqual(json.loads(child.stdout.readline())['type'],'goodbye')
                    child.stdin.close();child.wait(timeout=5)
                    child.stdout.close();child.stderr.close()
            finally:broker.close()
            rows=[json.loads(line) for line in Path(folder,'helper.log.jsonl').read_text().splitlines()]
            # OS scheduling may place the two clients in one or two batches.
            self.assertEqual(sum(row['sequences'] for row in rows),2)
            self.assertTrue(all(row['errors']==0 for row in rows))


if __name__=='__main__':unittest.main()
