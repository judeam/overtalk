import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from .vllm_setup import local_engine_options, tokenizer_directory


class TokenizerSetupTests(unittest.TestCase):
    def test_local_options_use_loopback_and_corrected_processor_without_full_graphs(self):
        with tempfile.TemporaryDirectory() as folder,patch.dict('os.environ',{},clear=True):
            model=Path(folder)/'model';model.mkdir()
            (model/'config.json').write_text('{"model_type":"qwen3_asr"}')
            (model/'tokenizer_config.json').write_text('{"original":true}')
            view=Path(folder)/'tokenizer';view.mkdir()
            (view/'tokenizer_config.json').write_text('{"fix_mistral_regex":true}')
            import os
            with patch('evaluations.meetingbench.vllm_setup.tokenizer_directory',return_value=view):
                options=local_engine_options(model)
                self.assertNotIn('compilation_config',local_engine_options(model,eager=True))
            self.assertEqual(options['tokenizer'],str(view))
            self.assertEqual(options['model'],str(view))
            self.assertTrue((view/'config.json').is_symlink())
            self.assertEqual((model/'tokenizer_config.json').read_text(),'{"original":true}')
            self.assertEqual(options['compilation_config'],{'cudagraph_mode':'PIECEWISE'})
            self.assertEqual(os.environ['VLLM_HOST_IP'],'127.0.0.1')
            self.assertEqual(os.environ['VLLM_NO_USAGE_STATS'],'1')

    def test_cache_preserves_qwen_flag_and_changes_with_source_without_editing_model(self):
        with tempfile.TemporaryDirectory() as folder:
            root=Path(folder);model=root/'model';model.mkdir()
            config=model/'tokenizer_config.json';config.write_text('{"example":1}')
            calls=[]
            class Tokenizer:
                def save_pretrained(self,path):
                    (path/'tokenizer_config.json').write_text('{}')
            def factory(path,**kwargs):
                calls.append(kwargs);return Tokenizer()
            a=tokenizer_directory(model,root/'cache',factory)
            self.assertTrue(json.loads((a/'tokenizer_config.json').read_text())['fix_mistral_regex'])
            self.assertEqual(tokenizer_directory(model,root/'cache',factory),a)
            self.assertEqual(len(calls),1)
            self.assertTrue(calls[0]['local_files_only'])
            self.assertEqual(config.read_text(),'{"example":1}')
            config.write_text('{"example":2}')
            self.assertNotEqual(tokenizer_directory(model,root/'cache',factory),a)


if __name__=='__main__':unittest.main()
