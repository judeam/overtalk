"""Batch real native notes requests across meetings using one private helper.

Only scheduling and context allocation change. Prompts, sampling, token limits,
source checks, templates and report assembly remain the native application's.
The socket is in an owner-only temporary directory and never listens on TCP.
"""
from collections import deque
from concurrent.futures import ThreadPoolExecutor
import json
import hashlib
import os
from pathlib import Path
import queue
import socket
import subprocess
import tempfile
import threading
import time

from .notes_proxy import MAX_LINE


def prompts(request):
    kind = request.get('type')
    if kind == 'generate':
        values = [request.get('prompt')]
    elif kind == 'generate_batch':
        values = request.get('prompts')
    else:
        raise ValueError('Unsupported shared-helper request')
    if not isinstance(values, list) or not values or any(not isinstance(p, str) for p in values):
        raise ValueError('Invalid generation prompts')
    return values


def compatible(left, right):
    ignored = {'type', 'prompt', 'prompts', 'context_size'}
    return ({k:v for k,v in left.items() if k not in ignored}
            == {k:v for k,v in right.items() if k not in ignored})


def merged_request(requests, context_size):
    base = {k:v for k,v in requests[0].items() if k not in {'type','prompt','prompts','context_size'}}
    if not all(compatible(requests[0], item) for item in requests):
        raise ValueError('Cannot combine incompatible generation settings')
    return dict(base, type='generate_batch', prompts=[p for r in requests for p in prompts(r)],
                context_size=context_size)


def distribute(requests, response):
    values = response.get('texts')
    if response.get('type') != 'batch_response' or response.get('error'):
        raise RuntimeError(response.get('error') or response.get('message') or 'Invalid helper response')
    if not isinstance(values,list) or len(values) != sum(len(prompts(r)) for r in requests):
        raise RuntimeError('Shared helper returned the wrong number of outputs')
    output=[]; offset=0
    for request in requests:
        size=len(prompts(request)); texts=values[offset:offset+size]; offset+=size
        output.append(dict(type='response',text=texts[0],error=None) if request['type']=='generate'
                      else dict(type='batch_response',texts=texts,error=None))
    return output


