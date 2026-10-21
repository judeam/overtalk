"""Batch inference while replaying the real ASR adapter's timing/retry logic.

Only acoustically predicted turns may guide decoding. No script, speaker name or
reference text is accepted. This evaluation-only path preserves 30-second audio
windows, full audio coverage, alignment and the adapter's repetition retries.
"""
import hashlib
import math
import os
from pathlib import Path
import sys
import time

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'speech-worker'))
from sonefold_speech.asr import (ASREngine, ALIGNER_LANGUAGES, QWEN_LANGUAGES, SAMPLE_RATE, audio_windows,
                                has_repetition_loop, parse_words, restore_aligned_text,
                                split_speaker_window)
from sonefold_speech.diarization import speaker_change_boundaries
from .batch_asr import bounded_inference


def audio_key(samples):
    return hashlib.sha256(memoryview(samples).cast('B')).digest()


class CachedModel:
    def __init__(self, model, cache, counters):
        self.model, self.cache, self.counters = model, cache, counters

    def transcribe(self, *, audio, language=None, context='', **kwargs):
        samples, rate = audio
        key = (audio_key(samples), language, context)
        if rate == SAMPLE_RATE and key in self.cache:
            self.counters['recognition_cache_hits'] += 1
            return [self.cache[key]]
        self.counters['recognition_cache_misses'] += 1
        return self.model.transcribe(audio=audio, language=language, context=context, **kwargs)

    def __getattr__(self, name):
        return getattr(self.model, name)


class CachedAligner:
    def __init__(self, aligner, cache, counters):
        self.aligner, self.cache, self.counters = aligner, cache, counters

    def align(self, *, audio, text, language):
        key = (audio_key(audio[0]), text, language)
        if key in self.cache:
            self.counters['alignment_cache_hits'] += 1
            return [self.cache[key]]
        self.counters['alignment_cache_misses'] += 1
        return self.aligner.align(audio=audio, text=text, language=language)


