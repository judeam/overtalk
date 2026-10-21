import importlib.util
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from sonefold_speech.nemo_runtime import prepare

spec = importlib.util.spec_from_file_location('patch_nemo_runtime', Path(__file__).parents[1] / 'patch_nemo_runtime.py')
patcher = importlib.util.module_from_spec(spec)
spec.loader.exec_module(patcher)


class NeMoSecurityTests(unittest.TestCase):
    def test_inference_overrides_unsafe_parent_environment(self):
        with patch.dict(os.environ, {'TORCH_FORCE_NO_WEIGHTS_ONLY_LOAD': '1'}, clear=True), patch('sonefold_speech.model_security.require_versions'):
            prepare()
            self.assertNotIn('TORCH_FORCE_NO_WEIGHTS_ONLY_LOAD', os.environ)
            self.assertEqual(os.environ['TORCH_FORCE_WEIGHTS_ONLY_LOAD'], '1')
            self.assertEqual(os.environ['OVERTALK_INFERENCE_ONLY'], '1')
            self.assertEqual(os.environ['HF_HUB_OFFLINE'], '1')

    def test_patch_is_idempotent_and_rejects_changed_upstream_before_writing(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            files = {
                'collections/asr/modules/transformer_encoder_utils.py': 'torch.compile(flex_attention, dynamic=True)\n    return _flex_attention_compiled if x.is_cuda else flex_attention\n',
                'utils/exp_manager.py': 'from lightning.pytorch.loggers import MLFlowLogger, NeptuneLogger, TensorBoardLogger, WandbLogger\n',
                'lightning/callback_group.py': 'from nemo.lightning.one_logger_callback import OneLoggerNeMoCallback\nself._callbacks: List[BaseCallback] = [OneLoggerNeMoCallback()]\n',
            }
            for name, content in files.items():
                path = root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(content)
            patcher.patch(root)
            once = {p: (root / p).read_text() for p in files}
            patcher.patch(root)
            self.assertEqual(once, {p: (root / p).read_text() for p in files})
            changed = root / 'utils/exp_manager.py'
            changed.write_text('new upstream interface')
            before = {p: (root / p).read_text() for p in files}
            with self.assertRaises(RuntimeError):
                patcher.patch(root)
            self.assertEqual(before, {p: (root / p).read_text() for p in files})


@unittest.skipUnless(importlib.util.find_spec('transformers') and importlib.util.find_spec('nagisa'), 'optional Qwen runtime not installed')
class QwenCompatibilityTests(unittest.TestCase):
    def test_positional_constant_survives_empty_materialization(self):
        import torch
        from sonefold_speech.vendor.qwen_asr.core.transformers_backend.modeling_qwen3_asr import SinusoidsPositionEmbedding
        module = SinusoidsPositionEmbedding(20, 16)
        expected = module.positional_embedding.clone()
        module.positional_embedding.zero_()
        module.restore_constant()
        torch.testing.assert_close(module.positional_embedding, expected)

    def test_remote_audio_is_never_fetched(self):
        from sonefold_speech.vendor.qwen_asr.inference.utils import load_audio_any
        with self.assertRaisesRegex(ValueError, 'network audio is disabled'):
            load_audio_any('https://example.invalid/audio.wav')

    def test_korean_alignment_dictionary_is_packaged(self):
        from sonefold_speech.vendor.qwen_asr.inference.qwen3_forced_aligner import Qwen3ForceAlignProcessor
        self.assertTrue(Qwen3ForceAlignProcessor().ko_score)
