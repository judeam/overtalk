#!/usr/bin/env python3
"""Evaluation-only native JSON adapter to a private, continuous llama.cpp server.

Uses the same GGUF and raw prompts as the app. Each HTTP request is independent,
so a completed sequence releases its slot without waiting for its whole batch.
The server must be loopback-only, authenticated and owned by the evaluation.
"""
from concurrent.futures import ThreadPoolExecutor
import json
import math
import os
from pathlib import Path
import secrets
import socket
import subprocess
import sys
import tempfile
import time
from urllib.error import HTTPError, URLError
from urllib.parse import urlsplit
from urllib.request import Request, build_opener, ProxyHandler, HTTPRedirectHandler

try:
    from .vllm_text_helper import MAX_LINE, trim_repeated_line
except ImportError:  # Native clients execute this file directly.
    from vllm_text_helper import MAX_LINE, trim_repeated_line


class NoRedirect(HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        raise ValueError('Private inference must not redirect')


def endpoint(value):
    parsed = urlsplit(value)
    if (parsed.scheme != 'http' or parsed.hostname != '127.0.0.1'
            or not parsed.port or parsed.username or parsed.password
            or parsed.path not in {'', '/'} or parsed.query or parsed.fragment):
        raise ValueError('Expected a private IPv4 loopback server URL')
    return value.rstrip('/')


def finite(value, default, minimum=0.):
    value = float(default if value is None else value)
    return max(minimum, value) if math.isfinite(value) else default


def completion_requests(request, allowed_model, slot_context=8192):
    if request.get('type') not in {'generate', 'generate_batch'}:
        raise ValueError('Unsupported generation request')
    prompts = ([request.get('prompt')] if request['type'] == 'generate'
               else request.get('prompts'))
    if (not isinstance(prompts, list) or not 1 <= len(prompts) <= 64
            or any(not isinstance(p, str) or not p for p in prompts)):
        raise ValueError('Expected one to 64 nonempty prompts')
    if request.get('model_path') is not None and Path(request['model_path']).resolve() != Path(allowed_model).resolve():
        raise ValueError('The server model does not match the native request')
    context = int(request.get('context_size') or 2048)
    limit = int(512 if request.get('max_tokens') is None else request['max_tokens'])
    if not 2048 <= context <= 131072 or not 1 <= limit <= 8192:
        raise ValueError('Invalid context or output limit')
    budget = min(slot_context, context // len(prompts))
    top_p = finite(request.get('top_p'), .95)
    top_p = top_p if 0 < top_p <= 1 else 1.
    repeat = finite(request.get('repeat_penalty'), 1.)
    repeat = repeat if repeat > 0 else 1.
    stops = request.get('stop_tokens') or []
    if not isinstance(stops, list) or any(not isinstance(s, str) or not s for s in stops):
        raise ValueError('Invalid stop tokens')
    last_n = max(0, int(request.get('penalty_last_n') or 0))
    presence = finite(request.get('presence_penalty'), 0.)
    frequency = finite(request.get('frequency_penalty'), 0.)
    if last_n and (presence or frequency or abs(repeat - 1.) > 1e-7):
        # Upstream initializes penalties with prompt tokens; the app penalizes
        # generated tokens only. Do not silently change that sampling policy.
        raise ValueError('Active generation-only penalties require the native helper')
    # Override server defaults explicitly, including samplers absent in the app.
    options = dict(n_predict=limit, temperature=finite(request.get('temperature'), 1.),
                   top_k=max(1, int(request.get('top_k') if request.get('top_k') is not None else 64)),
                   top_p=top_p, min_p=0., typical_p=1., dynatemp_range=0.,
                   repeat_last_n=last_n, repeat_penalty=repeat,
                   presence_penalty=0., frequency_penalty=0.,
                   dry_multiplier=0., xtc_probability=0., mirostat=0,
                   samplers=['penalties', 'top_k', 'top_p', 'temperature'],
                   stop=stops, stream=False, cache_prompt=True)
    return [dict(options, prompt=p) for p in prompts], budget


class ServerClient:
    def __init__(self, address, token, model, slot_context=8192):
        self.address = endpoint(address)
        if not isinstance(token, str) or len(token) < 32:
            raise ValueError('Private inference authentication is required')
        self.token, self.model, self.slot_context = token, str(model), slot_context

    def post(self, route, payload):
        req = Request(self.address + route, data=json.dumps(payload).encode(),
                      headers={'Authorization': 'Bearer ' + self.token,
                               'Content-Type': 'application/json'})
        # No environment proxy or redirects: recordings cannot leave loopback.
        with build_opener(ProxyHandler({}), NoRedirect()).open(req, timeout=600) as response:
            data = response.read(MAX_LINE + 1)
        if len(data) > MAX_LINE:
            raise ValueError('Oversized inference response')
        result = json.loads(data)
        if not isinstance(result, dict) or result.get('error'):
            raise RuntimeError('Private inference returned an error')
        return result

    def generate(self, request):
        jobs, budget = completion_requests(request, self.model, self.slot_context)

        def predict(job):
            tokenized = self.post('/tokenize', dict(content=job['prompt'], add_special=True))
            tokens = tokenized.get('tokens')
            if not isinstance(tokens, list) or len(tokens) + job['n_predict'] > budget:
                raise ValueError('Prompt and output exceed the requested context budget')
            result = self.post('/completion', job)
            text = result.get('content')
            if not isinstance(text, str):
                raise ValueError('Invalid private inference output')
            return trim_repeated_line(text) if request.get('stop_on_repeated_line') else text

        with ThreadPoolExecutor(max_workers=min(16, len(jobs))) as pool:
            texts = list(pool.map(predict, jobs))
        return (dict(type='response', text=texts[0], error=None) if request['type'] == 'generate'
                else dict(type='batch_response', texts=texts, error=None))


class OwnedServer:
    """Local subprocess lifetime and credentials; never reuse an unrelated server."""
    def __init__(self, binary, model, log_path, *, slots=16, slot_context=8192, env=None):
        if not 1 <= slots <= 32 or not 2048 <= slot_context <= 32768:
            raise ValueError('Invalid server resource limits')
        self.model = str(Path(model).resolve())
        if not Path(self.model).is_file() or not Path(binary).is_file():
            raise ValueError('Local server binary and model are required')
        self.directory = tempfile.TemporaryDirectory(prefix='ot-eval-server-')
        self.token = secrets.token_urlsafe(32)
        keyfile = Path(self.directory.name)/'key'
        keyfile.write_text(self.token + '\n')
        keyfile.chmod(0o600)
        with socket.socket() as probe:
            probe.bind(('127.0.0.1', 0))
            port = probe.getsockname()[1]
        self.address = 'http://127.0.0.1:' + str(port)
        self.slot_context = slot_context
        command = [str(Path(binary).resolve()), '-m', self.model, '--host', '127.0.0.1',
                   '--port', str(port), '--api-key-file', str(keyfile), '-ngl', '999',
                   '-c', str(slots * slot_context), '-np', str(slots), '-b', '2048',
                   '-ub', '512', '-t', '4', '-tb', '4', '-fa', 'on', '--cont-batching',
                   '--kv-unified', '--kv-unified-per-slot', str(slot_context),
                   '--cache-ram', '0', '--no-context-shift', '--no-mmproj', '--no-webui']
        self.log = Path(log_path).open('a')
        try:
            self.process = subprocess.Popen(command, env=env, stdin=subprocess.DEVNULL,
                                            stdout=self.log, stderr=self.log)
        except BaseException:
            self.log.close(); self.directory.cleanup()
            raise
        client = ServerClient(self.address, self.token, self.model, slot_context)
        try:
            deadline = time.monotonic() + 120
            while True:
                if self.process.poll() is not None:
                    raise RuntimeError('Private server exited; inspect its log')
                try:
                    # Tokenization verifies both readiness and authentication.
                    client.post('/tokenize', dict(content='ready', add_special=True))
                    break
                except (URLError, HTTPError, OSError):
                    if time.monotonic() >= deadline:
                        raise TimeoutError('Private server did not become ready')
                    time.sleep(.1)
        except BaseException:
            self.close()
            raise

    def close(self):
        if self.process.poll() is None:
            self.process.terminate()
            try:
                self.process.wait(timeout=20)
            except subprocess.TimeoutExpired:
                self.process.kill(); self.process.wait(timeout=10)
        self.log.close()
        self.directory.cleanup()


def main():
    # Native worker processes share a server owned by the outer evaluation.
    client = None
    while True:
        raw = sys.stdin.buffer.readline(MAX_LINE + 1)
        if not raw:
            break
        request = {}
        try:
            if len(raw) > MAX_LINE:
                raise ValueError('Oversized native inference request')
            request = json.loads(raw)
            if not isinstance(request, dict):
                raise ValueError('Expected a JSON object')
            if request.get('type') in {'ping', 'shutdown'}:
                answer = dict(type='pong' if request['type'] == 'ping' else 'goodbye')
            else:
                if client is None:
                    client = ServerClient(os.environ['OVERTALK_EVAL_SERVER'],
                        os.environ['OVERTALK_EVAL_SERVER_TOKEN'], os.environ['OVERTALK_EVAL_SERVER_MODEL'],
                        int(os.environ.get('OVERTALK_EVAL_SERVER_CONTEXT', '8192')))
                answer = client.generate(request)
        except (OSError, ValueError, TypeError, KeyError, RuntimeError) as exc:
            batch = isinstance(request, dict) and request.get('type') == 'generate_batch'
            answer = (dict(type='batch_response', texts=[], error=str(exc)) if batch
                      else dict(type='response', text='', error=str(exc)))
        print(json.dumps(answer, ensure_ascii=False), flush=True)
        if len(raw) > MAX_LINE:
            break  # Never reinterpret the rest of an oversized request as a command.
        if isinstance(request, dict) and request.get('type') == 'shutdown':
            break


if __name__ == '__main__':
    main()
