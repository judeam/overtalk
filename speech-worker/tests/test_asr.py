import io
import importlib.util
import json
import math
import os
import sys
from pathlib import Path
import tempfile
import subprocess
import time
from types import SimpleNamespace
import unittest
from unittest.mock import MagicMock, patch

from sonefold_speech.asr import (
    ASREngine, SpeechError, cached_checkpoint, choose_device, decodable_audio,
    ALIGNMENT_WARNING, REPETITION_RETRY_WARNING, has_repetition_loop, join_word_text, local_audio_path, normalize_language, parse_words, restore_aligned_text, split_speaker_window, word_segments,
)
from sonefold_speech.__main__ import analyze, serve, speaker_counts, split_speaker_turns, stream, streaming_language, run_diarization_request


class FakeSamples:
    def any(self):
        return True


class ASRTests(unittest.TestCase):
    def test_optional_multilingual_timing_preserves_text_before_speaker_assignment(self):
        class Samples(FakeSamples):
            def __len__(self):
                return 96000

            def __getitem__(self, bounds):
                return self

        engine, model = ASREngine("cpu"), MagicMock()
        model.transcribe.return_value = [SimpleNamespace(text="xin chào cảm ơn", language="Vietnamese")]
        words = [{"text": "xin chào", "start": 0.1, "end": 2.7, "speaker": None},
                 {"text": "cảm ơn", "start": 3.1, "end": 5.7, "speaker": None}]
        diarization = {"turns": [{"start": 0, "end": 3, "speaker": "A"},
                                 {"start": 3, "end": 6, "speaker": "B"}],
                       "speakers": [{"id": "A"}, {"id": "B"}], "model": "nemotron-3"}
        with tempfile.NamedTemporaryFile() as audio, \
                patch.dict(os.environ, {"SONEFOLD_OMNI_ALIGNER_DIR": "/preloaded/omni"}), \
                patch("sonefold_speech.asr.audio_windows", return_value=[(Samples(), 0, 6)]), \
                patch.object(engine, "_load", return_value=model), patch.object(engine, "unload"), \
                patch.object(engine, "_multilingual_words", return_value=words, create=True) as align, \
                patch("sonefold_speech.__main__.run_diarization_request", return_value=diarization):
            result = analyze({"id": "aligned", "operation": "analyze", "audio_path": audio.name,
                              "language": "vi", "diarize": True}, engine)
        align.assert_called_once()
        self.assertEqual(result["text"], "xin chào cảm ơn")
        self.assertEqual([s["speaker"] for s in result["segments"]], ["A", "B"])
        self.assertNotIn(ALIGNMENT_WARNING, result["warnings"])
        self.assertEqual(model.transcribe.call_count, 1)

    def test_speaker_partition_preserves_every_sample_and_absolute_time(self):
        samples = range(32000)
        partitions = split_speaker_window(samples, 12.345, 2, [12.445, 13.345, 14.344])
        self.assertEqual([len(part[0]) for part in partitions], [16000, 16000])
        self.assertEqual([sample for part, _, _ in partitions for sample in part], list(samples))
        self.assertEqual([(offset, duration) for _, offset, duration in partitions],
                         [(12.345, 1), (13.345, 1)])
        self.assertEqual(split_speaker_window(samples, 12.345, 2, []), [(samples, 12.345, 2)])

    def test_diarized_language_without_aligner_decodes_each_speaker_turn(self):
        # A 30-second ASR block can span several perfectly diarized speakers.
        # Vietnamese has no Qwen forced aligner; assigning that whole block
        # loses both identities. Exercise the real decoding/protocol seam.
        class Samples(FakeSamples):
            def __init__(self, start=0, end=96000):
                self.start, self.end = start, end

            def __len__(self):
                return self.end - self.start

            def __getitem__(self, bounds):
                start, end, step = bounds.indices(len(self))
                self.assert_unit_step(step)
                return Samples(self.start + start, self.start + end)

            @staticmethod
            def assert_unit_step(step):
                assert step == 1

        engine, model = ASREngine("cpu"), MagicMock()

        def decode(*, audio, **kwargs):
            samples = audio[0]
            text = "xin chào cảm ơn" if len(samples) == 96000 else (
                "xin chào" if samples.start == 0 else "cảm ơn")
            return [SimpleNamespace(text=text, language="Vietnamese")]

        model.transcribe.side_effect = decode
        diarization = {"turns": [{"start": 0, "end": 3, "speaker": "A"},
                                 {"start": 3, "end": 6, "speaker": "B"}],
                       "speakers": [{"id": "A"}, {"id": "B"}], "model": "nemotron-3"}
        with tempfile.NamedTemporaryFile() as audio, \
                patch("sonefold_speech.asr.audio_windows", return_value=[(Samples(), 0, 6)]), \
                patch.object(engine, "_load", return_value=model), \
                patch.object(engine, "unload"), \
                patch("sonefold_speech.__main__.run_diarization_request", return_value=diarization):
            result = analyze({"id": "vietnamese", "operation": "analyze", "audio_path": audio.name,
                              "language": "vi", "model": "qwen3-asr-1.7b", "diarize": True}, engine)
        self.assertEqual([s["speaker"] for s in result["segments"]], ["A", "B"])
        self.assertEqual([s["text"] for s in result["segments"]], ["xin chào", "cảm ơn"])
        self.assertEqual([(s["start"], s["end"]) for s in result["segments"]], [(0, 3), (3, 6)])
        self.assertFalse(any(s.get("words") for s in result["segments"]))

    def test_failed_turn_guided_asr_releases_model_memory(self):
        engine = MagicMock()
        engine._stream_session = None
        engine.transcribe.side_effect = SpeechError("inference failed")
        diarization = {"turns": [], "speakers": [], "model": "nemotron-3"}
        with patch("sonefold_speech.__main__.run_diarization_request", return_value=diarization):
            with self.assertRaisesRegex(SpeechError, "inference failed"):
                analyze({"id": "failure", "operation": "analyze", "audio_path": "/tmp/audio",
                         "diarize": True}, engine)
        self.assertEqual(engine.unload.call_count, 2)

    def test_project_vocabulary_is_a_hint_and_preserves_recognized_text(self):
        engine, model = ASREngine("cpu"), MagicMock()
        model.transcribe.return_value = [SimpleNamespace(text="Actual spoken words", language="English")]
        with tempfile.NamedTemporaryFile() as audio, \
                patch("sonefold_speech.asr.audio_windows", return_value=[(FakeSamples(), 0, 3)]), \
                patch.object(engine, "_load", return_value=model):
            result = engine.transcribe(audio.name, word_timestamps=False, context="Overtalk, Priya")
        self.assertEqual(model.transcribe.call_args.kwargs["context"], "Overtalk, Priya")
        self.assertEqual(result["text"], "Actual spoken words")
        with self.assertRaises(SpeechError):
            engine.transcribe("unused", context="x" * 4001)

    def test_only_sustained_phrase_repetition_requests_a_retry(self):
        self.assertTrue(has_repetition_loop("Not the replacement credential. " * 111))
        self.assertFalse(has_repetition_loop("No, no, I said no. Please check the replacement credential."))
        self.assertFalse(has_repetition_loop("Please check the replacement credential. " * 3))

    def test_long_sentence_repetition_requests_audio_retry_without_deleting_text(self):
        phrase = ("Understood. I have corrected my notes. I was looking at the earlier version, "
                  "which explains why the amount and date did not match. ")
        self.assertTrue(has_repetition_loop(phrase * 8))
        self.assertFalse(has_repetition_loop(phrase * 7))
        self.assertFalse(has_repetition_loop(" ".join(f"word{i}" for i in range(1024))))

    def test_repetitive_asr_redecodes_audio_without_deleting_or_inventing_words(self):
        class Samples(FakeSamples):
            def __len__(self):
                return 320000

            def __getitem__(self, key):
                return self

        repeated = [
            "Not the replacement credential. " * 111,
            ("Understood. I have corrected my notes. I was looking at the earlier version, "
             "which explains why the amount and date did not match. ") * 16,
        ]
        for text in repeated:
            with self.subTest(phrase=text[:50]):
                engine, model = ASREngine("cpu"), MagicMock()
                model.transcribe.side_effect = [
                    [SimpleNamespace(text=text, language="English")],
                    [SimpleNamespace(text="Please verify the worker.", language="English")],
                    [SimpleNamespace(text="The new credential is safe.", language="English")],
                ]
                with tempfile.NamedTemporaryFile() as audio, \
                        patch("sonefold_speech.asr.audio_windows", return_value=[(Samples(), 120, 20)]), \
                        patch.object(engine, "_load", return_value=model):
                    result = engine.transcribe(audio.name, word_timestamps=False)
                self.assertEqual(result["text"], "Please verify the worker.\nThe new credential is safe.")
                self.assertEqual([(s["start"], s["end"]) for s in result["segments"]], [(120, 130), (130, 140)])
                self.assertEqual(result["audio_seconds"], 140)
                self.assertEqual(result["warnings"], [REPETITION_RETRY_WARNING])
                self.assertEqual(model.transcribe.call_count, 3)

    def test_benchmark_distance_and_silence_metrics_match_known_results(self):
        spec = importlib.util.spec_from_file_location("sonefold_benchmark", Path(__file__).resolve().parents[1] / "tools/benchmark_asr.py")
        benchmark = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(benchmark)
        self.assertEqual(benchmark.edit_distance("kitten", "sitting"), 3)
        self.assertEqual(benchmark.edit_distance(["one", "two"], ["one", "new", "two"]), 1)
        self.assertEqual(benchmark.edit_distance("", "words"), 5)
        self.assertEqual(benchmark.edit_distance("a" * 2000, "a" * 1999 + "b"), 1)
        self.assertEqual(benchmark.error_rates("Hello, WORLD!", "hello world")["wer"], 0)
        silence = benchmark.error_rates("", "Invented words")
        self.assertIsNone(silence["wer"])
        self.assertEqual(silence["silence_hallucination_words"], 2)

    def test_language_hints_validate_model_coverage(self):
        self.assertEqual(normalize_language("de-DE", "qwen3-asr-1.7b"), "German")
        self.assertEqual(normalize_language("English", "qwen3-asr-1.7b"), "English")
        self.assertIsNone(normalize_language("de", "parakeet-tdt-0.6b-v3"))
        with self.assertRaises(SpeechError):
            normalize_language("uk", "qwen3-asr-0.6b")
        with self.assertRaises(SpeechError):
            normalize_language("zh", "parakeet-tdt-0.6b-v3")

    def test_urls_cannot_reach_audio_loader(self):
        for value in ("https://example.org/a.wav", "data:audio/wav;base64,a", "", None):
            with self.subTest(value=value), self.assertRaises(SpeechError):
                local_audio_path(value)

    def test_timestamp_offsets_and_near_boundary_rounding(self):
        result = parse_words([SimpleNamespace(text="Hi", start_time=0.25, end_time=1.01)], 30, 1)
        self.assertEqual(result, [{"text": "Hi", "start": 30.25, "end": 31.0, "speaker": None}])

    def test_bad_model_timestamps_cannot_be_persisted(self):
        for start, end in ((-1, 2), (2, 1), (1, 1), (3.01, 3.02), (0, 5), (math.nan, 1), (True, 1)):
            with self.subTest(start=start, end=end), self.assertRaises(SpeechError):
                parse_words([{"text": "bad", "start": start, "end": end}], 0, 3)
        with self.assertRaises(SpeechError):
            parse_words([{"text": "a", "start": 2, "end": 2.5}, {"text": "b", "start": 1, "end": 2}], 0, 3)

    def test_collapsed_aligner_word_keeps_neighboring_speech_and_all_text(self):
        # Qwen quantizes boundaries to frames; a short word can have zero duration.
        # One such word must not discard all real timings in a 30-second passage.
        spans = [dict(text="Eight", start=0, end=0),
                 dict(text="o'clock", start=0, end=.4),
                 dict(text="tonight", start=.4, end=1),
                 dict(text="is", start=1, end=1),
                 dict(text="cancelled", start=1, end=1.5),
                 dict(text="now", start=1.5, end=1.5)]
        words = parse_words(spans, 210, 2, coalesce_zero_duration=True)
        restored = restore_aligned_text(words, "Eight o'clock tonight is cancelled now.")
        self.assertEqual(join_word_text(restored), "Eight o'clock tonight is cancelled now.")
        self.assertEqual([(w["start"], w["end"]) for w in restored],
                         [(210, 210.4), (210.4, 211), (211, 211.5)])
        self.assertEqual(word_segments(restored, "English")[0]["timing"], "phrase")

    def test_collapsed_alignment_cannot_fabricate_times_or_hide_invalid_bounds(self):
        for spans in ([dict(text="lost", start=0, end=0)],
                      [dict(text="bad", start=-1, end=0)],
                      [dict(text="bad", start=1, end=.5)],
                      [dict(text="bad", start=0, end=4)]):
            with self.subTest(spans=spans), self.assertRaises(SpeechError):
                parse_words(spans, 0, 2, coalesce_zero_duration=True)

    def test_groups_words_without_inventing_cjk_spaces(self):
        words = [{"text": "你好", "start": 0, "end": 1, "speaker": None},
                 {"text": "世界。", "start": 1, "end": 2, "speaker": None}]
        segments = word_segments(words, "Chinese")
        self.assertEqual(segments[0]["text"], "你好世界。")
        self.assertEqual(segments[0]["words"], words)

    def test_qwen_alignment_keeps_original_punctuation_and_mixed_script_spacing(self):
        words = [{"text": "Hello", "start": 0, "end": .5}, {"text": "world", "start": .5, "end": 1}]
        restored = restore_aligned_text(words, '"Hello,  world!"')
        self.assertEqual(join_word_text(restored), '"Hello,  world!"')
        self.assertEqual(restored[0]["start"], 0)
        self.assertEqual(restored[1]["end"], 1)
        mixed = restore_aligned_text([{"text": "你"}, {"text": "好"}, {"text": "AI"}], "你好 AI。")
        self.assertEqual(join_word_text(mixed, "Chinese"), "你好 AI。")
        with self.assertRaises(SpeechError):
            restore_aligned_text(words, "Hello extra world.")

    def test_alignment_survives_accent_spelling_and_a_dropped_short_word(self):
        decomposed = [{"text": "Discussa\u0303o", "start": 0, "end": .6}, {"text": "sobre", "start": .6, "end": .9},
                      {"text": "questo\u0303es", "start": .9, "end": 1.4}, {"text": "técnicas", "start": 1.4, "end": 2}]
        restored = restore_aligned_text(decomposed, "Discussão sobre as questões técnicas.")
        self.assertEqual([w["text"] for w in restored], ["Discussão", "sobre as", "questões", "técnicas."])
        self.assertEqual(join_word_text(restored), "Discussão sobre as questões técnicas.")
        self.assertEqual(restored[2]["start"], .9)

    def test_loading_a_qwen_model_warms_it_once(self):
        engine = ASREngine("cpu")
        model = MagicMock()
        fake_numpy = SimpleNamespace(zeros=lambda size, dtype=None: [0.0] * size)
        with patch.object(engine, "_load", return_value=model), patch.dict("sys.modules", {"numpy": fake_numpy}):
            engine.load("qwen3-asr-1.7b")
            engine.load("qwen3-asr-1.7b")
        self.assertEqual(model.transcribe.call_count, 1)
        self.assertIsNone(model.transcribe.call_args.kwargs["language"])

    def test_cached_models_use_local_files_only(self):
        hub = MagicMock()
        hub.snapshot_download.return_value = "/tmp/cached-model"
        with patch.dict(os.environ, {}, clear=True), patch("sonefold_speech.asr.importlib.import_module", return_value=hub):
            cached_checkpoint("Qwen/test", "MODEL_DIR")
        hub.snapshot_download.assert_called_once_with(repo_id="Qwen/test", local_files_only=True)

    def test_bad_explicit_aligner_path_reports_configuration_error(self):
        with patch.dict(os.environ, {"MODEL_DIR": "/does/not/exist"}), self.assertRaises(SpeechError):
            cached_checkpoint("Qwen/test", "MODEL_DIR", optional=True)

    def test_only_one_asr_model_is_cached(self):
        qwen = MagicMock()
        qwen.Qwen3ASRModel.from_pretrained.side_effect = [object(), object()]
        engine = ASREngine("cpu")
        engine._torch = MagicMock()
        engine.dtype = "float32"
        with patch("sonefold_speech.asr.cached_checkpoint", return_value=Path("/tmp/model")), \
                patch("sonefold_speech.asr.importlib.import_module", return_value=qwen):
            first = engine._load("qwen3-asr-1.7b")
            self.assertIs(first, engine._load("qwen3-asr-1.7b"))
            self.assertIsNot(first, engine._load("qwen3-asr-0.6b"))
        self.assertEqual(qwen.Qwen3ASRModel.from_pretrained.call_count, 2)
        self.assertTrue(qwen.Qwen3ASRModel.from_pretrained.call_args.kwargs["local_files_only"])
        self.assertEqual(qwen.Qwen3ASRModel.from_pretrained.call_args.kwargs["max_inference_batch_size"], 1)

    def test_unavailable_cuda_is_explicit(self):
        torch = MagicMock()
        torch.cuda.is_available.return_value = False
        with self.assertRaises(SpeechError):
            choose_device(torch, "cuda:0")

    def test_device_change_releases_cached_model_and_revalidates_runtime(self):
        engine = ASREngine("cpu")
        engine._model, engine._model_name, engine._torch = object(), "qwen3-asr-1.7b", MagicMock()
        engine.configure_device("cuda:0")
        self.assertIsNone(engine._model)
        self.assertIsNone(engine._torch)
        self.assertEqual(engine.requested_device, "cuda:0")
        for value in (True, 4, "mps", "cuda:-1", "cuda:garbage"):
            with self.subTest(value=value), self.assertRaises(SpeechError):
                engine.configure_device(value)

    def test_unsupported_alignment_language_uses_visible_coarse_timing(self):
        engine = ASREngine("cpu")
        model = MagicMock()
        model.transcribe.return_value = [SimpleNamespace(text="नमस्ते", language="Hindi")]
        with tempfile.NamedTemporaryFile() as audio, \
                patch("sonefold_speech.asr.audio_windows", return_value=iter([(FakeSamples(), 12, 2)])), \
                patch.object(engine, "_load", return_value=model), patch.object(engine, "_load_aligner") as aligner:
            result = engine.transcribe(audio.name)
        aligner.assert_not_called()
        self.assertEqual(result["segments"][0]["start"], 12)
        self.assertEqual(result["segments"][0]["end"], 14)
        self.assertEqual(result["segments"][0]["timing"], "chunk")
        self.assertTrue(result["warnings"])

    def test_official_qwen_align_api_preserves_absolute_offsets(self):
        engine = ASREngine("cpu")
        model, aligner = MagicMock(), MagicMock()
        model.transcribe.return_value = [SimpleNamespace(text="Hello world.", language="English")]
        aligner.align.return_value = [[SimpleNamespace(text="Hello", start_time=0, end_time=0.5),
                                      SimpleNamespace(text="world.", start_time=0.5, end_time=1)]]
        with tempfile.NamedTemporaryFile() as audio, \
                patch("sonefold_speech.asr.audio_windows", return_value=iter([(FakeSamples(), 30, 2)])), \
                patch.object(engine, "_load", return_value=model), patch.object(engine, "_load_aligner", return_value=aligner):
            result = engine.transcribe(audio.name, language="en")
        self.assertEqual(result["segments"][0]["text"], "Hello world.")
        self.assertEqual(result["segments"][0]["words"][1]["end"], 31)

    def test_invalid_alignment_keeps_text_and_marks_chunk_timing(self):
        engine = ASREngine("cpu")
        model, aligner = MagicMock(), MagicMock()
        model.transcribe.return_value = [SimpleNamespace(text="Keep these words.", language="English")]
        aligner.align.return_value = [[SimpleNamespace(text="Keep", start_time=0.5, end_time=0.5)]]
        with tempfile.NamedTemporaryFile() as audio, \
                patch("sonefold_speech.asr.audio_windows", return_value=iter([(FakeSamples(), 30, 2)])), \
                patch.object(engine, "_load", return_value=model), patch.object(engine, "_load_aligner", return_value=aligner):
            result = engine.transcribe(audio.name, language="en")
        self.assertEqual(result["segments"], [{"text": "Keep these words.", "start": 30, "end": 32,
                                               "speaker": None, "timing": "chunk"}])
        self.assertEqual(result["warnings"], [ALIGNMENT_WARNING])
        self.assertEqual(model.transcribe.call_args.kwargs["language"], "English")

    def test_compressed_audio_conversion_is_local_and_cleaned_up(self):
        sf = MagicMock()
        sf.info.side_effect = RuntimeError("format unsupported")
        with tempfile.NamedTemporaryFile() as ffmpeg, \
                patch("sonefold_speech.asr.shutil.which", return_value=ffmpeg.name), \
                patch("sonefold_speech.asr.subprocess.run") as run:
            with decodable_audio(Path("/tmp/recording.m4a"), sf) as normalized:
                folder = normalized.parent
                self.assertTrue(folder.is_dir())
            self.assertFalse(folder.exists())
        command = run.call_args.args[0]
        self.assertIn("/tmp/recording.m4a", command)
        self.assertIn("-nostdin", command)
        self.assertNotIn("shell", run.call_args.kwargs)

    def test_health_does_not_import_torch_or_load_models(self):
        engine = ASREngine()
        with patch("sonefold_speech.asr.importlib.util.find_spec", return_value=None), \
                patch("sonefold_speech.asr.cached_checkpoint", return_value=None), patch.object(engine, "_load") as load:
            result = engine.health()
        self.assertFalse(result["ready"])
        self.assertTrue(result["model_needs_download"])
        load.assert_not_called()

    def test_stream_health_checks_helpers_and_both_checkpoints_without_importing_models(self):
        engine = ASREngine("cpu")
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            asr, diar = root / "asr.nemo", root / "diar.nemo"
            asr.write_bytes(b"fixture")
            diar.write_bytes(b"fixture")
            helpers = root / "nemo/collections/asr/parts/utils/multispk_transcribe_utils.py"
            helpers.parent.mkdir(parents=True)
            helpers.write_text("class SpeakerTaggedASR: pass\ndef configure_diar_streaming(): pass\ndef validate_feature_frame_strides(): pass\n")
            spec = SimpleNamespace(submodule_search_locations=[str(root / "nemo")])
            with patch.dict(os.environ, {"SONEFOLD_STREAMING_ASR_MODEL_PATH": str(asr),
                                         "SONEFOLD_DIARIZATION_MODEL_DIR": str(diar)}), \
                    patch("sonefold_speech.asr.importlib.util.find_spec", return_value=spec), \
                    patch("sonefold_speech.asr.importlib.import_module") as imports:
                ready = engine.stream_health()
                self.assertTrue(ready["ready"])
                imports.assert_not_called()
                helpers.write_text("# Older NeMo runtime without streaming helpers\n")
                self.assertFalse(engine.stream_health()["ready"])
                diar.unlink()
                self.assertTrue(engine.stream_health()["model_needs_download"])


