"""Run frozen synthetic cases through the native Privatemode notes pipeline.

Only a confirmed Free-plan account is allowed. The loopback forwarding server
adds the key, counts usage and enforces a conservative token ceiling before
each request. It does not grade notes or read expected answers into prompts.
"""
import argparse
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import shutil
import stat
import subprocess
import threading
import time
from urllib.error import HTTPError
from urllib.parse import urlsplit
from urllib.request import Request, ProxyHandler, HTTPRedirectHandler, build_opener

MODELS = ('glm-5.3', 'gpt-oss-120b', 'glm-5.3-flash')
ROOT = Path(__file__).resolve().parents[2]


def digest(path):
    with Path(path).open('rb') as source:
        return hashlib.file_digest(source, 'sha256').hexdigest()


def local_endpoint(value):
    url = urlsplit(value)
    if (url.scheme not in ('http', 'https') or url.hostname not in ('127.0.0.1', '::1', 'localhost')
            or url.username is not None or url.password is not None or url.query or url.fragment
            or url.path.rstrip('/') not in ('', '/v1')):
        raise ValueError('Use the official proxy on loopback, without credentials in its URL')
    host = '[::1]' if url.hostname == '::1' else '127.0.0.1'
    return f'{url.scheme}://{host}' + (f':{url.port}' if url.port else '') + '/v1'


def read_key(path):
    path = Path(path)
    info = path.lstat()
    if not stat.S_ISREG(info.st_mode) or path.is_symlink():
        raise ValueError('Key must be a regular file, not a symlink')
    if os.name == 'posix' and (info.st_uid != os.getuid() or stat.S_IMODE(info.st_mode) & 0o077):
        raise ValueError('Key file must belong to you and have mode 600 or stricter')
    key = path.read_text().strip()
    if not key or len(key) > 4096 or any(c.isspace() for c in key):
        raise ValueError('Key file is empty or invalid')
    return key


