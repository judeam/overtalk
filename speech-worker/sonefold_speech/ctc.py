"""Bounded CTC alignment; token spans are model frames, not word times.

The caller must supply the checkpoint's own token IDs and unpadded log
probabilities. This never changes recognized text or estimates missing words.
"""
from dataclasses import dataclass

import numpy as np


@dataclass(frozen=True)
class TokenSpan:
    token_index: int
    start_frame: int
    end_frame: int
    mean_log_probability: float


def _problem(log_probabilities, tokens, blank, max_cells):
    scores = np.asarray(log_probabilities, dtype=np.float32)
    if scores.ndim != 2 or not scores.shape[0] or not scores.shape[1]:
        raise ValueError("Expected a nonempty frames-by-vocabulary score matrix")
    if np.isnan(scores).any() or np.isposinf(scores).any() or np.any(scores > 1e-5):
        raise ValueError("Expected finite or negative-infinite log probabilities")
    frames, vocabulary = scores.shape
    if isinstance(blank, bool) or not isinstance(blank, int) or not 0 <= blank < vocabulary:
        raise ValueError("Invalid blank token")
    tokens = list(tokens)
    if any(isinstance(t, bool) or not isinstance(t, int) or not 0 <= t < vocabulary or t == blank
           for t in tokens):
        raise ValueError("Target contains blank or invalid token IDs")
    minimum = len(tokens) + sum(a == b for a, b in zip(tokens, tokens[1:]))
    if minimum > frames:
        raise ValueError("Too few model frames to align every target token")
    states = 2 * len(tokens) + 1
    if frames * states > max_cells:
        raise ValueError("Alignment exceeds the bounded traceback budget")
    labels = np.full(states, blank, dtype=np.int64)
    labels[1::2] = tokens
    can_skip = np.zeros(states, dtype=bool)
    can_skip[2:] = (labels[2:] != blank) & (labels[2:] != labels[:-2])
    return scores, tokens, labels, can_skip


def sequence_log_probability(log_probabilities, tokens, *, blank=0, max_cells=8_000_000):
    """Sum all valid CTC paths in log space with O(target length) memory.

    This is an acoustic-model score, not a calibrated correctness probability.
    Scores for different windows/lengths are not directly interchangeable.
    """
    scores, tokens, labels, can_skip = _problem(log_probabilities, tokens, blank, max_cells)
    previous = np.full(len(labels), -np.inf, dtype=np.float32)
    previous[:2] = scores[0, labels[:2]]
    for frame in range(1, len(scores)):
        step = np.concatenate(([-np.inf], previous[:-1]))
        skip = np.concatenate(([-np.inf, -np.inf], previous[:-2]))[:len(labels)]
        total = np.logaddexp(previous, step)
        total = np.logaddexp(total, np.where(can_skip, skip, -np.inf))
        previous = total + scores[frame, labels]
    return float(np.logaddexp(previous[-1], previous[-2]) if tokens else previous[-1])


def align_tokens(log_probabilities, tokens, *, blank=0, max_cells=8_000_000):
    scores, tokens, labels, can_skip = _problem(log_probabilities, tokens, blank, max_cells)
    if not tokens:
        return []
    frames, states = len(scores), len(labels)
    trace = np.zeros((frames, states), dtype=np.uint8)
    previous = np.full(states, -np.inf, dtype=np.float32)
    previous[:2] = scores[0, labels[:2]]
    for frame in range(1, frames):
        best = previous.copy()
        step = np.concatenate(([-np.inf], previous[:-1]))
        better = step > best
        best[better] = step[better]
        trace[frame, better] = 1
        skip = np.concatenate(([-np.inf, -np.inf], previous[:-2]))
        better = can_skip & (skip > best)
        best[better] = skip[better]
        trace[frame, better] = 2
        previous = best + scores[frame, labels]
    state = states - 1 if previous[-1] > previous[-2] else states - 2
    if not np.isfinite(previous[state]):
        raise ValueError("No valid alignment covers every target token")
    starts = np.full(len(tokens), -1, dtype=np.int64)
    ends = starts.copy()
    sums = np.zeros(len(tokens), dtype=np.float64)
    counts = np.zeros(len(tokens), dtype=np.int64)
    for frame in range(frames - 1, -1, -1):
        if state % 2:
            token = state // 2
            starts[token] = frame
            ends[token] = max(ends[token], frame + 1)
            sums[token] += scores[frame, tokens[token]]
            counts[token] += 1
        state -= int(trace[frame, state])
    if np.any(starts < 0) or np.any(counts == 0):
        raise ValueError("Alignment omitted a target token")
    return [TokenSpan(i, int(starts[i]), int(ends[i]), float(sums[i] / counts[i]))
            for i in range(len(tokens))]
