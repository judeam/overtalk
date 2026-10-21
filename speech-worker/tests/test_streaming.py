import importlib.util
import unittest

from sonefold_speech.streaming import PCMFrameBuffer, StreamingSession, segments_from_seglst, mask_log_mel_features, _prewarm_streamer


class FakeAdapter:
    hop_samples = 3
    cache_samples = 2

    def __init__(self):
        self.frames = []
        self.closed = False

    def process(self, samples, *, final):
        self.frames.append((list(samples), final))

    def records(self):
        return [{"speaker": "speaker_0", "words": "Hello", "start_time": 0, "end_time": .001}] if self.frames else []

    def close(self):
        self.closed = True


class StreamingTests(unittest.TestCase):
    def test_prewarm_discards_decoder_labels_and_resets_first_recording_step(self):
        class Streamer:
            _cache_gating = True
            def __init__(self):
                self.records = []
        class Adapter:
            hop_samples, cache_samples = 3, 2
            def __init__(self):
                self.streamer, self.step = Streamer(), 0
                self.calls = []
            def process(self, samples, *, final):
                self.calls.append((len(samples), final, self.streamer._cache_gating))
                self.streamer.records.append("Warmup label")
                self.step += 1
        adapter = Adapter()
        _prewarm_streamer(adapter, Streamer)
        self.assertEqual(adapter.calls, [(5, False, False), (5, True, False)])
        self.assertEqual(adapter.step, 0)
        self.assertEqual(adapter.streamer.records, [])
        self.assertTrue(adapter.streamer._cache_gating)

    def test_failed_prewarm_still_resets_recording_state(self):
        class Streamer:
            _cache_gating = True
        class Adapter:
            hop_samples, cache_samples = 3, 2
            step = 7
            streamer = Streamer()
            def process(self, samples, *, final):
                raise RuntimeError("Decoder initialization failed")
        adapter = Adapter()
        with self.assertRaisesRegex(RuntimeError, "Decoder initialization"):
            _prewarm_streamer(adapter, Streamer)
        self.assertEqual(adapter.step, 0)
        self.assertTrue(adapter.streamer._cache_gating)

    def test_equal_frame_starts_keep_both_segments_with_distinct_stable_ids(self):
        records = [
            {"speaker": 0, "words": "Hello", "start_time": 0, "end_time": .4},
            {"speaker": 0, "words": "again", "start_time": 0, "end_time": .8},
        ]
        first = segments_from_seglst(records)
        self.assertEqual(len(set(segment["id"] for segment in first)), 2)
        records[1]["words"] = "again."
        self.assertEqual([segment["id"] for segment in first], [segment["id"] for segment in segments_from_seglst(records)])

    def test_geometry_carries_context_and_flushes_tail(self):
        buffer = PCMFrameBuffer(3, 2)
        buffer.append([.1, .2])
        self.assertEqual(list(buffer.frames()), [])
        buffer.append([.3, .4, .5, .6, .7])
        self.assertEqual(list(buffer.frames()), [[0, 0, .1, .2, .3], [.2, .3, .4, .5, .6]])
        self.assertEqual(buffer.finish(), [.5, .6, .7])
        self.assertIsNone(buffer.finish())
        self.assertEqual(buffer.total_samples, 7)

    def test_validation_is_atomic(self):
        buffer = PCMFrameBuffer(3, 0, 4)
        for samples in ([.1, float("nan")], [2], [True], [0] * 5):
            with self.assertRaises(ValueError):
                buffer.append(samples)
        with self.assertRaises(ValueError):
            buffer.append([0], 48000)
        self.assertEqual(buffer.total_samples, 0)
        self.assertEqual(buffer.pending, [])

    def test_sessions_keep_independent_state_and_finalize_once(self):
        first, second = FakeAdapter(), FakeAdapter()
        a, b = StreamingSession(_adapter=first), StreamingSession(_adapter=second)
        result = a.accept_audio([.1, .2, .3, .4])
        self.assertTrue(result["is_partial"])
        self.assertEqual(len(first.frames), 1)
        self.assertEqual(second.frames, [])
        final = a.finish()
        self.assertFalse(final["is_partial"])
        self.assertEqual(first.frames[-1], ([.2, .3, .4], True))
        count = len(first.frames)
        self.assertEqual(a.finish(), final)
        self.assertEqual(len(first.frames), count)
        with self.assertRaises(ValueError):
            a.accept_audio([0])
        a.close()
        self.assertTrue(first.closed)
        self.assertFalse(second.closed)
        b.close()

    def test_empty_session_emits_no_inference(self):
        adapter = FakeAdapter()
        session = StreamingSession(_adapter=adapter)
        self.assertEqual(session.finish()["segments"], [])
        self.assertEqual(adapter.frames, [])

    def test_overlapping_structured_output_keeps_both_voices(self):
        result = segments_from_seglst([
            {"speaker": 0, "words": "one", "start_time": 0, "end_time": 1},
            {"speaker": "speaker_1", "words": "two", "start_time": .5, "end_time": 2},
        ])
        self.assertEqual([segment["text"] for segment in result], ["one", "two"])
        self.assertTrue(all(segment["overlap"] for segment in result))
        self.assertEqual(result[1]["speaker"], "SPEAKER_01")

    def test_final_snapshot_clips_only_artificial_future_time(self):
        result = segments_from_seglst([
            {"speaker": 0, "words": "tail", "start_time": .5, "end_time": 2},
            {"speaker": 0, "words": "padding", "start_time": 2, "end_time": 3},
        ], duration=1, partial=False)
        self.assertEqual(len(result), 1)
        self.assertEqual(result[0]["text"], "tail")
        self.assertEqual(result[0]["end"], 1)
        self.assertFalse(result[0]["is_partial"])

    def test_missing_checkpoint_and_invalid_count_fail_without_ml_imports(self):
        with self.assertRaisesRegex(RuntimeError, "predownloaded"):
            StreamingSession(asr_path="/nonexistent/model.nemo")
        with self.assertRaises(ValueError):
            StreamingSession(max_speakers=9)


@unittest.skipUnless(importlib.util.find_spec("torch"), "Tensor masking tests require PyTorch")
class LogMelMaskTests(unittest.TestCase):
    def test_inactive_frames_use_silence_floor_and_active_zeros_survive(self):
        import torch
        features = torch.arange(16, dtype=torch.float32).reshape(1, 1, 16)
        masked = mask_log_mel_features(features, torch.tensor([[1., 0.]]))
        self.assertTrue(torch.equal(masked[:, :, :8], features[:, :, :8]))
        self.assertEqual(masked[0, 0, 0].item(), 0)
        self.assertTrue(torch.all(masked[:, :, 8:] == -16.6355))

    def test_missing_left_cache_is_silence_for_every_feature_and_speaker(self):
        import torch
        features = torch.ones(2, 3, 20)
        mask = torch.tensor([[1., 0.], [0., 1.]])
        masked = mask_log_mel_features(features, mask)
        self.assertTrue(torch.all(masked[:, :, :4] == -16.6355))
        self.assertTrue(torch.all(masked[0, :, 4:12] == 1))
        self.assertTrue(torch.all(masked[0, :, 12:] == -16.6355))
        self.assertTrue(torch.all(masked[1, :, 4:12] == -16.6355))
        self.assertTrue(torch.all(masked[1, :, 12:] == 1))

    def test_mask_batch_mismatch_is_rejected(self):
        import torch
        with self.assertRaises(ValueError):
            mask_log_mel_features(torch.ones(2, 3, 8), torch.ones(1, 1))


if __name__ == "__main__":
    unittest.main()