class SharedHelper:
    def __init__(self, helper, env, log_path, *, batch_size=16, context_size=131072):
        if not 1 <= batch_size <= 16 or not 32768 <= context_size <= 131072:
            raise ValueError('Invalid evaluation helper resource limit')
        self.batch_size, self.context_size = batch_size, context_size
        # Explicit opt-in for hand-authored synthetic regression inputs only.
        capture=env.get('OVERTALK_EVAL_CAPTURE_SYNTHETIC')
        self.capture=Path(capture) if capture else None
        if self.capture is not None:self.capture.mkdir(parents=True,exist_ok=False)
        self.directory=tempfile.TemporaryDirectory(prefix='ot-eval-notes-')
        self.address=str(Path(self.directory.name)/'helper.sock')
        self.queue=queue.Queue(maxsize=64)
        self.stop=threading.Event()
        self.listener=socket.socket(socket.AF_UNIX)
        try:
            self.listener.bind(self.address)
            os.chmod(self.address,0o600)
            self.listener.listen(16)
            self.listener.settimeout(.2)
        except OSError:
            self.listener.close();self.directory.cleanup()
            raise
        self.log=Path(log_path).open('a')
        self.metrics=Path(str(log_path)+'.jsonl').open('a')
        try:
            self.process=subprocess.Popen([str(helper)],env=env,stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,stderr=self.log,text=True,bufsize=1)
        except OSError:
            self.listener.close();self.directory.cleanup();self.log.close();self.metrics.close()
            raise
        self.connections=set()
        self.connection_lock=threading.Lock()
        self.threads=[threading.Thread(target=target,daemon=True) for target in [self.accept,self.work]]
        for thread in self.threads:thread.start()

    def accept(self):
        while not self.stop.is_set():
            try: connection,_=self.listener.accept()
            except socket.timeout:continue
            except OSError:break
            with self.connection_lock:self.connections.add(connection)
            threading.Thread(target=self.client,args=(connection,),daemon=True).start()

    def client(self, connection):
        try:
            with connection,connection.makefile('rwb') as stream:
                while not self.stop.is_set():
                    line=stream.readline(MAX_LINE+1)
                    if not line:break
                    if len(line)>MAX_LINE:raise ValueError('Request too large')
                    request=json.loads(line)
                    if request.get('type') in {'ping','shutdown'}:
                        answer={'type':'pong' if request['type']=='ping' else 'goodbye'}
                    else:
                        prompts(request)
                        job=dict(request=request,ready=threading.Event())
                        self.queue.put(job,timeout=5)
                        if not job['ready'].wait(600):raise TimeoutError('Shared helper timed out')
                        answer=job['answer']
                    stream.write((json.dumps(answer)+'\n').encode());stream.flush()
                    if request.get('type')=='shutdown':break
        except (OSError,ValueError,RuntimeError,queue.Full):
            pass  # The native client observes the disconnect as a failed request.
        finally:
            with self.connection_lock:self.connections.discard(connection)

    def predict(self, jobs):
        requests=[job['request'] for job in jobs]
        request=merged_request(requests,self.context_size)
        started=time.perf_counter()
        try:
            self.process.stdin.write(json.dumps(request)+'\n');self.process.stdin.flush()
            raw=self.process.stdout.readline(MAX_LINE+1)
            if not raw or len(raw)>MAX_LINE:raise RuntimeError('Shared model helper exited')
            answers=distribute(requests,json.loads(raw))
        except (OSError,ValueError,RuntimeError) as error:
            if len(jobs)>1:
                middle=len(jobs)//2
                self.predict(jobs[:middle]);self.predict(jobs[middle:])
                return
            original=requests[0]
            answers=[dict(type='response' if original['type']=='generate' else 'batch_response',
                          text='',texts=[],error=str(error))]
        self.metrics.write(json.dumps(dict(requests=len(jobs),sequences=len(request['prompts']),
            seconds=time.perf_counter()-started,errors=sum(bool(a.get('error')) for a in answers)))+'\n')
        self.metrics.flush()
        for job,answer in zip(jobs,answers):
            if self.capture is not None:
                raw=json.dumps(job['request'],sort_keys=True,separators=(',',':'),ensure_ascii=False)
                name=hashlib.sha256(raw.encode()).hexdigest()
                (self.capture/f'{name}.json').write_text(json.dumps(dict(request=job['request'],response=answer),ensure_ascii=False))
            job['answer']=answer;job['ready'].set()

    def work(self):
        pending=deque()
        while not self.stop.is_set():
            try: first=pending.popleft() if pending else self.queue.get(timeout=.2)
            except queue.Empty:continue
            jobs=[first];count=len(prompts(first['request']))
            deadline=time.monotonic()+.025
            deferred=[]
            while count<self.batch_size:
                try:
                    job=pending.popleft() if pending else self.queue.get(timeout=max(.001,deadline-time.monotonic()))
                except queue.Empty:break
                size=len(prompts(job['request']))
                if compatible(first['request'],job['request']) and count+size<=self.batch_size:
                    jobs.append(job);count+=size
                else:deferred.append(job)
                if time.monotonic()>=deadline:break
            pending.extend(deferred)
            self.predict(jobs)

    def close(self):
        self.stop.set();self.listener.close()
        with self.connection_lock:
            for connection in self.connections:
                try:connection.shutdown(socket.SHUT_RDWR)
                except OSError:pass
        if self.process.poll() is None:
            try:
                self.process.stdin.write('{"type":"shutdown"}\n');self.process.stdin.flush()
                self.process.wait(timeout=20)
            except (OSError,subprocess.TimeoutExpired):
                self.process.terminate();self.process.wait(timeout=10)
        for thread in self.threads:thread.join(timeout=1)
        self.process.stdin.close();self.process.stdout.close()
        self.log.close();self.metrics.close();self.directory.cleanup()


def run_native_notes(native_runner, names, env, output, root, *, workers=4):
    if not names:return
    if not 1<=workers<=8:raise ValueError('Use one to eight native notes workers')
    output=Path(output)
    env=dict(env,TOKIO_WORKER_THREADS='2')
    if workers==1:
        with (output/'notes.log').open('a') as log:
            subprocess.run([str(native_runner),'summary::eval_long_meetings::eval_long_meetings','--exact','--ignored','--nocapture'],
                           env=env,cwd=root,stdout=log,stderr=log,check=True)
        return
    broker=SharedHelper(env['SONEFOLD_LLAMA_HELPER'],env,output/'shared-notes-helper.log')
    shared_env=dict(env,SONEFOLD_LLAMA_HELPER=str(Path(__file__).with_name('notes_proxy.py').resolve()),
                    OVERTALK_EVAL_HELPER_SOCKET=broker.address)
    processes=[]
    process_lock=threading.Lock()
    cancelled=threading.Event()
    def run(index):
        group=names[index::workers]
        if not group:return
        with (output/f'notes-worker-{index}.log').open('a') as log:
            command=[str(native_runner),'summary::eval_long_meetings::eval_long_meetings','--exact','--ignored','--nocapture']
            with process_lock:
                if cancelled.is_set():return
                process=subprocess.Popen(command,
                    env=dict(shared_env,OVERTALK_EVAL_MEETINGS=','.join(group)),cwd=root,
                    stdout=log,stderr=log)
                processes.append(process)
            code=process.wait()
            if code:raise subprocess.CalledProcessError(code,command)
    pool=ThreadPoolExecutor(max_workers=workers)
    try:
        futures=[pool.submit(run,index) for index in range(workers)]
        for future in futures:future.result()
    except BaseException:
        cancelled.set()
        with process_lock:
            for process in processes:
                if process.poll() is None:process.terminate()
        raise
    finally:
        pool.shutdown(wait=True,cancel_futures=True)
        broker.close()