class ThroughputEngine(ASREngine):
    def __init__(self, *, backend='transformers', batch_size=32, gpu_fraction=.4, eager=False, progress=None,
                 initial_decode_tokens=256):
        super().__init__('cuda')
        if backend not in {'transformers', 'vllm'} or not 1 <= batch_size <= 64:
            raise ValueError('Invalid throughput backend or batch size')
        if not .1 <= gpu_fraction <= .8:
            raise ValueError('GPU fraction must be between .1 and .8')
        self.backend, self.batch_size = backend, batch_size
        self.gpu_fraction, self.eager = gpu_fraction, eager
        self.progress = progress
        if not 128 <= initial_decode_tokens <= 1024:
            raise ValueError('Initial decoding budget must be 128 to 1024 tokens')
        self.initial_decode_tokens = initial_decode_tokens

    def _runtime(self):
        runtime = super()._runtime()
        # Token sampling and feature preparation are small CPU operations around
        # GPU kernels. Large OpenMP pools cost more synchronization than work.
        if runtime.get_num_threads() > 4:
            runtime.set_num_threads(4)
        return runtime

    def unload(self):
        if getattr(self, 'backend', None) == 'vllm' and self._model is not None:
            engine = getattr(getattr(self._model, 'model', None), 'llm_engine', None)
            core = getattr(engine, 'engine_core', None)
            if core is not None:
                core.shutdown()
        super().unload()

    def _load(self, name):
        if name == self._model_name:
            return self._model
        if self.backend == 'transformers':
            result = super()._load(name)
            result.max_inference_batch_size = self.batch_size
            return result
        if name != 'qwen3-asr-1.7b':
            raise ValueError('The throughput experiment retains Qwen3-ASR 1.7B')
        self.unload()
        self._runtime()
        from qwen_asr import Qwen3ASRModel
        from .vllm_setup import local_engine_options
        checkpoint = os.environ['SONEFOLD_QWEN3_ASR_1_7B_DIR']
        self._model = Qwen3ASRModel.LLM(
            gpu_memory_utilization=self.gpu_fraction, max_inference_batch_size=self.batch_size,
            max_new_tokens=1024, max_model_len=2048, max_num_seqs=self.batch_size,
            enforce_eager=self.eager, disable_log_stats=True,**local_engine_options(checkpoint,eager=self.eager))
        self._model_name = name
        return self._model

    def transcribe_batched(self, audio_path, *, speaker_turns=None):
        began = time.perf_counter()
        model = self._load('qwen3-asr-1.7b')
        loading_seconds = time.perf_counter() - began
        counters = dict(recognition_cache_hits=0, recognition_cache_misses=0,
                        alignment_cache_hits=0, alignment_cache_misses=0,
                        recognition_batches=0, alignment_batches=0,
                        recognition_seconds=0., alignment_seconds=0., full_decode_retries=0,
                        avoided_full_windows=0)
        recognition, alignment = {}, {}
        boundaries = speaker_change_boundaries(speaker_turns or [])
        aligner = None
        observed_token_rate = [0.]
        initial_probe_done = [False]
        prefer_guided = [False]
        guided_from = [math.inf]

        def infer(requests):
            fresh, seen = [], set()
            for samples, offset, duration in requests:
                key = (audio_key(samples), None, '')
                if key not in recognition and key not in seen:
                    fresh.append((samples, offset, duration, key))
                    seen.add(key)
            # Similar durations share a batch, so a long turn does not pad
            # dozens of short turns. Replay still uses original source order.
            fresh.sort(key=lambda item:len(item[0]))
            tokenizer = getattr(getattr(model, 'processor', None), 'tokenizer', None)
            if tokenizer is not None and not initial_probe_done[0] and len(fresh)>1:
                # Learn token density from this audio, never from language labels
                # in the reference. Indic scripts can need much more than 256
                # tokens per window; avoid retrying an entire truncated batch.
                initial_probe_done[0]=True
                infer([fresh[0][:3]])
                fresh=[item for item in fresh if item[3] not in recognition]
            for start in range(0, len(fresh), self.batch_size):
                group = fresh[start:start+self.batch_size]
                clock = time.perf_counter()
                budget = self.initial_decode_tokens if tokenizer is not None else 1024
                if observed_token_rate[0] and group:
                    estimated=math.ceil((max(item[2] for item in group)*observed_token_rate[0]*1.3+32)/64)*64
                    budget=min(1024,max(budget,estimated))

                def decode(items, tokens):
                    previous = getattr(model, 'max_new_tokens', 1024)
                    params = getattr(model, 'sampling_params', None)
                    previous_param = getattr(params, 'max_tokens', None)
                    model.max_new_tokens = tokens
                    if params is not None: params.max_tokens = tokens
                    try:
                        return bounded_inference(lambda batch: model.transcribe(
                            audio=[(item[0], SAMPLE_RATE) for item in batch], language=None, return_time_stamps=False),
                            items, self.batch_size, self._torch.cuda.empty_cache)
                    finally:
                        model.max_new_tokens = previous
                        if params is not None: params.max_tokens = previous_param

                outputs = decode(group, budget)
                longer = [index for index,(result,error) in enumerate(outputs) if not error
                          and needs_full_decode(result, tokenizer, budget)]
                if longer:
                    retries=decode([group[index] for index in longer],1024)
                    for index,prediction in zip(longer,retries):outputs[index]=prediction
                    counters['full_decode_retries'] += len(longer)
                counters['recognition_seconds'] += time.perf_counter() - clock
                counters['recognition_batches'] += 1
                if self.progress:self.progress(dict(counters,phase='recognition'))
                for item, (result, error) in zip(group, outputs):
                    if not error:
                        recognition[item[3]] = result
                        if tokenizer is not None and not has_repetition_loop(result.text):
                            observed_token_rate[0]=max(observed_token_rate[0],
                                len(tokenizer.encode(result.text,add_special_tokens=False))/item[2])

        def align(requests):
            nonlocal aligner
            eligible = []
            for samples, offset, duration in requests:
                result = recognition.get((audio_key(samples), None, ''))
                if (result and result.text.strip() and result.language in ALIGNER_LANGUAGES
                        and not has_repetition_loop(result.text)):
                    key = (audio_key(samples), result.text.strip(), result.language)
                    if key not in alignment:
                        eligible.append((samples, offset, duration, result, key))
            if not eligible:
                return
            eligible.sort(key=lambda item:len(item[0]))
            aligner = self._load_aligner()
            if aligner is None:
                return
            for start in range(0, len(eligible), self.batch_size):
                group = eligible[start:start+self.batch_size]
                clock = time.perf_counter()
                outputs = bounded_inference(lambda items: aligner.align(
                    audio=[(item[0], SAMPLE_RATE) for item in items],
                    text=[item[3].text.strip() for item in items], language=[item[3].language for item in items]),
                    group, self.batch_size, self._torch.cuda.empty_cache)
                counters['alignment_seconds'] += time.perf_counter() - clock
                counters['alignment_batches'] += 1
                if self.progress:self.progress(dict(counters,phase='alignment'))
                for item, (result, error) in zip(group, outputs):
                    if not error:
                        alignment[item[4]] = result

        def prepare(pending):
            seen_requests = set()
            while pending:
                infer(pending)
                align(pending)
                following = []
                for samples, offset, duration in pending:
                    signature = (audio_key(samples), offset, duration)
                    if signature in seen_requests:
                        continue
                    seen_requests.add(signature)
                    result = recognition.get((signature[0], None, ''))
                    if result is None:
                        continue  # The real adapter retries failed prefetch work.
                    text = result.text.strip()
                    if has_repetition_loop(text) and duration > 5:
                        middle = len(samples) // 2
                        split = duration * middle / len(samples)
                        following.extend([(samples[:middle], offset, split),
                                          (samples[middle:], offset+split, duration-split)])
                    elif text and boundaries:
                        aligned = alignment.get((signature[0], text, result.language))
                        try:
                            usable = bool(aligned and restore_aligned_text(parse_words(
                                aligned, offset, duration, coalesce_zero_duration=True), text))
                        except (RuntimeError, ValueError):
                            usable = False
                        if not usable:
                            prefer_guided[0] = True
                            guided_from[0] = min(guided_from[0], offset)
                # The real adapter keeps speaker-window mode for every later
                # window after the first alignment failure. A prefetched group
                # can cross that transition, including otherwise alignable
                # later windows. Prepare those same real-audio partitions too.
                for samples, offset, duration in pending:
                    prediction = recognition.get((audio_key(samples), None, ''))
                    if offset >= guided_from[0] and prediction and not has_repetition_loop(prediction.text):
                        parts = split_speaker_window(samples, offset, duration, boundaries)
                        if len(parts) > 1:
                            following.extend(parts)
                unique = {(audio_key(item[0]), item[1], item[2]): item for item in following}
                pending = [item for key, item in unique.items() if key not in seen_requests]

        windows = iter(audio_windows(Path(audio_path)))
        primed = False
        while True:
            roots = []
            for _ in range(self.batch_size):
                item = next(windows, None)
                if item is None:break
                if item[0].any():roots.append(item)
            if not roots:
                if item is None:break
                continue
            if boundaries and not primed:
                # The production adapter learns once whether it needs speaker
                # windows. Do that before prefetching a whole meeting; otherwise
                # every subsequently unused full window is decoded a second time.
                prepare([roots.pop(0)])
                primed = True
                initial_probe_done[0] = True
            pending = []
            for samples,offset,duration in roots:
                parts=split_speaker_window(samples,offset,duration,boundaries) if prefer_guided[0] else [(samples,offset,duration)]
                if len(parts)>1:counters['avoided_full_windows']+=1
                pending.extend(parts)
            prepare(pending)
        # Replay the production adapter instead of reimplementing its transcript
        # construction. Cache misses preserve the original inference fallback.
        self._model = CachedModel(model, recognition, counters)
        original_aligner = self._aligner
        if original_aligner is not None:
            self._aligner = CachedAligner(original_aligner, alignment, counters)
        replay = time.perf_counter()
        try:
            result = super().transcribe(audio_path, 'qwen3-asr-1.7b', speaker_turns=speaker_turns)
        finally:
            self._model, self._aligner = model, original_aligner
        counters.update(loading_seconds=loading_seconds, replay_seconds=time.perf_counter()-replay,
                        total_seconds=time.perf_counter()-began,
                        cache_entries=len(recognition), backend=self.backend, batch_size=self.batch_size)
        if hasattr(self._torch, 'get_num_threads'):
            counters['cpu_threads'] = self._torch.get_num_threads()
        return result, counters


def needs_full_decode(result, tokenizer, budget):
    """Conservative cap detection; never accept a potentially clipped passage."""
    if budget >= 1024 or tokenizer is None:
        return False
    # A repetition loop is re-read from its full audio as shorter real segments
    # by the unchanged adapter. Extending the loop to 1024 tokens adds no speech.
    if has_repetition_loop(result.text):
        return False
    if result.language not in QWEN_LANGUAGES.values():
        return True
    return len(tokenizer.encode(result.text,add_special_tokens=False)) >= budget - 32
