"""Evaluate completed corpus recordings through an isolated installed app profile.

The real library is never modified. Runs Qwen3-ASR 1.7B + Nemotron-3, and the
production Qwen3.5 4B notes pipeline on both recognized and reference transcripts.
Run with the existing Qwen Python environment, optionally under a GPU-sharing
launcher (finish_run.py uses the one named by OVERTALK_GPU_LAUNCHER, if set).
"""
import argparse
import hashlib
import html
import json
import os
from pathlib import Path
import queue
import sqlite3
import subprocess
import threading
import time
import uuid

from .gemini import atomic_json
from .metrics import transcript_score
from .review_preview import interval_diagnostics

ROOT=Path(__file__).resolve().parents[2]
RUNTIME=ROOT/'evaluations/meetingbench/.runtime'


class NativeEngineExited(RuntimeError):
    """Transport loss invalidates the client, not every subsequent recording."""


def archive_failed_results(runtime):
    """Move only failed attempts out of the active run, preserving all evidence."""
    retried = set()
    for path in sorted((runtime / 'results').glob('*/result.json')):
        result = json.loads(path.read_text())
        if result.get('status') != 'failed':
            continue
        key = path.parent.name
        archive = runtime / 'attempts' / key / str(uuid.uuid4())
        archive.parent.mkdir(parents=True, exist_ok=True)
        # Rename on this filesystem is atomic. Never overwrite a prior attempt.
        path.parent.rename(archive)
        retried.add(key)
    return retried


