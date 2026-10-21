import importlib.util
import unittest

from .timed_speaker_metrics import score, segments


class TimedInputTests(unittest.TestCase):
    def test_invalid_times_cannot_be_replaced_with_invented_word_timing(self):
        for start, end in [(None, 1), (0, None), (2, 1), (0, float('nan')), (-1, 1)]:
            with self.subTest(start=start, end=end), self.assertRaises(ValueError):
                segments([dict(start=start, end=end, speaker='A', text='hello')], 'x')

    def test_unspaced_text_and_unknown_speakers_remain_visible(self):
        rows, count = segments([dict(start=0, end=1, speaker=None, text='不能发布')], 'x', characters=True)
        self.assertEqual(count, 4)
        self.assertEqual(rows[0]['words'], '不 能 发 布')


@unittest.skipUnless(importlib.util.find_spec('meeteval'), 'Install the separate metrics environment')
class TimedScoreTests(unittest.TestCase):
    def row(self, speaker, text, start=0, end=1):
        return dict(speaker=speaker, text=text, start=start, end=end)

    def test_anonymous_labels_can_be_permuted_but_wrong_timing_cannot(self):
        ref = [self.row('A', 'hello'), self.row('A', 'world', 20, 21)]
        good = [self.row('X', 'hello'), self.row('X', 'world', 20, 21)]
        bad = [self.row('X', 'hello world')]
        self.assertEqual(score(ref, good, collar=1)['standard']['errors'], 0)
        self.assertGreater(score(ref, bad, collar=1)['standard']['errors'], 0)

    def test_unassigned_words_never_gain_identity_credit(self):
        ref = [self.row('A', 'hello')]
        result = score(ref, [self.row(None, 'hello')])
        self.assertEqual(result['standard']['errors'], 0)
        self.assertEqual(result['unassigned_aware']['errors'], 2)

    def test_one_global_mapping_penalizes_identity_switches(self):
        ref = [self.row('A', 'first'), self.row('B', 'second', 3, 4),
               self.row('A', 'third', 6, 7), self.row('B', 'fourth', 9, 10)]
        hyp = [self.row('X', 'first'), self.row('Y', 'second', 3, 4),
               self.row('Y', 'third', 6, 7), self.row('X', 'fourth', 9, 10)]
        self.assertGreater(score(ref, hyp, collar=.25)['standard']['errors'], 0)


if __name__ == '__main__':
    unittest.main()
