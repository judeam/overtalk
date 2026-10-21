import unittest

from .probe_acoustic_candidates import choose_candidate, group_partitions


class CandidateTests(unittest.TestCase):
    def test_partition_grouping_rejects_missing_or_duplicate_audio(self):
        parents = [dict(start=0, end=10), dict(start=10, end=20)]
        parts = [dict(start=0, end=4, text='one'), dict(start=4, end=10, text='two'),
                 dict(start=10, end=20, text='three')]
        self.assertEqual(group_partitions(parents, parts), ['one two', 'three'])
        for broken in [parts[1:], parts + [parts[-1]],
                       [parts[0], dict(start=3, end=10, text='overlap'), parts[2]]]:
            with self.assertRaises(ValueError):
                group_partitions(parents, broken)

    def test_selection_abstains_without_comparable_acoustic_evidence(self):
        texts = dict(uniform='ten apples', context='two apples', speaker_bounded='apples')
        self.assertEqual(choose_candidate(texts, dict(context=-1)), 'uniform')
        self.assertEqual(choose_candidate(texts, dict(uniform=-20, context=-19.5)), 'uniform')
        self.assertEqual(choose_candidate(texts, dict(uniform=-20, speaker_bounded=-1)), 'uniform')
        self.assertEqual(choose_candidate(texts, dict(uniform=-20, context=-15)), 'context')


if __name__ == '__main__':
    unittest.main()
