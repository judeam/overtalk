import itertools
import unittest

import numpy as np

from .ctc_alignment import align_tokens


def collapse(path, blank=0):
    return [token for token, _ in itertools.groupby(path) if token != blank]


class AlignmentTests(unittest.TestCase):
    def test_forward_score_sums_all_valid_paths(self):
        from .ctc_alignment import sequence_log_probability
        rng = np.random.default_rng(41)
        probabilities = rng.dirichlet(np.ones(3), size=5).astype(np.float32)
        for tokens in ([], [1], [1, 1], [1, 2], [2, 1, 2]):
            total = sum(float(np.prod([probabilities[i, token] for i, token in enumerate(path)]))
                        for path in itertools.product(range(3), repeat=5)
                        if collapse(path) == tokens)
            self.assertAlmostEqual(sequence_log_probability(np.log(probabilities), tokens),
                                   float(np.log(total)), places=5)

    def test_known_path_keeps_repeated_tokens_separated_by_blank(self):
        values = np.full((7, 3), -20, dtype=np.float32)
        for frame, token in enumerate([0, 1, 1, 0, 1, 2, 0]):
            values[frame, token] = 0
        spans = align_tokens(values, [1, 1, 2])
        self.assertEqual([(s.start_frame, s.end_frame) for s in spans], [(1, 3), (4, 5), (5, 6)])

    def test_dynamic_program_matches_exhaustive_ctc_paths(self):
        rng = np.random.default_rng(6)
        for tokens in ([1], [1, 2], [1, 1], [2, 1, 2]):
            probabilities = rng.dirichlet(np.ones(3), size=5).astype(np.float32)
            scores = np.log(probabilities)
            candidates = [(sum(scores[t, token] for t, token in enumerate(path)), path)
                          for path in itertools.product(range(3), repeat=5) if collapse(path) == list(tokens)]
            _, expected = max(candidates)
            expected_frames = [[] for _ in tokens]
            index, previous = -1, None
            for frame, token in enumerate(expected):
                if token != previous and token:
                    index += 1
                if token:
                    expected_frames[index].append(frame)
                previous = token
            actual = align_tokens(scores, tokens)
            self.assertEqual([(s.start_frame, s.end_frame) for s in actual],
                             [(min(f), max(f) + 1) for f in expected_frames])

    def test_invalid_input_and_unbounded_work_fail_explicitly(self):
        scores = np.log(np.full((5, 3), 1 / 3, dtype=np.float32))
        for tokens in ([0], [-1], [3], [True]):
            with self.subTest(tokens=tokens), self.assertRaises(ValueError):
                align_tokens(scores, tokens)
        with self.assertRaisesRegex(ValueError, "Too few"):
            align_tokens(scores[:2], [1, 1])
        with self.assertRaisesRegex(ValueError, "budget"):
            align_tokens(scores, [1, 2], max_cells=20)
        with self.assertRaises(ValueError):
            align_tokens(np.array([[np.nan, -1]]), [1])
        with self.assertRaisesRegex(ValueError, "No valid"):
            align_tokens(np.full((5, 3), -np.inf), [1])


if __name__ == "__main__":
    unittest.main()
