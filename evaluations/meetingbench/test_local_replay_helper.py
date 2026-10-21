import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


class LocalReplayTests(unittest.TestCase):
    def test_replays_only_identical_inference_requests(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory)
            request={'type':'generate','prompt':'synthetic known prompt','model_path':'synthetic.gguf','temperature':0.2}
            raw=json.dumps(request,sort_keys=True,separators=(',',':'),ensure_ascii=False)
            digest=hashlib.sha256(raw.encode()).hexdigest()
            answer={'type':'response','text':'Recorded synthetic notes','error':None}
            (root/f'{digest}.json').write_text(json.dumps({'request':request,'response':answer}))
            commands=[request,{**request,'prompt':'changed prompt'},{'type':'shutdown'}]
            result=subprocess.run([sys.executable,str(Path(__file__).with_name('local_replay_helper.py'))],
                input='\n'.join(json.dumps(c) for c in commands)+'\n',text=True,capture_output=True,check=True,
                env=dict(os.environ,OVERTALK_EVAL_REPLAY_DIR=str(root),OVERTALK_EVAL_REPLAY_LOG=str(root/'calls.jsonl')))
            replies=[json.loads(line) for line in result.stdout.splitlines()]
            self.assertEqual(replies[0],answer)
            self.assertIn('Unrecorded',replies[1]['error'])
            self.assertEqual(replies[2]['type'],'goodbye')
            logs=[json.loads(line) for line in (root/'calls.jsonl').read_text().splitlines()]
            self.assertEqual([r['hit'] for r in logs],[True,False])
            self.assertNotIn('synthetic known prompt',(root/'calls.jsonl').read_text())


if __name__=='__main__':unittest.main()
