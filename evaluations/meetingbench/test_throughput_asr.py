import sys
from pathlib import Path
from types import SimpleNamespace
import unittest
import tempfile
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / 'speech-worker'))
from .throughput_asr import CachedModel, CachedAligner, ThroughputEngine, audio_key, needs_full_decode


class ThroughputCacheTests(unittest.TestCase):
    def test_vllm_workers_are_released_before_the_next_model_stage(self):
        closed=[]
        engine=ThroughputEngine(backend='vllm')
        engine._model=SimpleNamespace(model=SimpleNamespace(llm_engine=SimpleNamespace(
            engine_core=SimpleNamespace(shutdown=lambda:closed.append(True)))))
        engine.unload()
        self.assertEqual(closed,[True])
        self.assertIsNone(engine._model)

    def test_gpu_runtime_bounds_cpu_threads_without_raising_a_lower_budget(self):
        for threads, expected in [(24,4),(2,2)]:
            configured=[]
            runtime=SimpleNamespace(get_num_threads=lambda:threads,set_num_threads=configured.append)
            engine=ThroughputEngine()
            with patch('sonefold_speech.asr.ASREngine._runtime',return_value=runtime):
                self.assertIs(engine._runtime(),runtime)
            self.assertEqual(configured,[expected] if threads>4 else [])

    def test_potentially_truncated_decoding_requires_full_budget(self):
        tokenizer=SimpleNamespace(encode=lambda text,**kwargs:list(text))
        self.assertTrue(needs_full_decode(SimpleNamespace(text='a'*224,language='English'),tokenizer,256))
        self.assertFalse(needs_full_decode(SimpleNamespace(text='normal sentence',language='English'),tokenizer,256))
        self.assertTrue(needs_full_decode(SimpleNamespace(text='short',language='unknown'),tokenizer,256))
        self.assertFalse(needs_full_decode(SimpleNamespace(text='one two three four '*8,language='English'),tokenizer,256))

    def samples(self):
        import numpy as np
        return np.asarray([0., .1, .2], dtype='float32')

    def test_cached_recognition_keeps_text_and_falls_back_for_different_context(self):
        samples = self.samples()
        expected = SimpleNamespace(text='exact original', language='English')
        calls = []
        model = SimpleNamespace(transcribe=lambda **kw: calls.append(kw) or ['fallback'])
        counters = dict(recognition_cache_hits=0, recognition_cache_misses=0)
        cached = CachedModel(model, {(audio_key(samples), None, ''): expected}, counters)
        self.assertIs(cached.transcribe(audio=(samples, 16000))[0], expected)
        self.assertEqual(cached.transcribe(audio=(samples, 16000), context='different'), ['fallback'])
        self.assertEqual(len(calls), 1)

    def test_cached_alignment_uses_exact_audio_text_and_language(self):
        samples = self.samples()
        original = [{'text':'hello', 'start_time':0., 'end_time':.1}]
        counters = dict(alignment_cache_hits=0, alignment_cache_misses=0)
        aligner = SimpleNamespace(align=lambda **kw: ['fallback'])
        cached = CachedAligner(aligner, {(audio_key(samples), 'hello', 'English'): original}, counters)
        self.assertEqual(cached.align(audio=(samples,16000),text='hello',language='English'), [original])
        self.assertEqual(cached.align(audio=(samples,16000),text='different',language='English'), ['fallback'])

    def run_fake(self, windows, predict, turns=None, batch_size=2, aligner=None):
        calls=[]
        class Model:
            def transcribe(self, *, audio, **kwargs):
                inputs=audio if isinstance(audio,list) else [audio]
                calls.append(len(inputs))
                return [predict(item[0]) for item in inputs]
        class Engine(ThroughputEngine):
            def _load(self, name):
                if self._model is None:self._model=Model()
                return self._model
            def _load_aligner(self):
                self._aligner=aligner
                return aligner
        engine=Engine(batch_size=batch_size)
        engine._torch=SimpleNamespace(cuda=SimpleNamespace(empty_cache=lambda:None))
        with tempfile.NamedTemporaryFile(suffix='.wav') as audio, \
             patch('evaluations.meetingbench.throughput_asr.audio_windows',side_effect=lambda path:iter(windows)), \
             patch('sonefold_speech.asr.audio_windows',side_effect=lambda path:iter(windows)):
            result,timing=engine.transcribe_batched(audio.name,speaker_turns=turns)
        return result,timing,calls

    def test_silence_batch_does_not_drop_later_audio_and_replay_uses_cached_output(self):
        import numpy as np
        windows=[(np.zeros(8,dtype='float32'),0.,30.),(np.zeros(8,dtype='float32'),30.,30.),
                 (np.ones(8,dtype='float32'),60.,30.),(np.full(8,2,dtype='float32'),90.,30.)]
        result,timing,calls=self.run_fake(windows,lambda samples:SimpleNamespace(
            text='one' if samples[0]==1 else 'two',language='English'))
        self.assertEqual(result['text'],'one\ntwo')
        self.assertEqual(result['audio_seconds'],120.)
        self.assertEqual(calls,[2])
        self.assertEqual(timing['recognition_cache_hits'],2)
        self.assertEqual(timing['recognition_cache_misses'],0)

    def test_repetition_retries_keep_every_sample_and_hit_prefetched_children(self):
        import numpy as np
        samples=np.arange(8,dtype='float32')
        result,timing,_calls=self.run_fake([(samples,0.,6.)],lambda sample:SimpleNamespace(
            text=('one two three four '*8) if len(sample)>4 else 'clear speech',language='English'))
        self.assertEqual(result['text'],'clear speech\nclear speech')
        self.assertEqual([(s['start'],s['end']) for s in result['segments']],[(0.,3.),(3.,6.)])
        self.assertEqual(timing['recognition_cache_misses'],0)

    def test_speaker_fallback_uses_predicted_turns_without_dropping_time(self):
        import numpy as np
        samples=np.arange(8,dtype='float32')
        turns=[dict(start=0.,end=3.,speaker='A'),dict(start=3.,end=6.,speaker='B')]
        result,timing,_calls=self.run_fake([(samples,0.,6.)],lambda sample:SimpleNamespace(
            text='बात',language='Hindi'),turns)
        self.assertEqual([(s['start'],s['end']) for s in result['segments']],[(0.,3.),(3.,6.)])
        self.assertEqual(timing['recognition_cache_misses'],0)

    def test_known_speaker_fallback_does_not_decode_unused_full_windows(self):
        import numpy as np
        sizes=[]
        def prediction(sample):
            sizes.append(len(sample))
            return SimpleNamespace(text='बात',language='Hindi')
        windows=[(np.arange(8,dtype='float32'),0.,6.),(np.arange(8,dtype='float32')+8,6.,6.)]
        turns=[dict(start=i*3.,end=(i+1)*3.,speaker=str(i%2)) for i in range(4)]
        result,timing,_calls=self.run_fake(windows,prediction,turns)
        self.assertEqual(sizes.count(8),1)
        self.assertEqual(timing['avoided_full_windows'],1)
        self.assertEqual([(s['start'],s['end']) for s in result['segments']],[(0.,3.),(3.,6.),(6.,9.),(9.,12.)])
        self.assertEqual(timing['recognition_cache_misses'],0)

    def test_mid_batch_language_change_prefetches_later_guided_windows(self):
        import numpy as np
        windows=[(np.arange(8,dtype='float32')+i*8,i*6.,6.) for i in range(3)]
        turns=[dict(start=i*3.,end=(i+1)*3.,speaker=str(i%2)) for i in range(6)]
        def prediction(samples):
            return SimpleNamespace(text='बात' if 8<=samples[0]<16 else 'hello',
                                   language='Hindi' if 8<=samples[0]<16 else 'English')
        class Aligner:
            def align(self,*,audio,text,language):
                inputs=audio if isinstance(audio,list) else [audio]
                return [[dict(text='hello',start_time=0.,end_time=len(item[0])*.75)] for item in inputs]
        result,timing,_=self.run_fake(windows,prediction,turns,batch_size=3,aligner=Aligner())
        self.assertEqual(timing['recognition_cache_misses'],0)
        self.assertEqual(timing['alignment_cache_misses'],0)
        self.assertEqual(result['audio_seconds'],18.)
        self.assertEqual(result['text'],'hello\nबात\nबात\nhello\nhello')
        self.assertEqual([(r['start'],r['end']) for r in result['segments']],
                         [(0.,6.),(6.,9.),(9.,12.),(12.,15.),(15.,18.)])

    def test_full_retry_replaces_potentially_clipped_text_before_replay(self):
        import numpy as np
        budgets=[]
        class Model:
            max_new_tokens=1024
            processor=SimpleNamespace(tokenizer=SimpleNamespace(encode=lambda text,**kw:list(text)))
            def transcribe(self, *, audio, **kwargs):
                budgets.append(self.max_new_tokens)
                items=audio if isinstance(audio,list) else [audio]
                text='x'*224 if self.max_new_tokens==256 else 'complete transcription'
                return [SimpleNamespace(text=text,language='English') for _ in items]
        class Engine(ThroughputEngine):
            def _load(self,name):
                if self._model is None:self._model=Model()
                return self._model
            def _load_aligner(self):return None
        engine=Engine(batch_size=2)
        engine._torch=SimpleNamespace(cuda=SimpleNamespace(empty_cache=lambda:None))
        windows=[(np.ones(8,dtype='float32'),0.,30.)]
        with tempfile.NamedTemporaryFile(suffix='.wav') as audio, \
             patch('evaluations.meetingbench.throughput_asr.audio_windows',side_effect=lambda path:iter(windows)), \
             patch('sonefold_speech.asr.audio_windows',side_effect=lambda path:iter(windows)):
            result,timing=engine.transcribe_batched(audio.name)
        self.assertEqual(result['text'],'complete transcription')
        self.assertEqual(budgets,[256,1024])
        self.assertEqual(timing['full_decode_retries'],1)


if __name__ == '__main__':
    unittest.main()
