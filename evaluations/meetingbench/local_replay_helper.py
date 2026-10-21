#!/usr/bin/env python3
"""Offline synthetic inference replay. No model loading or network fallback."""
import hashlib
import json
import os
from pathlib import Path
import sys


def main():
    directory=Path(os.environ['OVERTALK_EVAL_REPLAY_DIR'])
    for line in sys.stdin:
        if len(line)>32*1024*1024:raise ValueError('Oversized request')
        request=json.loads(line)
        kind=request.get('type')
        if kind in ('ping','shutdown'):
            answer={'type':'pong' if kind=='ping' else 'goodbye'}
        else:
            raw=json.dumps(request,sort_keys=True,separators=(',',':'),ensure_ascii=False)
            name=hashlib.sha256(raw.encode()).hexdigest()
            path=directory/f'{name}.json'
            if path.is_file():
                value=json.loads(path.read_text())
                assert value['request']==request
                answer=value['response']
                hit=True
            else:
                answer={'type':'response' if kind=='generate' else 'batch_response',
                        'error':'Unrecorded inference request blocked; no fallback','text':'','texts':[]}
                hit=False
            with Path(os.environ['OVERTALK_EVAL_REPLAY_LOG']).open('a') as log:
                log.write(json.dumps({'request_sha256':name,'hit':hit})+'\n')
        print(json.dumps(answer),flush=True)
        if kind=='shutdown':break


if __name__=='__main__':main()
