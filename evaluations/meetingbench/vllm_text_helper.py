"""Optional evaluation-only Qwen3.5 text sidecar in an isolated vLLM environment.

Implements the native helper's JSON transport for unpenalized extraction and
judging. Sliding-window penalties are rejected, not silently approximated.
Model precision differs from GGUF; compare quality before selecting this engine.
"""
import contextlib
import json
import math
import os
from pathlib import Path
import re
import sys

MAX_LINE = 32 * 1024 * 1024


def requests_and_options(request, allowed_model):
    kind = request.get('type')
    if kind not in {'generate', 'generate_batch'}:
        raise ValueError('Unsupported text helper request')
    values = [request.get('prompt')] if kind == 'generate' else request.get('prompts')
    if (not isinstance(values, list) or not 1 <= len(values) <= 64
            or any(not isinstance(value, str) or not value for value in values)):
        raise ValueError('Expected one to 64 nonempty prompts')
    model = request.get('model_path')
    if model is not None and Path(model).resolve() != Path(allowed_model).resolve():
        raise ValueError('The fixed Qwen3.5 4B model does not match the request')
    temperature = float(request.get('temperature') if request.get('temperature') is not None else 1.)
    temperature = max(0., temperature) if math.isfinite(temperature) else 0.
    top_k = max(1, int(request.get('top_k') if request.get('top_k') is not None else 64))
    top_p = float(request.get('top_p') if request.get('top_p') is not None else .95)
    top_p = top_p if math.isfinite(top_p) and 0 < top_p <= 1 else 1.
    last_n = max(0, int(request.get('penalty_last_n') or 0))
    presence = max(0., float(request.get('presence_penalty') or 0.))
    frequency = max(0., float(request.get('frequency_penalty') or 0.))
    repeat = float(request.get('repeat_penalty') or 1.)
    if last_n and (presence > 0 or frequency > 0 or abs(repeat - 1.) > 1e-7):
        raise ValueError('Sliding-window penalties require the native helper')
    limit = int(request.get('max_tokens') if request.get('max_tokens') is not None else 512)
    context = int(request.get('context_size') or 2048)
    if not 1 <= limit <= 8192 or not 2048 <= context <= 131072:
        raise ValueError('Invalid generation or context budget')
    stops = request.get('stop_tokens') or []
    if not isinstance(stops, list) or any(not isinstance(s, str) or not s for s in stops):
        raise ValueError('Invalid stop tokens')
    return values, dict(temperature=temperature, top_k=top_k, top_p=top_p,
                       max_tokens=limit, stop=stops or None), context // len(values)


def trim_repeated_line(text):
    """Return the prefix preceding the first repeated completed line, as in the helper."""
    seen = set()
    for match in re.finditer('\n', text):
        end = match.start()
        begin = text.rfind('\n', 0, end) + 1
        line = text[begin:end]
        key = ''.join(c for c in line if not c.isascii() or not c.isdigit()).strip()
        if key and key in seen:
            return text[:max(0, begin - 1)].strip()
        if key:
            seen.add(key)
    return text


class TextEngine:
    def __init__(self):
        self.model = None
        self.compat_model = os.environ.get('OVERTALK_VLLM_COMPAT_GGUF', str(
            Path.home()/'.local/share/com.judeam.sonefold/models/summary/Qwen3.5-4B-Q4_K_M.gguf'))

    def load(self):
        if self.model is not None:
            return
        checkpoint = Path(os.environ['OVERTALK_VLLM_TEXT_MODEL']).resolve()
        if not (checkpoint/'config.json').is_file():
            raise ValueError('Local Qwen3.5 model is missing')
        fraction = float(os.environ.get('OVERTALK_VLLM_GPU_FRACTION', '.65'))
        if not .1 <= fraction <= .8:
            raise ValueError('GPU fraction must be between .1 and .8')
        os.environ.update(VLLM_WORKER_MULTIPROC_METHOD='spawn', VLLM_HOST_IP='127.0.0.1',
                          VLLM_NO_USAGE_STATS='1', DO_NOT_TRACK='1', HF_HUB_OFFLINE='1',
                          TRANSFORMERS_OFFLINE='1', HF_HUB_DISABLE_TELEMETRY='1')
        from vllm import LLM
        self.model = LLM(model=str(checkpoint), tokenizer=str(checkpoint), trust_remote_code=False,
                         dtype='bfloat16', language_model_only=True, max_model_len=32768,
                         max_num_seqs=32, max_num_batched_tokens=8192,
                         gpu_memory_utilization=fraction,
                         enable_prefix_caching=True, disable_log_stats=True,
                         compilation_config={'cudagraph_mode': 'PIECEWISE',
                                             'max_cudagraph_capture_size': 32,
                                             'cudagraph_capture_sizes': [1,2,4,8,16,24,32]})

    def generate(self, request):
        prompts, options, per_sequence = requests_and_options(request, self.compat_model)
        self.load()
        from vllm import SamplingParams
        tokenizer = self.model.get_tokenizer()
        for prompt in prompts:
            if len(tokenizer.encode(prompt)) + options['max_tokens'] > min(per_sequence, 32768):
                raise ValueError('Prompt and output exceed the requested context budget')
        results = self.model.generate(prompts, SamplingParams(**options), use_tqdm=False)
        texts = [result.outputs[0].text for result in results]
        if request.get('stop_on_repeated_line'):
            texts = [trim_repeated_line(text) for text in texts]
        return (dict(type='response', text=texts[0], error=None) if request['type'] == 'generate'
                else dict(type='batch_response', texts=texts, error=None))

    def close(self):
        if self.model is not None:
            core = getattr(getattr(self.model, 'llm_engine', None), 'engine_core', None)
            if core is not None:
                core.shutdown()
            self.model = None


def main():
    # Keep framework/child logs off the JSON channel, including writes directly
    # to file descriptor 1 by spawned workers. Only this process owns replies.
    protocol = os.fdopen(os.dup(sys.stdout.fileno()), 'w', buffering=1)
    os.dup2(sys.stderr.fileno(), sys.stdout.fileno())
    engine = TextEngine()
    try:
        while True:
            raw = sys.stdin.buffer.readline(MAX_LINE + 1)
            if not raw:
                break
            if len(raw) > MAX_LINE:
                raise ValueError('Request exceeds the text helper transport limit')
            request = {}
            try:
                request = json.loads(raw)
                if not isinstance(request, dict):
                    raise ValueError('Expected a JSON object')
                if request.get('type') == 'ping':
                    answer = dict(type='pong')
                elif request.get('type') == 'shutdown':
                    answer = dict(type='goodbye')
                else:
                    with contextlib.redirect_stdout(sys.stderr):
                        answer = engine.generate(request)
            except (ValueError, TypeError, KeyError, RuntimeError) as exc:
                kind = request.get('type') if isinstance(request, dict) else None
                answer = (dict(type='batch_response', texts=[], error=str(exc)) if kind == 'generate_batch'
                          else dict(type='response', text='', error=str(exc)))
            protocol.write(json.dumps(answer, ensure_ascii=False) + '\n')
            if isinstance(request, dict) and request.get('type') == 'shutdown':
                break
    finally:
        engine.close()
        protocol.close()


if __name__ == '__main__':
    main()
