import tempfile
import os
from pathlib import Path
import unittest
from unittest.mock import patch

from sonefold_speech.asr import ASREngine
from sonefold_speech.omni_alignment import OmniAlignment, normalized_alignment_text, transformer_key, unspaced_character


class OmniTests(unittest.TestCase):
    def test_multilingual_normalization_keeps_letters_numbers_and_marks(self):
        self.assertEqual(normalized_alignment_text("हाँ, 4,800! Được rồi."), "हाँ 4 800 được rồi")
        self.assertEqual(normalized_alignment_text("中文。日本語！ภาษาไทย"), "中文 日本語 ภาษาไทย")

    def test_alignment_normalizes_thai_sara_am_and_optional_arabic_vowels(self):
        self.assertEqual(normalized_alignment_text("ทำ"), "ท\u0e4dา")
        self.assertEqual(normalized_alignment_text("شكراً"), "شكرا")

    def test_apostrophes_in_turkish_suffixes_and_contractions_are_retained(self):
        self.assertEqual(normalized_alignment_text("Haziran'dı. John's can't."), "haziran'dı john's can't")

    def test_unspaced_scripts_do_not_turn_an_entire_sentence_into_one_word(self):
        self.assertTrue(all(unspaced_character(char) for char in "วัน中あア𠀀"))
        self.assertFalse(any(unspaced_character(char) for char in "ABC3ह한"))

    def test_mapping_covers_distinct_attention_and_final_norms(self):
        cases = {
            "encoder.layers.1.self_attn.output_proj.weight": "wav2vec2.encoder.layers.1.attention.out_proj.weight",
            "encoder.layers.1.self_attn_layer_norm.weight": "wav2vec2.encoder.layers.1.layer_norm.weight",
            "encoder.layers.1.ffn_layer_norm.weight": "wav2vec2.encoder.layers.1.final_layer_norm.weight",
            "encoder_frontend.pos_encoder.conv.weight_g": "wav2vec2.encoder.pos_conv_embed.conv.parametrizations.weight.original0",
            "final_proj.weight": "lm_head.weight",
        }
        for source, expected in cases.items():
            self.assertEqual(transformer_key(source), expected)
        with self.assertRaises(ValueError):
            transformer_key("unrecognized.layer.weight")

    def test_changed_checkpoint_is_rejected_before_loading_torch(self):
        with tempfile.TemporaryDirectory() as folder:
            (Path(folder) / "omniASR-CTC-300M.pt").write_bytes(b"wrong checkpoint")
            with self.assertRaisesRegex(ValueError, "checksum"):
                OmniAlignment(Path(folder))

    def test_unload_also_releases_optional_cpu_model(self):
        engine = ASREngine("cpu")
        engine._omni_aligner = object()
        engine._omni_checked = True
        engine.unload()
        self.assertIsNone(engine._omni_aligner)
        self.assertFalse(engine._omni_checked)

    def test_optional_model_is_not_loaded_without_configuration(self):
        with patch.dict(os.environ, {}, clear=True), patch("sonefold_speech.omni_alignment.OmniAlignment") as loader:
            self.assertEqual(ASREngine("cpu")._multilingual_words([], "hello", 0, 1, "English"), [])
        loader.assert_not_called()

    def test_bad_optional_install_only_attempts_setup_once(self):
        engine = ASREngine("cpu")
        with patch.dict(os.environ, {"SONEFOLD_OMNI_ALIGNER_DIR": "/invalid/checkpoint"}), \
                patch("sonefold_speech.omni_alignment.OmniAlignment", side_effect=ValueError("checksum")) as loader:
            for _ in range(2):
                self.assertEqual(engine._multilingual_words([], "hello", 0, 1, "English"), [])
        loader.assert_called_once()


if __name__ == "__main__":
    unittest.main()
