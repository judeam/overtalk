"""Permutation-invariant joint transcription/attribution diagnostics.

Unlike lane counts or assigned-word coverage, a merged speaker is penalized.
This is cpWER/cpCER against synthetic scripts, not validated acoustic DER and
not a measure of speaker-only accuracy. Missing and spurious streams can make
the joint rate exceed 100%, as with ordinary edit-distance error rates.
"""
from __future__ import annotations

import argparse
from collections import defaultdict
import hashlib
import json
from pathlib import Path

from .gemini import atomic_json
from .metrics import CHARACTER_PRIMARY, distance, normalize
from .run_evaluation import RUNTIME


def minimum_assignment(cost):
    """Square integer assignment in O(n^3), without an ML dependency."""
    n = len(cost)
    u, v, p, way = [0] * (n + 1), [0] * (n + 1), [0] * (n + 1), [0] * (n + 1)
    for i in range(1, n + 1):
        p[0] = i
        j0 = 0
        minima, used = [float('inf')] * (n + 1), [False] * (n + 1)
        while True:
            used[j0] = True
            i0, delta, j1 = p[j0], float('inf'), 0
            for j in range(1, n + 1):
                if not used[j]:
                    current = cost[i0 - 1][j - 1] - u[i0] - v[j]
                    if current < minima[j]:
                        minima[j], way[j] = current, j0
                    if minima[j] < delta:
                        delta, j1 = minima[j], j
            for j in range(n + 1):
                if used[j]:
                    u[p[j]] += delta
                    v[j] -= delta
                else:
                    minima[j] -= delta
            j0 = j1
            if p[j0] == 0:
                break
        while j0:
            j1 = way[j0]
            p[j0] = p[j1]
            j0 = j1
    return [(p[j] - 1, j - 1) for j in range(1, n + 1)]


def speaker_attributed_error(reference, predicted, *, characters=False):
    def tokens(text):
        value = normalize(text)
        return list(value.replace(' ', '')) if characters else value.split()

    groups = []
    unknown = 0
    for is_prediction, rows in enumerate((reference, predicted)):
        streams = defaultdict(list)
        for speaker, text in rows:
            units = tokens(text)
            if is_prediction and not speaker:
                unknown += len(units)
            else:
                streams[speaker].extend(units)
        groups.append(streams)
    ref, hyp = groups
    if len(ref) > 64 or len(hyp) > 64:
        raise ValueError('Speaker diagnostic supports at most 64 identities per side')
    ref_ids, hyp_ids = sorted(ref), sorted(hyp)
    n = max(len(ref), len(hyp))
    cost = [[distance(ref[ref_ids[i]] if i < len(ref_ids) else [],
                      hyp[hyp_ids[j]] if j < len(hyp_ids) else [])
             for j in range(n)] for i in range(n)]
    pairs = minimum_assignment(cost)
    errors = sum(cost[i][j] for i, j in pairs) + unknown
    total = sum(len(x) for x in ref.values())
    return dict(metric='cpCER' if characters else 'cpWER', errors=errors,
                reference_units=total, rate=errors / total if total else None,
                unassigned_units=unknown, reference_speakers=len(ref), predicted_speakers=len(hyp),
                mapping={hyp_ids[j]: ref_ids[i] for i, j in pairs
                         if i < len(ref_ids) and j < len(hyp_ids)})


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    if args.output.exists():
        parser.error('Choose a new output directory; earlier diagnostics remain immutable')
    args.output.mkdir(parents=True)
    scores = []
    for path in sorted((RUNTIME / 'results').glob('*/result.json')):
        result = json.loads(path.read_text())
        if result['status'] != 'completed':
            continue
        ref = json.loads((RUNTIME / 'corpus' / result['id'] / 'reference.json').read_text())
        hyp = json.loads((path.parent / 'transcript.json').read_text())
        score = speaker_attributed_error(
            [(t['speaker'], t['text']) for t in ref['turns']],
            [(t.get('speaker_id'), t['text']) for t in hyp['rows']],
            characters=bool(set(ref['languages']) & CHARACTER_PRIMARY))
        score.update(id=result['id'], difficulty=result['difficulty'], languages=result['languages'])
        score['input_sha256'] = {
            'reference': hashlib.sha256((RUNTIME/'corpus'/result['id']/'reference.json').read_bytes()).hexdigest(),
            'transcript': hashlib.sha256((path.parent/'transcript.json').read_bytes()).hexdigest(),
        }
        atomic_json(args.output / f'{result["id"]}.json', score)
        scores.append(score)
    atomic_json(args.output / 'summary.json', dict(cases=len(scores), scores=scores,
        caveats=['Joint speaker-attributed transcription metric, not speaker-only accuracy.',
                 'Synthetic reference words have not been independently audio-validated.',
                 'True IDs are used only for scoring a single global permutation, never as model inputs.']))
    print(json.dumps(dict(cases=len(scores), output=str(args.output))))


if __name__ == '__main__':
    main()
