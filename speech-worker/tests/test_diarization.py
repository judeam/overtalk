import unittest
from pathlib import Path
import tempfile

from sonefold_speech.diarization import assign_speakers, diarize, speaker_change_boundaries


class AssignmentTests(unittest.TestCase):
    def test_speaker_boundaries_keep_same_voice_pauses_and_overlaps_together(self):
        turns = [{"start": 0, "end": 2, "speaker": "A"},
                 {"start": 2.2, "end": 4, "speaker": "A"},
                 {"start": 5, "end": 8, "speaker": "B"}]
        self.assertEqual(speaker_change_boundaries(turns), [4.5])
        # A long utterance still covers both shorter interruptions. Adjacent
        # list entries alone cannot establish a non-overlapping cut.
        nested = [{"start": 0, "end": 10, "speaker": "A"},
                  {"start": 2, "end": 3, "speaker": "B"},
                  {"start": 4, "end": 5, "speaker": "C"},
                  {"start": 11, "end": 12, "speaker": "D"}]
        self.assertEqual(speaker_change_boundaries(nested), [10.5])

    def test_preserves_text_times_ids_and_source(self):
        segments = [{"id": "original", "text": "Hello!", "start": 2.0, "end": 3.0, "speaker": "mic"}]
        result = assign_speakers(segments, [{"start": 2, "end": 3, "speaker": "A"}])
        self.assertEqual(result[0]["speaker"], "A")
        self.assertEqual(result[0]["source_speaker"], "mic")
        for key in ("id", "text", "start", "end"):
            self.assertEqual(result[0][key], segments[0][key])
        self.assertEqual(segments[0]["speaker"], "mic")

    def test_overlap_ambiguous_and_silence_remain_unknown(self):
        turns = [{"start": 0, "end": 1, "speaker": "A"}, {"start": 0, "end": 1, "speaker": "B"}]
        result = assign_speakers([{"text": "overlap", "start": 0, "end": 1},
                                 {"text": "silence", "start": 2, "end": 3}], turns)
        self.assertIsNone(result[0]["speaker"])
        self.assertTrue(result[0]["overlap"])
        self.assertTrue(result[0]["speaker_ambiguous"])
        self.assertIsNone(result[1]["speaker"])
        self.assertFalse(result[1]["overlap"])

    def test_exclusive_attribution_preserves_regular_overlap(self):
        regular = [{"start": 0, "end": 1, "speaker": "A"}, {"start": 0, "end": 1, "speaker": "B"}]
        result = assign_speakers([{"text": "Hello", "start": 0, "end": 1}], regular, regular[:1])
        self.assertEqual(result[0]["speaker"], "A")
        self.assertTrue(result[0]["overlap"])

    def test_adjacent_speakers_are_ambiguous_but_do_not_overlap(self):
        result = assign_speakers([{"text": "Both spoke", "start": 0, "end": 2}],
                                [{"start": 0, "end": 1, "speaker": "A"},
                                 {"start": 1, "end": 2, "speaker": "B"}])
        self.assertIsNone(result[0]["speaker"])
        self.assertFalse(result[0]["overlap"])

    def test_duplicate_turns_do_not_inflate_support(self):
        result = assign_speakers([{"text": "short", "start": 0, "end": 1}],
                                [{"start": 0, "end": .4, "speaker": "A"}] * 3)
        self.assertIsNone(result[0]["speaker"])

    def test_words_are_assigned_individually_without_losing_unaligned_word(self):
        segment = {"text": "hello there", "start": 0, "end": 2, "words": [
            {"text": "hello", "start": 0, "end": .8}, {"text": "there", "start": 1, "end": 2},
            {"text": "2014", "start": None, "end": None}]}
        result = assign_speakers([segment], [{"start": 0, "end": 1, "speaker": "A"},
                                            {"start": 1, "end": 2, "speaker": "B"}])
        self.assertEqual([word["speaker"] for word in result[0]["words"]], ["A", "B", None])

    def test_invalid_intervals_fail_without_mutating_input(self):
        for start, end in [(0, 0), (2, 1), (-1, 1), (0, float("nan"))]:
            with self.assertRaises(ValueError):
                assign_speakers([{"text": "x", "start": start, "end": end}], [])

    def test_missing_model_fails_before_importing_ml_packages(self):
        with tempfile.TemporaryDirectory() as directory:
            audio = Path(directory) / "test.wav"
            audio.write_bytes(b"unused")
            with self.assertRaisesRegex(RuntimeError, "Download a local diarization"):
                diarize(str(audio), model_dir=str(audio.parent / "missing"))

    def test_speaker_count_validation(self):
        for options in ({"num_speakers": 0}, {"min_speakers": 3, "max_speakers": 2}, {"num_speakers": True}):
            with self.assertRaises(ValueError):
                diarize("unused.wav", **options)


if __name__ == "__main__":
    unittest.main()
