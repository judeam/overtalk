import unittest
import itertools
import random

from .speaker_attribution import minimum_assignment, speaker_attributed_error


class SpeakerAttributionTests(unittest.TestCase):
    def test_assignment_matches_exhaustive_independent_search(self):
        randomizer = random.Random(731)
        for size in range(1, 6):
            for _ in range(20):
                costs = [[randomizer.randrange(30) for _ in range(size)] for _ in range(size)]
                expected = min(sum(costs[i][j] for i, j in enumerate(p))
                               for p in itertools.permutations(range(size)))
                actual = sum(costs[i][j] for i, j in minimum_assignment(costs))
                self.assertEqual(actual, expected)

    def test_identity_names_and_cross_talk_order_do_not_matter(self):
        reference = [('A', 'yes ship it'), ('B', 'no wait')]
        predicted = [('speaker2', 'no wait'), ('speaker1', 'yes ship it')]
        score = speaker_attributed_error(reference, predicted)
        self.assertEqual(score['errors'], 0)
        self.assertEqual(score['mapping'], {'speaker1': 'A', 'speaker2': 'B'})

    def test_collapsed_speakers_are_not_counted_as_correct(self):
        score = speaker_attributed_error([('A', 'yes ship'), ('B', 'no wait')],
                                         [('one', 'yes ship no wait')])
        self.assertEqual(score['errors'], 4)
        self.assertEqual(score['reference_units'], 4)

    def test_unassigned_words_cannot_be_permuted_into_a_real_identity(self):
        score = speaker_attributed_error([('A', 'hello')], [(None, 'hello')])
        self.assertEqual(score['unassigned_units'], 1)
        self.assertEqual(score['errors'], 2)
        self.assertEqual(score['mapping'], {})

    def test_global_assignment_is_one_to_one_not_greedy(self):
        reference = [('A', 'red red blue blue'), ('B', 'red red red')]
        predicted = [('X', 'red red red'), ('Y', 'blue blue')]
        score = speaker_attributed_error(reference, predicted)
        self.assertEqual(score['mapping'], {'X': 'B', 'Y': 'A'})
        self.assertEqual(score['errors'], 2)

    def test_unspaced_language_uses_characters(self):
        score = speaker_attributed_error([('A', '不能发布')], [('X', '不能')], characters=True)
        self.assertEqual(score['reference_units'], 4)
        self.assertEqual(score['errors'], 2)

    def test_extra_speaker_and_silent_reference_retain_insertions(self):
        score = speaker_attributed_error([], [('X', 'phantom words')])
        self.assertEqual(score['errors'], 2)
        self.assertIsNone(score['rate'])


if __name__ == '__main__':
    unittest.main()