class NativeClient:
    def __init__(self):
        profile=RUNTIME/'evaluation-profile';data=profile/'data/com.judeam.sonefold'
        data.mkdir(parents=True,exist_ok=True)
        real=Path.home()/'.local/share/com.judeam.sonefold'
        settings=json.loads((real/'speech-runtime.json').read_text())
        if settings['model']!='qwen3-asr-1.7b':raise RuntimeError('Requested ASR is not configured')
        atomic_json(data/'speech-runtime.json',settings)
        models=data/'models/summary';models.mkdir(parents=True,exist_ok=True)
        checkpoint=real/'models/summary/Qwen3.5-4B-Q4_K_M.gguf'
        target=models/checkpoint.name
        if not target.exists():target.symlink_to(checkpoint)
        self.data=data
        pinned=RUNTIME/'evaluation-install.json'
        if not pinned.exists():
            install=(Path.home()/'.local/opt/sonefold/slint/current').resolve()
            atomic_json(pinned,dict(path=str(install),service_sha256=hashlib.sha256((install/'bin/sonefold-service').read_bytes()).hexdigest()))
        self.installation=json.loads(pinned.read_text());self.install=Path(self.installation['path'])
        if hashlib.sha256((self.install/'bin/sonefold-service').read_bytes()).hexdigest()!=self.installation['service_sha256']:
            raise RuntimeError('The pinned evaluation binary changed; refusing to mix app versions in one run')
        env=dict(os.environ,APPDIR=str(self.install),SONEFOLD_LLAMA_HELPER=str(self.install/'bin/llama-helper'),
                 XDG_DATA_HOME=str(profile/'data'),XDG_CONFIG_HOME=str(profile/'config'),XDG_CACHE_HOME=str(profile/'cache'))
        env['LD_LIBRARY_PATH']=str(self.install/'lib')+(os.pathsep+env['LD_LIBRARY_PATH'] if env.get('LD_LIBRARY_PATH') else '')
        env['PATH']=str(self.install/'bin')+os.pathsep+env['PATH']
        env.setdefault('DISPLAY',':1');env.setdefault('DBUS_SESSION_BUS_ADDRESS',f'unix:path=/run/user/{os.getuid()}/bus')
        self.log=(RUNTIME/'evaluation-engine.log').open('a')
        self.process=subprocess.Popen([str(self.install/'bin/sonefold-service'),'--stdio'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=self.log,env=env)
        self.messages=queue.Queue();self.serial=0
        def receive():
            for line in self.process.stdout:
                if line.startswith(b'OTRPC1 '):self.messages.put(json.loads(line[7:]))
            self.messages.put({'engine_exited':self.process.poll()})
        threading.Thread(target=receive,daemon=True).start()
        while True:
            message=self.messages.get(timeout=120)
            if 'engine_exited' in message:raise NativeEngineExited('Isolated native engine failed to start')
            if message.get('kind')=='ready':break
        self.call('set_language_preference',language='auto')
        self.call('api_save_model_config',provider='builtin-ai',model='qwen3.5:4b',whisperModel='base')

    def call(self,command,**args):
        if self.process.poll() is not None:
            raise NativeEngineExited(f'Isolated native engine exited (code {self.process.returncode})')
        self.serial+=1
        try:
            self.process.stdin.write(b'OTRPC1 '+json.dumps(dict(id=self.serial,command=command,args=args)).encode()+b'\n');self.process.stdin.flush()
        except (BrokenPipeError, OSError) as error:
            raise NativeEngineExited('Lost connection to isolated native engine') from error
        while True:
            try:
                reply=self.messages.get(timeout=3600)
            except queue.Empty as error:
                raise NativeEngineExited('Isolated native engine stopped responding') from error
            if 'engine_exited' in reply:raise NativeEngineExited(f'Isolated native engine exited (code {reply["engine_exited"]})')
            if reply.get('id')!=self.serial:continue
            if reply.get('error'):raise RuntimeError(f'{command}: {reply["error"]}')
            return reply['result']

    def close(self):
        try:
            self.process.stdin.write(b'OTRPC1 {"id":999999,"command":"shutdown","args":{}}\n');self.process.stdin.flush()
            self.process.wait(timeout=30)
        except Exception:
            if self.process.poll() is None:
                self.process.terminate()
                try:self.process.wait(timeout=15)
                except subprocess.TimeoutExpired:self.process.kill();self.process.wait()
        self.log.close()

    def save(self,key,rows,audio=None):
        folder=RUNTIME/'evaluation-inputs'/key;folder.mkdir(parents=True,exist_ok=True)
        if audio and not (folder/'audio.wav').exists():(folder/'audio.wav').symlink_to(Path(audio).resolve())
        return self.call('api_save_transcript',meetingTitle=key,folderPath=str(folder),transcripts=rows)['meeting_id']

    def summary(self,meeting,text):
        started=time.perf_counter()
        self.call('api_process_transcript',text=text,model='builtin-ai',modelName='qwen3.5:4b',meetingId=meeting,
                  chunkSize=None,overlap=None,customPrompt=None,templateId='standard_meeting',summaryLanguage='en',authToken=None)
        deadline=time.monotonic()+3600
        while time.monotonic()<deadline:
            result=self.call('api_get_summary',meetingId=meeting)
            if result['status']=='completed':return dict(result,wall_seconds=time.perf_counter()-started)
            if result['status'] in {'error','failed','cancelled'}:raise RuntimeError('Summary failed: '+str(result.get('error')))
            time.sleep(.3)
        raise TimeoutError('Summary exceeded one hour')


def transcript_text(rows):
    return '\n'.join(f'[{int(r.get("audio_start_time",0))//60:02}:{int(r.get("audio_start_time",0))%60:02}] '
                      f'[{r.get("speaker_name") or r.get("speaker_id") or "Unknown speaker"}] {r["text"]}' for r in rows)


def evaluate(client,reference,directory):
    began=time.perf_counter();key=reference['id'];directory.mkdir(parents=True,exist_ok=True)
    actual=client.save(key+'-asr',[],reference['audio_path'])
    asr_started=time.perf_counter()
    analysis=client.call('speech_analyze_meeting',meetingId=actual,jobId='benchmark-'+key,diarize=True,numSpeakers=None)
    asr_wall=time.perf_counter()-asr_started
    rows=client.call('api_get_meeting_transcripts',meetingId=actual,offset=0,limit=10000)['transcripts']
    speakers=client.call('list_meeting_speakers',meetingId=actual)
    with sqlite3.connect(f'file:{client.data}/meeting_minutes.sqlite?mode=ro',uri=True) as db:
        turns=db.execute('SELECT speaker_id,start_time,end_time FROM speaker_turns WHERE meeting_id=? ORDER BY start_time',(actual,)).fetchall()
    atomic_json(directory/'transcript.json',dict(rows=rows,analysis=analysis,speakers=speakers,turns=turns,wall_seconds=asr_wall))
    # Source labels supplied to this control are ground truth, never ASR output.
    people={p['id']:p['name'] for p in reference['people']}
    refrows=[dict(id=t['id'],text=t['text'],audio_start_time=t['start_seconds'],audio_end_time=t['end_seconds'],
                  duration=t['end_seconds']-t['start_seconds'],timestamp='00:00:00',speaker_id=t['speaker'],speaker_name=people[t['speaker']]) for t in reference['turns']]
    ref=client.save(key+'-reference',refrows)
    summaries={}
    for label,meeting,text in [('asr',actual,transcript_text(rows)),('reference',ref,transcript_text(refrows))]:
        path=directory/(label+'-summary.json')
        try:
            summaries[label]=client.summary(meeting,text)
        except NativeEngineExited:
            raise
        except Exception as e:
            summaries[label]=dict(status='failed',error=str(e))
        atomic_json(path,summaries[label])
    expected='\n'.join(t['text'] for t in reference['turns']);hyp='\n'.join(t['text'] for t in rows)
    result=dict(id=key,status='completed',difficulty=reference['difficulty'],languages=reference['languages'],
        duration_seconds=reference['duration_seconds'],participants=len(reference['people']),
        script_sha256=reference['script_sha256'],app_installation=client.installation,models=dict(asr='qwen3-asr-1.7b',diarization='nemotron-3',summary='qwen3.5:4b'),
        transcript=transcript_score(expected,hyp,reference['languages']),
        speakers=dict(true_voices=len(people),detected_voices=len(speakers),passages=len(rows),unassigned=sum(not r.get('speaker_id') for r in rows),
            **interval_diagnostics([(t['speaker'],t['start_seconds'],t['end_seconds']) for t in reference['turns']],turns)),
        timing=dict(asr_and_diarization_seconds=asr_wall,asr_real_time_factor=asr_wall/reference['duration_seconds'],total_seconds=time.perf_counter()-began),
        summary_status={k:v['status'] for k,v in summaries.items()},
        limitations=['Script-relative WER is order-sensitive during simultaneous speech.',
                      'TTS output and dialect authenticity have not been independently validated.',
                      'Speaker timing derives from full TTS clip intervals, not human speech boundaries.',
                      'Shared authored scenario templates limit statistical independence.'])
    atomic_json(directory/'result.json',result);return result


def write_report(results,expected,finished):
    directory=ROOT/'reports/meetingbench';directory.mkdir(parents=True,exist_ok=True)
    groups={}
    for result in results:
        for group in [result.get('difficulty','unknown'),*['language:'+l for l in result.get('languages',[])]]:
            bucket=groups.setdefault(group,dict(cases=0,failed=0,reference_words=0,word_errors=0,reference_characters=0,character_errors=0))
            bucket['cases']+=1;bucket['failed']+=result['status']=='failed'
            metric=result.get('transcript',{})
            for key in ['reference_words','word_errors','reference_characters','character_errors']:
                bucket[key]+=metric.get(key,0)
    for bucket in groups.values():
        bucket['weighted_wer']=bucket['word_errors']/bucket['reference_words'] if bucket['reference_words'] else None
        bucket['weighted_cer']=bucket['character_errors']/bucket['reference_characters'] if bucket['reference_characters'] else None
    data=dict(expected=expected,processed=len(results),missing=expected-len(results),finished=finished,results=results,groups=groups,
              scope='Qwen3-ASR 1.7B + Nemotron-3 and production Qwen3.5 4B notes; synthetic, unvalidated audio references')
    atomic_json(directory/'results.json',data)
    rows=[]
    for r in results:
        wer=r.get('transcript',{}).get('strict_wer'); sp=r.get('speakers',{})
        rows.append('<tr>'+''.join('<td>'+html.escape(str(v))+'</td>' for v in [r['id'],r.get('difficulty',''),', '.join(r.get('languages',[])),
            r['status'],'—' if wer is None else f'{100*wer:.1f}%',f'{sp.get("unassigned","—")} / {sp.get("passages","—")}',
            ', '.join(f'{k}: {v}' for k,v in r.get('summary_status',{}).items()),
            ', '.join(f'{k}: {v.get("diagnostic_band","ungraded")}' for k,v in r.get('summary_review',{}).items()) or 'Ungraded'])+'</tr>')
    document='''<!doctype html><meta charset="utf-8"><title>Overtalk meeting benchmark</title>
<style>body{background:#10161d;color:#e6edf4;font:16px/1.5 system-ui;margin:40px auto;max-width:1500px;padding:0 24px}h1{font-size:30px}p{color:#9daebe}table{width:100%;border-collapse:collapse;background:#18212b}td,th{text-align:left;padding:12px;border-bottom:1px solid #2d3a48}th{color:#76d7cf}a{color:#76d7cf}</style>
<h1>Overtalk meeting benchmark</h1>'''+f'<p>{len(results)} / {expected} cases processed. '+('Finished.' if finished else 'Run in progress.')+'</p>'
    document+='<p>Controlled synthetic scenarios, with shared templates. Raw script-relative word error can penalize valid overlap ordering. Audio, accents and reference speech boundaries have not been independently validated. Summary status means generation succeeded; semantic grading is separate.</p>'
    document+='<table><tr><th>Meeting</th><th>Difficulty</th><th>Languages</th><th>Status</th><th>Raw WER</th><th>Unassigned passages</th><th>Notes</th><th>Same-model diagnostic grade</th></tr>'+''.join(rows)+'</table>'
    (directory/'index.html').write_text(document)


def main():
    parser=argparse.ArgumentParser();parser.add_argument('--render-pid',type=int);parser.add_argument('--limit',type=int,default=200)
    parser.add_argument('--retry-failed', action='store_true', help='Archive failed attempts and retry them with the pinned installation')
    args=parser.parse_args()
    output=RUNTIME/'results';output.mkdir(exist_ok=True)
    if args.retry_failed:
        retried = archive_failed_results(RUNTIME)
        print(json.dumps(dict(retrying=sorted(retried), evidence=str(RUNTIME/'attempts'))), flush=True)
    results=[json.loads(p.read_text()) for p in sorted(output.glob('*/result.json'))]
    complete={r['id'] for r in results};client=None
    try:
        while len(complete)<args.limit:
            available=[]
            for path in (RUNTIME/'corpus').glob('*/reference.json'):
                reference=json.loads(path.read_text())
                if reference['id'] not in complete:available.append(reference)
            available.sort(key=lambda x:x['duration_seconds'])
            if not available:
                progress=json.loads((RUNTIME/'corpus/progress.json').read_text())
                producer_done=progress.get('state') in {'finished','stopped'}
                if args.render_pid:
                    try:
                        command=Path(f'/proc/{args.render_pid}/cmdline').read_bytes()
                        producer_done |= b'evaluations.meetingbench.render_corpus' not in command
                    except FileNotFoundError:producer_done=True
                if producer_done:break
                write_report(results,args.limit,False);time.sleep(5);continue
            if client is None:client=NativeClient()
            for reference in available:
                directory=output/reference['id']
                fatal = None
                try:result=evaluate(client,reference,directory)
                except Exception as e:
                    if isinstance(e, NativeEngineExited):fatal=e
                    result=dict(id=reference['id'],status='failed',difficulty=reference['difficulty'],languages=reference['languages'],error=str(e),
                                transcript=transcript_score('\n'.join(t['text'] for t in reference['turns']),None,reference['languages']))
                    atomic_json(directory/'result.json',result)
                results.append(result);complete.add(reference['id']);write_report(results,args.limit,False)
                print(json.dumps(dict(processed=len(results),meeting=result['id'],status=result['status'])),flush=True)
                if fatal is not None:
                    raise fatal
                if len(complete)>=args.limit:break
    finally:
        if client:client.close()
    write_report(results,args.limit,False)
    if len(complete)<args.limit:print(f'Only {len(complete)} of {args.limit} cases available; missing cases are not counted as successful.',flush=True)

if __name__=='__main__':main()
