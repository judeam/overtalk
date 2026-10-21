import unittest
from sonefold_speech.asr import SpeechError, normalize_language
from sonefold_speech.modern_asr import validate_moss
class ModernOutputTests(unittest.TestCase):
    def test_joint_output_preserves_overlapping_speakers_and_text(self):
        result=validate_moss('[0.0][S01]Ja, um, machen wir.[2.0]\n[1.0][S02]Okay.[2.5]',3)
        self.assertEqual([s['speaker'] for s in result],['S01','S02'])
        self.assertEqual(result[0]['text'],'Ja, um, machen wir.')
        self.assertEqual(result[1]['start'],1)
    def test_malformed_or_dropped_output_is_rejected(self):
        for raw in ['broken [0][S01]yes[1]', '[0][S01]truncated', '[1][S01]backwards[0]', '[0][S01]out of range[100]']:
            with self.assertRaises(SpeechError):validate_moss(raw,3)
    def test_silence_is_empty_not_invented(self):
        self.assertEqual(validate_moss(' ',3),[])
    def test_cohere_requires_supported_forced_language(self):
        self.assertEqual(normalize_language('German','cohere-transcribe'),'de')
        for value in ['auto','sv']:
            with self.assertRaises(SpeechError):normalize_language(value,'cohere-transcribe')
