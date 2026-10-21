import importlib.util
import sys
import unittest
from pathlib import Path

TOOLS = Path(__file__).parents[1] / "tools"
sys.path.insert(0, str(TOOLS))
from score_predictions import score
from score_speakers import validated

@unittest.skipUnless(importlib.util.find_spec("jiwer"), "Install requirements-eval.txt")
class EvaluationTests(unittest.TestCase):
    def test_failed_recording_counts_as_deletions_in_operational_metric(self):
        manifest = [{"audio_path": "a", "text": "one two three", "session_id": "meeting"},
                    {"audio_path": "b", "text": "four five", "session_id": "meeting"}]
        result = score(manifest, [{"audio_path": "a", "prediction": "one six three seven"},
                                  {"audio_path": "b", "prediction": None, "error": "refused"}])
        self.assertEqual(result["completed"], 1)
        self.assertEqual(result["accepted_predictions"]["reference_words"], 3)
        metric = result["all_recordings_failed_as_empty_predictions"]
        self.assertEqual((metric["substitutions"], metric["deletions"], metric["insertions"]), (1, 2, 1))
        self.assertEqual(metric["reference_words"], 5)
        self.assertEqual(metric["wer"], .8)

    def test_incomplete_duplicate_and_unknown_predictions_are_rejected(self):
        manifest = [{"audio_path": "a", "text": "hello"}, {"audio_path": "b", "text": "world"}]
        for predictions in ([{"audio_path": "a", "prediction": "hello"}],
                            [{"audio_path": "a", "prediction": "hello"}] * 2,
                            [{"audio_path": "other", "prediction": "hello"}]):
            with self.assertRaises(ValueError): score(manifest, predictions)

class SpeakerInputTests(unittest.TestCase):
    def test_invalid_timing_does_not_enter_diarization_metrics(self):
        for start, end in [(True, 1), (0, float("nan")), (2, 1), (-1, 1)]:
            with self.assertRaises(ValueError):
                validated([dict(session_id="meeting", speaker="anonymous", start_time=start, end_time=end, words="yes")])

@unittest.skipUnless(importlib.util.find_spec("torch"), "Torch is optional in protocol-only tests")
class PoolingTests(unittest.TestCase):
    def test_pooling_uses_last_attended_token_for_both_padding_directions(self):
        import torch
        from sonefold_speech.retrieval import last_token_pool
        hidden = torch.tensor([[[1.], [2.], [3.]], [[4.], [5.], [6.]]])
        result = last_token_pool(hidden, torch.tensor([[1, 1, 0], [0, 1, 1]]))
        self.assertEqual(result.tolist(), [[2.], [6.]])
        with self.assertRaises(Exception): last_token_pool(hidden[:1], torch.tensor([[0, 0, 0]]))

if __name__ == "__main__": unittest.main()
