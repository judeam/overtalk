"""Replay recorded synthetic provider responses; refuse every unknown request.

No remote endpoint, key or GPU is used. This checks that changes targeted at one
model leave other models' real request bodies and final reports byte-identical.
It is a regression replay, never a new accuracy/latency measurement.
"""
import argparse
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import shutil
import subprocess
import threading

from .privatemode_compare import ROOT, digest


def key(body):
    return hashlib.sha256(json.dumps(body,sort_keys=True,separators=(',',':'),ensure_ascii=False).encode()).hexdigest()


class Replay:
    def __init__(self, captures):
        self.responses={}
        for path in Path(captures).glob('*.json'):
            value=json.loads(path.read_text())
            self.responses[key(value['request'])]=value['response']
        self.hits=0; self.misses=0
        replay=self

        class Handler(BaseHTTPRequestHandler):
            def log_message(self,*args):pass
            def do_POST(self):
                size=int(self.headers.get('Content-Length',0))
                if self.path!='/v1/chat/completions' or not 0<size<=8*1024*1024:
                    self.send_error(400);return
                body=json.loads(self.rfile.read(size))
                response=replay.responses.get(key(body))
                if response is None:
                    replay.misses+=1
                    response={'error':'Unrecorded request blocked; no external fallback'}
                    code=409
                else:
                    replay.hits+=1;code=200
                raw=json.dumps(response).encode()
                self.send_response(code);self.send_header('Content-Type','application/json')
                self.send_header('Content-Length',str(len(raw)));self.end_headers();self.wfile.write(raw)
        self.server=ThreadingHTTPServer(('127.0.0.1',0),Handler)
        self.thread=threading.Thread(target=self.server.serve_forever,daemon=True)

    def __enter__(self):self.thread.start();return self
    def __exit__(self,*args):self.server.shutdown();self.server.server_close();self.thread.join()


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--baseline',type=Path,required=True)
    parser.add_argument('--output',type=Path,required=True)
    parser.add_argument('--native-runner',type=Path,required=True)
    args=parser.parse_args()
    config=json.loads((args.baseline/'configuration.json').read_text())
    args.output=args.output.resolve();args.output.mkdir(parents=True,exist_ok=False)
    shutil.copytree(args.baseline/'inputs',args.output/'inputs');(args.output/'notes').mkdir()
    with Replay(args.baseline/'synthetic-response-captures') as replay:
        env=dict(os.environ,OVERTALK_EVAL_APP_DIR=str(args.output),
            OVERTALK_EVAL_INPUT_DIR=str(args.output/'inputs'),OVERTALK_EVAL_OUT=str(args.output/'notes'),
            OVERTALK_EVAL_PROVIDER='privatemode',OVERTALK_EVAL_FREE_PLAN_CONFIRMED='1',
            OVERTALK_EVAL_PRIVATEMODE_ENDPOINT=f'http://127.0.0.1:{replay.server.server_port}/v1',
            OVERTALK_EVAL_MAX_COMPLETION_TOKENS=str(config['max_completion_tokens']),
            OVERTALK_EVAL_MODELS=','.join(config['models']),OVERTALK_EVAL_MEETINGS=','.join(config['cases']),
            OVERTALK_EVAL_MODES='app',OVERTALK_EVAL_BASELINE_PROMPT='0',
            OVERTALK_EVAL_TOPIC_PLAN='0',OVERTALK_EVAL_QUOTED_DECISIONS='0',TOKIO_WORKER_THREADS='2')
        for name in ('OVERTALK_EVAL_READING_WINDOW','OVERTALK_EVAL_DEBUG_DIR'):env.pop(name,None)
        with (args.output/'native.log').open('w') as log:
            subprocess.run([str(args.native_runner.resolve()),'summary::eval_long_meetings::eval_long_meetings','--exact','--ignored','--nocapture'],
                cwd=ROOT,env=env,stdout=log,stderr=log,check=True,timeout=600)
        mismatches=[]
        for case in config['cases']:
            for model in config['models']:
                file=f'{case}__{model}__app.md'
                if not (args.output/'notes'/file).exists() or digest(args.output/'notes'/file)!=digest(args.baseline/'notes'/file):
                    mismatches.append(file)
        result=dict(hits=replay.hits,misses=replay.misses,mismatched_reports=mismatches,
            passed=replay.misses==0 and not mismatches and replay.hits>0,external_requests=0,
            native_sha256=digest(args.native_runner),baseline_configuration_sha256=digest(args.baseline/'configuration.json'))
    (args.output/'result.json').write_text(json.dumps(result,indent=2))
    (args.output/'configuration.json').write_text(json.dumps(dict(config,
        native_sha256=digest(args.native_runner), stage='offline response replay',
        source_configuration_sha256=digest(args.baseline/'configuration.json')),indent=2))
    print(json.dumps(result))


if __name__=='__main__':main()