class NoRedirect(HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


class Budget:
    def __init__(self, limit):
        self.limit, self.used = limit, 0
        self.lock = threading.Lock()
        self.stopped = False

    def reserve(self, body):
        model = body.get('model')
        if model not in MODELS:
            raise ValueError('Unapproved comparison model')
        output = body.get('max_completion_tokens')
        if type(output) is not int or not 1 <= output <= 16384:
            raise ValueError('Every request must cap completion plus reasoning tokens')
        # UTF-8 bytes bound ordinary tokenizers conservatively; reserve chat
        # framing and all output. gpt-oss counts twice against the free quota.
        multiplier = 2 if model == 'gpt-oss-120b' else 1
        amount = (len(json.dumps(body, ensure_ascii=False).encode()) + output + 1024) * multiplier
        with self.lock:
            if self.stopped or self.used + amount > self.limit:
                self.stopped = True
                raise ValueError('Comparison token allowance exhausted; no request sent')
            self.used += amount
        return amount, multiplier

    def settle(self, reserved, multiplier, usage):
        prompt, completion = usage.get('prompt_tokens'), usage.get('completion_tokens')
        if any(type(n) is not int or n < 0 for n in (prompt, completion)):
            return
        actual = (prompt + completion) * multiplier
        with self.lock:
            self.used += actual - reserved
            self.stopped |= self.used >= self.limit


class Gateway:
    def __init__(self, endpoint, key, output, limit, *, capture_dir=None):
        self.endpoint, self.key, self.output = local_endpoint(endpoint), key, Path(output)
        self.budget = Budget(limit)
        self.opener = build_opener(ProxyHandler({}), NoRedirect())
        self.records = []
        self.capture_dir = Path(capture_dir) if capture_dir is not None else None
        if self.capture_dir is not None:
            self.capture_dir.mkdir(parents=True, exist_ok=False)
        gateway = self

        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *args):
                pass  # Never emit request bodies or authorization headers.

            def do_POST(self):
                reserved = None
                started = time.perf_counter()
                body = {}
                try:
                    if self.path != '/v1/chat/completions':
                        raise ValueError('Unsupported comparison endpoint')
                    size = int(self.headers.get('Content-Length', '0'))
                    if not 0 < size <= 8 * 1024 * 1024:
                        raise ValueError('Invalid request length')
                    raw = self.rfile.read(size)
                    body = json.loads(raw)
                    reserved, multiplier = gateway.budget.reserve(body)
                    request = Request(gateway.endpoint + '/chat/completions', data=raw,
                        headers={'Content-Type': 'application/json', 'Authorization': 'Bearer ' + gateway.key})
                    try:
                        with gateway.opener.open(request, timeout=290) as response:
                            status = response.status
                            result = response.read(16 * 1024 * 1024 + 1)
                    except HTTPError as error:
                        status = error.code
                        # Errors can echo keys/prompts. Store only the status.
                        result = json.dumps({'error': f'Privatemode returned HTTP {status}'}).encode()
                    if len(result) > 16 * 1024 * 1024:
                        raise ValueError('Provider response is too large')
                    parsed = json.loads(result)
                    usage = parsed.get('usage', {})
                    if 200 <= status < 300:
                        gateway.budget.settle(reserved, multiplier, usage)
                        # Opt-in only for this authored synthetic challenge.
                        # Headers (including Authorization) are never captured.
                        if gateway.capture_dir is not None:
                            request_hash=hashlib.sha256(raw).hexdigest()
                            (gateway.capture_dir/f'{request_hash}.json').write_text(json.dumps({
                                'request':body,'response':parsed},ensure_ascii=False))
                    else:
                        gateway.budget.stopped = True
                    gateway.records.append(dict(model=body['model'], status=status,
                        request_sha256=hashlib.sha256(raw).hexdigest(),
                        seconds=time.perf_counter()-started, usage=usage,
                        completion_sha256=hashlib.sha256(result).hexdigest(),
                        finish_reasons=[c.get('finish_reason') for c in parsed.get('choices', [])],
                        quota_used_or_reserved=gateway.budget.used))
                except Exception:
                    gateway.budget.stopped = True
                    status = 503
                    result = b'{"error":"Comparison stopped: budget, connection, or response validation failed. No automatic retry."}'
                    gateway.records.append(dict(model=body.get('model'), status=status,
                        seconds=time.perf_counter()-started, quota_used_or_reserved=gateway.budget.used))
                gateway.output.write_text(json.dumps(gateway.records, indent=2))
                self.send_response(status)
                self.send_header('Content-Type', 'application/json')
                self.send_header('Content-Length', str(len(result)))
                self.end_headers()
                self.wfile.write(result)

        self.server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)

    def __enter__(self):
        self.thread.start()
        return self

    def __exit__(self, *args):
        self.server.shutdown(); self.server.server_close(); self.thread.join()
        self.key = ''


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--inputs', type=Path, required=True)
    parser.add_argument('--baseline', type=Path, required=True)
    parser.add_argument('--native-runner', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--cases', nargs='+', required=True, help='Frozen input IDs including -asr or -reference')
    parser.add_argument('--models', nargs='+', choices=MODELS, default=list(MODELS[:2]))
    parser.add_argument('--key-file', type=Path, required=True)
    parser.add_argument('--endpoint', default='http://127.0.0.1:8080/v1')
    parser.add_argument('--quota-tokens', type=int, default=500_000)
    parser.add_argument('--max-completion-tokens', type=int, default=8192)
    parser.add_argument('--confirmed-free-plan', action='store_true', help='Required: verify Free plan in portal, pay-as-you-go disabled')
    parser.add_argument('--prepare-only', action='store_true', help='Freeze a comparison plan without keys or network calls')
    args = parser.parse_args()
    if not args.prepare_only and not args.confirmed_free_plan:
        parser.error('Live tests require confirmed Free plan; an API key alone does not establish free-only billing')
    if args.output.exists():
        parser.error('Use a new output directory; frozen runs are never overwritten')
    if not 1 <= args.quota_tokens <= 1_000_000 or not 1 <= args.max_completion_tokens <= 16384:
        parser.error('Invalid token ceiling')
    if len(args.cases) != len(set(args.cases)) or any(not c or any(ch not in 'abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_-' for ch in c) for c in args.cases):
        parser.error('Cases must be unique safe file identifiers')
    files = []
    for case in args.cases:
        path = args.inputs / f'meeting-{case}.json'
        baseline = args.baseline / f'{case}__qwen3.5-4b__app.md'
        value = json.loads(path.read_text())
        if set(value) != {'transcript', 'language'} or any(not isinstance(v, str) for v in value.values()):
            parser.error('Inputs must contain only transcript and language')
        if not baseline.is_file():
            parser.error(f'Missing frozen baseline for {case}')
        files.append((case, path, baseline))
    endpoint = local_endpoint(args.endpoint)
    key = None if args.prepare_only else read_key(args.key_file)
    output = args.output.resolve()
    (output/'inputs').mkdir(parents=True)
    (output/'baseline').mkdir()
    (output/'notes').mkdir()
    for case, path, baseline in files:
        shutil.copyfile(path, output/'inputs'/path.name)
        shutil.copyfile(baseline, output/'baseline'/baseline.name)
    config = dict(models=args.models, cases=args.cases, endpoint=endpoint,
        native_sha256=digest(args.native_runner), quota_tokens=args.quota_tokens,
        max_completion_tokens=args.max_completion_tokens, free_plan_confirmed=args.confirmed_free_plan,
        inputs={case:digest(path) for case,path,_ in files},
        baseline={case:digest(path) for case,_,path in files},
        reference_answers_supplied=False, grading='pending direct assistant review',
        prepare_only=args.prepare_only)
    (output/'configuration.json').write_text(json.dumps(config, indent=2))
    if args.prepare_only:
        print(json.dumps(dict(prepared=len(files), models=args.models, inference_requests=0)))
        return
    began = time.perf_counter()
    with Gateway(endpoint, key, output/'usage.json', args.quota_tokens,
                 capture_dir=output/'synthetic-response-captures' if os.environ.get('OVERTALK_EVAL_CAPTURE_SYNTHETIC') == '1' else None) as gateway:
        env = dict(os.environ, OVERTALK_EVAL_APP_DIR=str(output),
            OVERTALK_EVAL_INPUT_DIR=str(output/'inputs'), OVERTALK_EVAL_OUT=str(output/'notes'),
            OVERTALK_EVAL_PROVIDER='privatemode', OVERTALK_EVAL_FREE_PLAN_CONFIRMED='1',
            OVERTALK_EVAL_PRIVATEMODE_ENDPOINT=f'http://127.0.0.1:{gateway.server.server_port}/v1',
            OVERTALK_EVAL_MAX_COMPLETION_TOKENS=str(args.max_completion_tokens),
            OVERTALK_EVAL_MODES='app', OVERTALK_EVAL_TOPIC_PLAN='0', OVERTALK_EVAL_QUOTED_DECISIONS='0',
            TOKIO_WORKER_THREADS='2')
        for unwanted in ['OVERTALK_EVAL_READING_WINDOW', 'OVERTALK_EVAL_DEBUG_DIR']:
            env.pop(unwanted, None)
        # One native process and one meeting/model at a time respect free-tier
        # limits. Never start a GPU model or an automated grader.
        for case in args.cases:
            for model in args.models:
                if gateway.budget.stopped:
                    break
                with (output/'native.log').open('a') as log:
                    subprocess.run([str(args.native_runner.resolve()), 'summary::eval_long_meetings::eval_long_meetings', '--exact', '--ignored', '--nocapture'],
                        env=dict(env, OVERTALK_EVAL_MEETINGS=case, OVERTALK_EVAL_MODELS=model),
                        cwd=ROOT, stdout=log, stderr=log, check=True, timeout=900)
        usage = gateway.budget.used
    notes = list((output/'notes').glob('*.md'))
    result = dict(completed=len(notes), expected=len(args.cases)*len(args.models),
        failed=[p.name for p in notes if p.read_text().startswith('ERROR:')],
        seconds=time.perf_counter()-began, quota_used_or_reserved=usage, grading='pending direct assistant review')
    (output/'result.json').write_text(json.dumps(result, indent=2))
    print(json.dumps(result))


if __name__ == '__main__':
    main()
