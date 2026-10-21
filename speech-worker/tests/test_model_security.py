import json
from pathlib import Path
import tempfile
import unittest
import io
import tarfile
import importlib.util
from unittest.mock import patch
from sonefold_speech.model_security import validate_checkpoint, validate_nemo_checkpoint, require_versions


class CheckpointSecurityTests(unittest.TestCase):
    def test_old_runtime_is_rejected_before_model_loading(self):
        with patch('importlib.metadata.version', return_value='2.7.1+cu128'):
            with self.assertRaisesRegex(RuntimeError, 'security'):
                require_versions({'torch':'2.13.0'})
        with patch('importlib.metadata.version', return_value='2.14.0'):
            require_versions({'torch':'2.13.0'})
    def test_rejects_nested_remote_code_and_attention_repository(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            for payload in [{"nested": {"trust_remote_code": True}},
                            {"_attn_implementation_internal": "attacker/kernel"},
                            {"custom_generate": "attacker/generation"}]:
                (root / "config.json").write_text(json.dumps(payload))
                with self.assertRaises(ValueError):
                    validate_checkpoint(root)

    def test_shards_cannot_escape_or_open_non_regular_files(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "weights.safetensors").write_bytes(b"synthetic")
            for shard in ["../outside", "/etc/passwd", "C:\\Windows\\win.ini", "weights.safetensors"]:
                (root / "model.safetensors.index.json").write_text(json.dumps({"weight_map": {"weight": shard}}))
                if shard == "weights.safetensors":
                    validate_checkpoint(root)
                else:
                    with self.assertRaises(ValueError):
                        validate_checkpoint(root)

    def test_standard_local_configuration_is_supported(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "config.json").write_text(json.dumps({"model_type": "qwen3_asr", "_attn_implementation_internal": "sdpa"}))
            validate_checkpoint(root)

    @unittest.skipUnless(importlib.util.find_spec("yaml"), "optional NeMo YAML dependency")
    def test_nemo_rejects_executable_targets_resolvers_and_archive_escape(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / 'test.nemo'
            cases = [
                ('model_config.yaml', 'target: os.system\n'),
                ('model_config.yaml', 'target: nemo.collections.asr.models.ASRModel\nsecret: ${oc.env:HOME}\n'),
                ('../model_config.yaml', 'target: nemo.collections.asr.models.ASRModel\n'),
                ('model_config.yaml', 'target: nemo.collections.asr.models.ASRModel\n'),
            ]
            for name, text in cases:
                with tarfile.open(path, 'w') as archive:
                    entry = tarfile.TarInfo(name)
                    entry.size = len(text.encode())
                    archive.addfile(entry, io.BytesIO(text.encode()))
                if name == 'model_config.yaml' and 'secret' not in text and 'os.system' not in text:
                    validate_nemo_checkpoint(path)
                else:
                    with self.assertRaises(ValueError):
                        validate_nemo_checkpoint(path)
