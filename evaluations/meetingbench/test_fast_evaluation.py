"""Exercise the staged runner and its cache boundaries without model inference."""
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from . import fast_evaluation as runner


class FastRunnerTests(unittest.TestCase):
    def test_uncertain_verdicts_complete_work_but_missing_grades_do_not(self):
        report=dict(precision=dict(counts={'insufficient':2}),recall=dict(counts={'unverified':2}))
        self.assertTrue(runner.summary_work_completed([report],1))
        self.assertFalse(runner.summary_work_completed([report],2))
        report['recall']['counts']['ungraded']=1
        self.assertFalse(runner.summary_work_completed([report],1))

    def test_both_summary_controls_resume_and_hash_changed_audio_is_rejected(self):
        with tempfile.TemporaryDirectory() as folder:
            root=Path(folder); runtime=root/'runtime'; home=root/'home'; out=root/'out'
            config=home/'.local/share/com.judeam.sonefold';config.mkdir(parents=True)
            (config/'speech-runtime.json').write_text(json.dumps(dict(diarization_python='nemo-python',
                diarization_model_path='model.nemo',asr_model_dir='asr',aligner_model_dir='aligner')))
            runtime.mkdir()
            install=root/'install';(install/'bin').mkdir(parents=True)
            (install/'bin/llama-helper').write_bytes(b'fixed helper')
            (runtime/'evaluation-install.json').write_text(json.dumps(dict(path=str(install))))
            audio=root/'audio.wav';audio.write_bytes(b'fixed audio')
            native=root/'native';native.write_bytes(b'fixed native runner')
            corpus=runtime/'corpus'/'normal-001';corpus.mkdir(parents=True)
            ref=dict(id='normal-001',difficulty='normal',languages=['en'],duration_seconds=1.,
                script_sha256='script',audio_path=str(audio),people=[dict(id='P1')],
                turns=[dict(speaker='P1',speaker_name='Alice',text='hello',start_seconds=0.,end_seconds=1.)],
                facts=[],scenario='test',participants=1,duration_band='2-4')
            (corpus/'reference.json').write_text(json.dumps(ref))
            old=runtime/'results'/'normal-001';old.mkdir(parents=True)
            baseline=dict(id='normal-001',status='completed',languages=['en'],difficulty='normal',
                          transcript=dict(word_errors=0,reference_words=1),timing=dict(total_seconds=100.))
            (old/'result.json').write_text(json.dumps(baseline))
            native_calls=[]
            def subprocess_run(command,**kwargs):
                if command[0]=='nemo-python':
                    target=Path(command[command.index('--output')+1]);target.mkdir(exist_ok=True)
                    (target/'normal-001.json').write_text(json.dumps(dict(status='completed',seconds=.1,
                        turns=[dict(speaker='S1',start=0.,end=1.)],speakers=[dict(id='S1')])) )
                else:
                    env=kwargs['env'];target=Path(env['OVERTALK_EVAL_OUT']);target.mkdir(exist_ok=True)
                    names=env['OVERTALK_EVAL_MEETINGS'].split(',');native_calls.append(names)
                    for name in names:
                        p=target/f'{name}__qwen3.5-4b__app.md';p.write_text('# Meeting notes\nHello [S1]')
                        p.with_suffix('.json').write_text('{"seconds":0.1}')
            class Engine:
                def __init__(self,**kwargs):pass
                def unload(self):pass
                def transcribe_batched(self,*args,**kwargs):
                    return dict(segments=[dict(text='hello',speaker=None,start=0.,end=1.)],text='hello',
                                language='English',warnings=[]),dict(total_seconds=.1)
            argv=['fast_evaluation','--output',str(out),'--native-runner',str(native),
                  '--notes-workers','1','--no-quality','--no-score']
            with patch.object(runner,'RUNTIME',runtime),patch('pathlib.Path.home',return_value=home), \
                 patch.object(runner,'model_artifact_hashes',return_value={'model':'unchanged'}), \
                 patch.object(runner.subprocess,'run',side_effect=subprocess_run), \
                 patch('evaluations.meetingbench.throughput_asr.ThroughputEngine',Engine):
                with patch('sys.argv',argv):runner.main()
                self.assertEqual(native_calls,[['normal-001-asr','normal-001-reference']])
                self.assertEqual(json.loads((out/'results/normal-001/result.json').read_text())['status'],'completed')
                with patch('sys.argv',argv+['--resume']):runner.main()
                self.assertEqual(len(native_calls),1)
                self.assertIsNone(json.loads((out/'timing.json').read_text())['observed_speedup'])
                audio.write_bytes(b'different audio')
                with patch('sys.argv',argv+['--resume']),self.assertRaisesRegex(ValueError,'configuration changed'):
                    runner.main()
