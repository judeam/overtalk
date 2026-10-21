import json
from pathlib import Path
import tempfile
import unittest

from .install_project import atomic_json, load_cases, segments


class ProjectImportTests(unittest.TestCase):
    def test_preserves_model_ambiguity_timestamps_and_names(self):
        rows = segments({"speakers": [{"id": "SPEAKER_00", "name": "Speaker 1"}], "rows": [
            {"text": "Unclear", "audio_start_time": 3661.25, "audio_end_time": 3662.0,
             "speaker_id": None, "overlap": True},
            {"text": "Hello", "audio_start_time": 3661.25, "audio_end_time": 3663.0,
             "speaker_id": "SPEAKER_00", "speaker_confidence": 0.8}]})
        self.assertIsNone(rows[0]["speaker_id"])
        self.assertTrue(rows[0]["overlap"])
        self.assertEqual(rows[0]["timestamp"], "01:01:01")
        self.assertEqual(rows[1]["speaker_name"], "Speaker 1")
        self.assertEqual(rows[1]["speaker_confidence"], 0.8)
        self.assertEqual([r["text"] for r in rows], ["Unclear", "Hello"])

    def test_rejects_incomplete_or_duplicate_manifest_before_writing(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            for manifest in ([], [{"id": "same"}] * 200):
                (root / "manifest.json").write_text(json.dumps(manifest))
                with self.assertRaises(ValueError):
                    load_cases(root)
            atomic_json(root / "marker.json", {"complete": True})
            self.assertTrue(json.loads((root / "marker.json").read_text())["complete"])
            self.assertFalse((root / "marker.json.tmp").exists())


if __name__ == "__main__":
    unittest.main()