class ProtocolTests(unittest.TestCase):
    def test_overflowing_numbers_and_nested_error_fields_preserve_framing(self):
        # Nonfinite constants are already rejected; finite JSON literals can
        # also overflow float(), including fields echoed only on errors.
        for field, value in [('id', '1e999'), ('model', '-1e999'),
                             ('id', '[' * 2000 + '0' + ']' * 2000),
                             ('model', '[' * 2000 + '0' + ']' * 2000)]:
            with self.subTest(field=field, value=value[:20]):
                output = io.StringIO()
                request = '{"id":1,"operation":"invalid","' + field + '":' + value + '}\n'
                serve(io.StringIO(request + '{"id":"alive","operation":"ping"}\n'), output, MagicMock())
                replies = [json.loads(line) for line in output.getvalue().splitlines()]
                self.assertEqual(len(replies), 2)
                self.assertFalse(replies[0]['ok'])
                self.assertIsNone(replies[0][field])
                self.assertTrue(replies[1]['ok'])
                self.assertEqual(replies[1]['id'], 'alive')

    def test_invalid_json_does_not_stop_next_request(self):
        output = io.StringIO()
        serve(io.StringIO('broken\n{"id":"alive","operation":"ping"}\n'), output, MagicMock())
        responses = [json.loads(line) for line in output.getvalue().splitlines()]
        self.assertFalse(responses[0]["ok"])
        self.assertTrue(responses[1]["ok"])

    def test_model_stdout_cannot_corrupt_protocol(self):
        engine = MagicMock()
        engine._stream_session = None
        def noisy(*args, **kwargs):
            print("Library startup log")
            return {"segments": [], "speakers": [], "model": "test", "language": None}
        engine.transcribe.side_effect = noisy
        output = io.StringIO()
        with context_stderr():
            serve(io.StringIO('{"id":1,"operation":"analyze","audio_path":"/tmp/test"}\n'), output, engine)
        self.assertEqual(len(output.getvalue().splitlines()), 1)
        self.assertTrue(json.loads(output.getvalue())["ok"])

    def test_oversized_line_is_drained_before_next_request(self):
        output = io.StringIO()
        serve(io.StringIO("x" * 100000 + '\n{"id":2,"operation":"ping"}\n'), output, MagicMock())
        responses = [json.loads(line) for line in output.getvalue().splitlines()]
        self.assertEqual(len(responses), 2)
        self.assertEqual(responses[1]["id"], 2)

    def test_shutdown_releases_model_and_exits(self):
        engine, output = MagicMock(), io.StringIO()
        serve(io.StringIO('{"id":1,"operation":"shutdown"}\n{"id":2,"operation":"ping"}\n'), output, engine)
        engine.unload.assert_called_once()
        self.assertEqual(len(output.getvalue().splitlines()), 1)

    def test_invalid_speaker_counts_are_rejected_before_inference(self):
        for counts in ({"num_speakers": True}, {"min_speakers": 3, "max_speakers": 2},
                       {"num_speakers": 2, "min_speakers": 3}, {"max_speakers": 33}):
            with self.subTest(counts=counts), self.assertRaises(SpeechError):
                speaker_counts(counts)

    def test_online_mode_cannot_be_requested(self):
        with self.assertRaises(SpeechError):
            analyze({"id": 1, "operation": "load", "offline": False}, MagicMock())

    def test_load_passes_saved_device_to_engine(self):
        engine = MagicMock()
        engine._stream_session = None
        engine.load.return_value = {"model": "qwen3-asr-0.6b", "device": "cpu"}
        result = analyze({"id": 4, "operation": "load", "model": "qwen3-asr-0.6b", "device": "cpu"}, engine)
        self.assertTrue(result["ok"])
        engine.load.assert_called_once_with("qwen3-asr-0.6b", "cpu")

    def test_text_only_provider_skips_alignment_and_keeps_the_same_text(self):
        engine = ASREngine("cpu")
        model = MagicMock()
        model.transcribe.return_value = [SimpleNamespace(text="Hello, world.", language="English")]
        with tempfile.NamedTemporaryFile(suffix=".wav") as audio, \
                patch("sonefold_speech.asr.audio_windows", return_value=[(FakeSamples(), 0, 1.7)]), \
                patch.object(engine, "_load", return_value=model), patch.object(engine, "_load_aligner") as aligner:
            result = analyze({"id": "provider", "operation": "analyze", "audio_path": audio.name,
                              "word_timestamps": False, "diarize": False}, engine)
        self.assertTrue(result["ok"])
        self.assertEqual(result["text"], "Hello, world.")
        self.assertEqual(result["segments"][0]["text"], "Hello, world.")
        self.assertEqual(result["segments"][0]["end"], 1.7)
        self.assertEqual(result["warnings"], [])
        aligner.assert_not_called()

    def test_word_timestamp_options_are_validated_before_inference(self):
        for options in ({"word_timestamps": "false"}, {"word_timestamps": 0},
                        {"word_timestamps": False, "diarize": True}):
            engine = MagicMock()
            engine._stream_session = None
            with self.subTest(options=options), self.assertRaises(SpeechError):
                analyze({"id": "invalid", "operation": "analyze", **options}, engine)
            engine.transcribe.assert_not_called()

    def test_analysis_preserves_diarization_timelines(self):
        engine = ASREngine("cpu")
        engine.transcribe = MagicMock(return_value={"segments": [{"text": "Hi", "start": 0, "end": 1}],
                                                   "speakers": [], "language": "English"})
        diarization = {"turns": [{"start": 0, "end": 1, "speaker": "A"}], "exclusive_turns": [],
                       "speakers": [{"id": "A", "name": "Speaker 1"}], "model": "nemotron-3"}
        with patch("sonefold_speech.__main__.run_diarization_request", return_value=diarization):
            result = analyze({"id": 1, "operation": "analyze", "audio_path": "/tmp/audio", "diarize": True}, engine)
        self.assertEqual(result["turns"], diarization["turns"])
        self.assertEqual(result["segments"][0]["speaker"], "A")

    def test_speaker_word_changes_split_phrase_without_dropping_unknown(self):
        words = [{"text": text, "start": index, "end": index + 1, "speaker": speaker}
                 for index, (text, speaker) in enumerate((("Hello", "A"), ("um", None), ("world.", "B")))]
        result = split_speaker_turns([{"text": "Hello um world.", "start": 0, "end": 3, "words": words}], "English")
        self.assertEqual([item["text"] for item in result], ["Hello", "um", "world."])
        self.assertEqual([item["speaker"] for item in result], ["A", None, "B"])

    def test_isolated_diarization_preserves_offline_env_and_detects_errors(self):
        engine = ASREngine("cpu")
        with patch.dict(os.environ, {"SONEFOLD_DIARIZATION_PYTHON": sys.executable}), \
                patch("sonefold_speech.__main__.subprocess.run", return_value=SimpleNamespace(stdout='{"ok":true,"turns":[]}')) as run:
            result = run_diarization_request({"id": 1, "audio_path": "/tmp/a.wav"}, engine, {})
        self.assertEqual(result["turns"], [])
        environment = run.call_args.kwargs["env"]
        self.assertNotIn("SONEFOLD_DIARIZATION_PYTHON", environment)
        self.assertEqual(environment["SONEFOLD_PARENT_PID"], str(os.getpid()))
        self.assertEqual(environment["HF_HUB_OFFLINE"], "1")
        self.assertEqual(json.loads(run.call_args.kwargs["input"])["operation"], "diarize")

    def test_stream_pcm_base64_validates_frame_size_and_normalizes(self):
        import base64
        import struct
        engine = ASREngine("cpu")
        engine._stream_session = MagicMock()
        engine._stream_session.accept_audio.return_value = {"text": "Hi", "is_partial": True}
        result = stream({"operation": "stream_audio", "pcm16_base64": base64.b64encode(struct.pack("<hh", -32768, 16384)).decode()}, engine)
        self.assertTrue(result["is_partial"])
        engine._stream_session.accept_audio.assert_called_once_with([-1.0, 0.5], sample_rate=16000)
        with self.assertRaises(SpeechError):
            stream({"operation": "stream_audio", "pcm16_base64": "YQ=="}, engine)

    def test_stream_start_passes_model_locale_and_speaker_limit(self):
        engine = ASREngine("cpu")
        with patch.dict(os.environ, {"SONEFOLD_STREAMING_ASR_MODEL_PATH": "/tmp/asr.nemo",
                                     "SONEFOLD_DIARIZATION_MODEL_DIR": "/tmp/diar.nemo"}), \
                patch.object(engine, "_runtime"), patch("sonefold_speech.streaming.StreamingSession") as session:
            result = stream({"operation": "stream_start", "asr_model": "nemotron-3.5",
                             "language": "de", "max_speakers": 3}, engine)
        session.assert_called_once_with("/tmp/asr.nemo", "/tmp/diar.nemo", device="cpu", max_speakers=3,
                                        asr_model="nemotron-3.5", language="de-DE")
        self.assertIs(engine._stream_session, session.return_value)
        self.assertEqual(result["language"], "de-DE")

    def test_stream_language_preserves_locale_and_rejects_adaptation_only(self):
        self.assertEqual(streaming_language("fr_CA", "nemotron-3.5"), "fr-CA")
        self.assertEqual(streaming_language("en-GB", "multitalker-parakeet"), "en-US")
        for value in ("el", "th", "he", "nn-NO", "de-AT", [], True):
            with self.subTest(value=value), self.assertRaises(SpeechError):
                streaming_language(value, "nemotron-3.5")
        with self.assertRaises(SpeechError):
            streaming_language("de", "multitalker-parakeet")

    def test_stream_stop_closes_session_even_after_finalize_failure(self):
        engine = ASREngine("cpu")
        session = MagicMock()
        engine._stream_session = session
        session.finish.side_effect = RuntimeError("finalize failed")
        with self.assertRaises(RuntimeError):
            stream({"operation": "stream_stop"}, engine)
        session.close.assert_called_once()
        self.assertIsNone(engine._stream_session)

    @unittest.skipUnless(sys.platform.startswith("linux"), "Linux parent-death signal integration")
    def test_isolated_inference_exits_when_supervising_parent_exits(self):
        # Use an actual long-running child with no ML imports or GPU work. Its
        # readiness acknowledgement proves the guard was armed before exit.
        child_code = ("from sonefold_speech.__main__ import configure_parent_death_guard; "
                      "import time; configure_parent_death_guard(); print('ready',flush=True); time.sleep(60)")
        parent_code = ("import os,subprocess,sys; env=dict(os.environ); "
                       "env['SONEFOLD_PARENT_PID']=str(os.getpid()); "
                       f"child=subprocess.Popen([sys.executable,'-c',{child_code!r}],env=env,"
                       "stdout=subprocess.PIPE,stderr=subprocess.DEVNULL,text=True); "
                       "assert child.stdout.readline().strip()=='ready'; print(child.pid,flush=True)")
        parent = subprocess.run([sys.executable, "-c", parent_code], stdout=subprocess.PIPE,
                                stderr=subprocess.PIPE, text=True, timeout=10, check=True)
        child_pid = int(parent.stdout.strip())
        try:
            deadline = time.monotonic() + 5
            while time.monotonic() < deadline:
                try:
                    status = Path(f"/proc/{child_pid}/status").read_text()
                except FileNotFoundError:
                    break
                if "State:\tZ" in status:  # Terminated; container init may reap later.
                    break
                time.sleep(0.01)
            else:
                self.fail("Isolated inference survived its supervising worker")
        finally:
            import signal
            try:
                os.kill(child_pid, signal.SIGKILL)
            except ProcessLookupError:
                pass


def context_stderr():
    from contextlib import redirect_stderr
    return redirect_stderr(io.StringIO())


if __name__ == "__main__":
    unittest.main()
