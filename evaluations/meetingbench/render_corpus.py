"""Resumable, budget-bounded cloud synthesis of the 200 controlled scripts.

Does not touch the user's library. No API calls create scripts. A single ledger
includes earlier probes and listening samples. Incomplete cases stay failures.
"""
from concurrent.futures import ThreadPoolExecutor, as_completed
from itertools import zip_longest
import argparse
import json
import os
from pathlib import Path
import threading
import time

from .gemini import Gemini, BudgetExceeded, atomic_json, config_from_env_file
from .make_preview import render


class Pace:
    def __init__(self, rate):
        self.rate=rate;self.next=0.;self.lock=threading.Lock()

    def __call__(self):
        with self.lock:
            at=max(time.monotonic(),self.next);self.next=at+1/self.rate
        time.sleep(max(0,at-time.monotonic()))


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--workers',type=int,default=64)
    parser.add_argument('--requests-per-second',type=float,default=8)
    parser.add_argument('--limit',type=int)
    parser.add_argument('--config',type=Path,default=os.environ.get('OVERTALK_TTS_ENV'),
                        help='File with the Gemini API key (KEY=value lines); defaults to $OVERTALK_TTS_ENV')
    args=parser.parse_args()
    if not args.config:
        parser.error('set --config or OVERTALK_TTS_ENV to the file holding the Gemini key')
    if not 1<=args.workers<=192 or not .1<=args.requests_per_second<=12:
        parser.error('At most 192 workers and 12 requests per second; the budget ledger is authoritative')
    os.umask(0o077)
    root=Path(__file__).parent/'.runtime';out=root/'corpus';out.mkdir(exist_ok=True)
    atomic_json(out/'render-process.json',dict(pid=os.getpid(),workers=args.workers,requests_per_second=args.requests_per_second))
    scripts=[json.loads(p.read_text()) for p in sorted((root/'scripts').glob('*.json'))]
    if len(scripts)!=200:raise ValueError('Expected all 200 structurally validated scripts')
    scripts.sort(key=lambda s:(s['duration_seconds'],s['id']))
    if args.limit:scripts=scripts[:args.limit]
    client=Gemini(config_from_env_file(args.config),root,request_timeout=240)
    client.request_gate=Pace(args.requests_per_second)
    custom_voice=json.loads(next((root/'voices').glob('*.json')).read_text())['id']
    states={s['id']:dict(script=s,clips=[None]*len(s['turns']),remaining=len(s['turns']),errors=[]) for s in scripts}
    for key,state in states.items():
        folder=out/key;folder.mkdir(exist_ok=True)
        old=folder/'reference.json'
        if old.exists():
            saved=json.loads(old.read_text())
            if saved.get('script_sha256')!=state['script']['script_sha256']:
                raise ValueError('Existing audio belongs to a different script: '+key)
            state['complete']=True;state['remaining']=0
    began=time.monotonic();done=sum(len(s['clips']) for s in states.values() if s.get('complete'));mixing={};last_save=0;phase='running'
    clip_goal=sum(len(s['clips']) for s in states.values())

    def save():
        atomic_json(out/'progress.json',dict(
            state=phase,requested=len(scripts),completed=sum(bool(s.get('complete')) for s in states.values()),
            speech_clips_completed=done,speech_clips_total=clip_goal,
            elapsed_seconds=time.monotonic()-began,cost=client.ledger.report(),
            cases={key:dict(remaining=s['remaining'],completed=bool(s.get('complete')),errors=s['errors']) for key,s in states.items()},
            dataset_provenance='Parameterized assistant-authored controlled scenarios; not independently validated natural meetings'))

    def synthesize(key,index):
        script=states[key]['script'];turn=script['turns'][index]
        person=next(p for p in script['people'] if p['id']==turn['speaker'])
        style=person['style']+f" Delivery: {turn['emotion']}. "
        if index>=len(script['turns'])-6:style+='Clear, steady recap; not theatrical.'
        if script.get('dialect'):style+=f" Requested regional variety: {script['dialect']}; do not change the written meaning."
        voice=custom_voice if person['voice']=='custom-existing' else person['voice']
        path,seconds=client.speak(turn['text'],voice,style)
        return key,index,dict(path=str(path),seconds=seconds)

    def finish_mixes():
        for future,key in list(mixing.items()):
            if not future.done():continue
            try:
                result=future.result();states[key]['complete']=True
                print(json.dumps(dict(completed=key,duration_seconds=result['duration_seconds'])),flush=True)
            except Exception as e:states[key]['errors'].append(dict(stage='mix',error=str(e)))
            del mixing[future]

    pool=ThreadPoolExecutor(max_workers=args.workers)
    mix_pool=ThreadPoolExecutor(max_workers=1)
    futures={}
    try:
        # Interleave cases so shorter meetings become available early and every
        # language starts without waiting behind an entire long meeting.
        rows=[[(key,i) for i in range(len(s['clips']))] for key,s in states.items() if not s.get('complete')]
        for round_rows in zip_longest(*rows):
            for task in round_rows:
                if task is not None:
                    futures[pool.submit(synthesize,*task)]=task
        save()
        failures=0
        for future in as_completed(futures):
            key,index=futures[future]
            try:
                _,_,clip=future.result();states[key]['clips'][index]=clip;done+=1
            except BudgetExceeded:
                print('Budget guard stopped new synthesis; existing reservations remain accounted.',flush=True)
                raise
            except Exception as e:
                states[key]['errors'].append(dict(stage='speech',turn=index,error=str(e)));failures+=1
                if failures>=12:raise RuntimeError('Stopped after twelve synthesis failures; inspect recorded errors before spending more')
            states[key]['remaining']-=1
            if states[key]['remaining']==0:
                folder=out/key
                atomic_json(folder/'generation.json',dict(clips=states[key]['clips'],errors=states[key]['errors']))
                if not states[key]['errors']:
                    mixing[mix_pool.submit(render,states[key]['script'],states[key]['clips'],folder)]=key
            finish_mixes()
            if time.monotonic()-last_save>5:
                save();last_save=time.monotonic()
        pool.shutdown(wait=True)
        mix_pool.shutdown(wait=True);finish_mixes();phase='finished';save()
    except BaseException:
        pool.shutdown(wait=True,cancel_futures=True)
        mix_pool.shutdown(wait=True);finish_mixes();phase='stopped';save()
        raise
    entries=[]
    for key,state in states.items():
        if not state.get('complete'):continue
        s=json.loads((out/key/'reference.json').read_text())
        entries.append(dict(id=key,audio_path=s['audio_path'],languages=s['languages'],text='\n'.join(t['text'] for t in s['turns']),
                            difficulty=s['difficulty'],reference_path=str(out/key/'reference.json')))
    manifest=out/'manifest.jsonl'
    manifest.write_text(''.join(json.dumps(e,ensure_ascii=False)+'\n' for e in entries))
    print(json.dumps(dict(completed=len(entries),requested=len(scripts),manifest=str(manifest),cost=client.ledger.report())),flush=True)

if __name__=='__main__':main()
