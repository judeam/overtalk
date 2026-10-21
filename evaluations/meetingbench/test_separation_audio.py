from pathlib import Path
import tempfile
import unittest

import numpy as np
import soundfile as sf

from .probe_separation import write_separation_audio


class SeparationAudioTests(unittest.TestCase):
    def test_high_gain_output_keeps_waveform_shape_and_float_encoding(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            waveform = np.array([[40., 2.], [-20., 1.], [4., 0.]], dtype=np.float32)
            mixture = np.array([.4, -.2, .04], dtype=np.float32)
            write_separation_audio(root, 'case', mixture, waveform, [1, 0])
            first, _ = sf.read(root/'case-source-0.wav')
            second, _ = sf.read(root/'case-source-1.wav')
            np.testing.assert_allclose(first, [1., .5, 0.])
            np.testing.assert_allclose(second, [1., -.5, .1], rtol=1e-6)
            self.assertEqual(sf.info(root/'case-source-0.wav').subtype, 'FLOAT')
            recovered, _ = sf.read(root/'case-mixture.wav')
            np.testing.assert_array_equal(recovered, mixture)

    def test_silent_channel_is_finite_and_invalid_audio_is_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            waveform = np.zeros((4, 2), dtype=np.float32)
            write_separation_audio(root, 'silence', np.zeros(4), waveform, [0, 1])
            output, _ = sf.read(root/'silence-source-0.wav')
            self.assertTrue(np.isfinite(output).all())
            self.assertFalse(output.any())
            waveform[0, 0] = np.nan
            with self.assertRaises(ValueError):
                write_separation_audio(root, 'invalid', np.zeros(4), waveform, [0, 1])


if __name__ == '__main__':
    unittest.main()
