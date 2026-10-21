"""Meeting-cluster uncertainty and worst-group reporting; never a 99% certificate."""
import argparse
from collections import defaultdict
import hashlib
import json
import math
from pathlib import Path


def wilson_interval(successes, total, z=1.959963984540054):
    if total <= 0:
        return None
    if not 0 <= successes <= total:
        raise ValueError('Invalid success count')
    p = successes / total
    divisor = 1 + z * z / total
    centre = (p + z * z / (2 * total)) / divisor
    half = z * math.sqrt(p * (1 - p) / total + z * z / (4 * total * total)) / divisor
    return [max(0., centre - half), min(1., centre + half)]


def ratio_summary(rows, *, seed=731, samples=2000, cluster='id'):
    """Bootstrap entire meetings/scenarios, not correlated individual words."""
    import numpy as np
    groups = defaultdict(lambda: [0., 0.])
    valid = []
    for row in rows:
        numerator, denominator = row['numerator'], row['denominator']
        if (not all(isinstance(x, (float, int)) and math.isfinite(x) for x in (numerator, denominator))
                or numerator < 0 or denominator < 0):
            raise ValueError('Metric counts must be finite and nonnegative')
        if denominator == 0:
            continue
        valid.append(row)
        key = row.get(cluster)
        if key is None:
            key = row['id']
        groups[key][0] += numerator
        groups[key][1] += denominator
    total = sum(row['denominator'] for row in valid)
    errors = sum(row['numerator'] for row in valid)
    passing = sum(row['numerator'] / row['denominator'] <= .01 for row in valid)
    result = dict(cases=len(valid), clusters=len(groups), numerator=errors, denominator=total,
                  rate=errors / total if total else None,
                  macro_rate=sum(row['numerator'] / row['denominator'] for row in valid) / len(valid) if valid else None,
                  bootstrap_95=None, cluster_unit=cluster,
                  meetings_at_or_below_one_percent=passing,
                  meeting_pass_rate_wilson_95=wilson_interval(passing, len(valid)),
                  uncertainty_note='Descriptive uncertainty on this synthetic sample; not population validation.')
    if len(groups) < 5:
        result['bootstrap_note'] = 'Fewer than five clusters; no bootstrap interval reported.'
        return result
    if not 100 <= samples <= 10000:
        raise ValueError('Use 100 to 10000 bootstrap samples')
    values = np.asarray(list(groups.values()), dtype=float)
    rng = np.random.default_rng(seed)
    # Bound memory even when the input grows beyond the present 200 meetings.
    rates = []
    for start in range(0, samples, 128):
        indices = rng.integers(0, len(values), (min(128, samples - start), len(values)))
        totals = values[indices].sum(axis=1)
        rates.extend((totals[:, 0] / totals[:, 1]).tolist())
    result['bootstrap_95'] = np.quantile(rates, [.025, .975]).tolist()
    return result


def paired_difference(before, after, *, seed=731, samples=2000):
    """Paired rate change on the same reference units; negative is better."""
    import numpy as np
    a, b = {r['id']: r for r in before}, {r['id']: r for r in after}
    if len(a) != len(before) or len(b) != len(after):
        raise ValueError('Duplicate meeting IDs')
    common = sorted(a.keys() & b.keys())
    values = []
    for key in common:
        if a[key]['denominator'] != b[key]['denominator']:
            raise ValueError('Paired scores require identical reference counts')
        if a[key]['denominator'] > 0:
            values.append([a[key]['numerator'], b[key]['numerator'], a[key]['denominator']])
    result = dict(cases=len(values), only_before=sorted(a.keys() - b.keys()),
                  only_after=sorted(b.keys() - a.keys()), delta=None, bootstrap_95=None)
    if not values:
        return result
    values = np.asarray(values, dtype=float)
    result['delta'] = float((values[:, 1].sum() - values[:, 0].sum()) / values[:, 2].sum())
    if len(values) >= 5:
        rng = np.random.default_rng(seed)
        deltas = []
        for start in range(0, samples, 128):
            indices = rng.integers(0, len(values), (min(128, samples-start), len(values)))
            totals = values[indices].sum(axis=1)
            deltas.extend(((totals[:, 1] - totals[:, 0]) / totals[:, 2]).tolist())
        result['bootstrap_95'] = np.quantile(deltas, [.025, .975]).tolist()
    return result


def group_keys(row):
    keys = [('all', 'all'), ('difficulty', row.get('difficulty', 'unknown')),
            ('language_set', '+'.join(sorted(row.get('languages', []))) or 'unknown')]
    for field in ('scenario', 'participants', 'duration_band'):
        if row.get(field) is not None:
            keys.append((field, str(row[field])))
    return keys


def build_report(observations, expected=None, *, samples=2000):
    if len({r['id'] for r in observations}) != len(observations):
        raise ValueError('Duplicate observations')
    expected = expected if expected is not None else observations
    if len({r['id'] for r in expected}) != len(expected):
        raise ValueError('Duplicate expected IDs')
    if not {r['id'] for r in observations} <= {r['id'] for r in expected}:
        raise ValueError('Observation outside expected corpus')
    denominators = defaultdict(set)
    for row in expected:
        for key in group_keys(row):
            denominators[key].add(row['id'])
    buckets = defaultdict(list)
    for row in observations:
        for metric, values in row.get('metrics', {}).items():
            for group in group_keys(row):
                buckets[(group, metric)].append(dict(id=row['id'], scenario=row.get('scenario'), **values))
    groups = []
    for (group, metric), rows in sorted(buckets.items()):
        seed = int.from_bytes(hashlib.sha256(repr((group, metric)).encode()).digest()[:8])
        summary = ratio_summary(rows, seed=seed, samples=samples)
        summary['scenario_cluster_uncertainty'] = ratio_summary(rows, seed=seed, samples=samples, cluster='scenario')
        groups.append(dict(dimension=group[0], value=group[1], metric=metric,
                           expected_cases=len(denominators[group]),
                           missing_or_ungraded=sorted(denominators[group] - {r['id'] for r in rows if r['denominator'] > 0}),
                           **summary))
    worst = defaultdict(list)
    for row in groups:
        if row['dimension'] == 'language_set' and row['rate'] is not None:
            worst[row['metric']].append({k: row[k] for k in ('value', 'rate', 'cases', 'bootstrap_95')})
    return dict(expected=len(expected), observed=len(observations),
                missing=sorted({r['id'] for r in expected} - {r['id'] for r in observations}),
                groups=groups,
                worst_language_groups={metric: sorted(rows, key=lambda row: row['rate'], reverse=True)
                                       for metric, rows in worst.items()},
                acceptance_claim=False,
                caveats=['Ratios are error rates; lower is better. WER and CER are never pooled.',
                         'Meeting and scenario cluster intervals are descriptive; shared synthetic voices/scripts limit independence.',
                         'Missing or ungraded results never count as passing.',
                         'The Wilson interval describes the fraction of meetings below 1% error, not token accuracy.'])


def load_observations(results, corpus, *, speaker_scores=None, summary_scores=None):
    expected, observations = [], []
    for path in sorted(corpus.glob('*/reference.json')):
        ref = json.loads(path.read_text())
        meta = {k: ref[k] for k in ('id', 'difficulty', 'languages', 'scenario', 'participants', 'duration_band') if k in ref}
        expected.append(meta)
        result_path = results / ref['id'] / 'result.json'
        if not result_path.exists():
            continue
        result = json.loads(result_path.read_text())
        metrics = {}
        text = result.get('transcript', {})
        if result.get('status') == 'completed':
            if set(ref['languages']) & {'zh', 'yue', 'ja', 'th'}:
                metrics['cer'] = dict(numerator=text['character_errors'], denominator=text['reference_characters'])
            else:
                metrics['wer'] = dict(numerator=text['word_errors'], denominator=text['reference_words'])
            sp = result.get('speakers', {})
            if 'reference_speaker_seconds' in sp:
                metrics['approximate_der'] = dict(numerator=sum(sp.get(k, 0) for k in (
                    'missed_speaker_seconds', 'false_alarm_speaker_seconds', 'confused_speaker_seconds')),
                    denominator=sp['reference_speaker_seconds'])
        if speaker_scores:
            score_path=speaker_scores/f"{ref['id']}.json"
            if score_path.exists():
                score=json.loads(score_path.read_text())
                if score.get('status')=='scored':
                    for field,label in [('standard','timed_joint'),('unassigned_aware','timed_joint_unassigned_aware')]:
                        values=score[field]
                        # Character and word variants remain distinct metrics.
                        metrics[label+'_'+score['metric'].lower()]=dict(numerator=values['errors'],denominator=values['length'])
        if summary_scores:
            for source in ('asr','reference'):
                score_path=summary_scores/f"{ref['id']}-{source}.json"
                if not score_path.exists():continue
                score=json.loads(score_path.read_text())
                p,r=score['precision'],score['recall']
                metrics[source+'_summary_unsupported_or_ungraded']=dict(
                    numerator=p['total_claims']-p['counts'].get('supported',0),denominator=p['total_claims'])
                metrics[source+'_summary_missing_partial_or_ungraded']=dict(
                    numerator=r['total_facts']-r['counts'].get('covered',0),denominator=r['total_facts'])
        observations.append(dict(**meta, metrics=metrics))
    return observations, expected


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--results', type=Path, required=True)
    parser.add_argument('--corpus', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--samples', type=int, default=2000)
    parser.add_argument('--speaker-scores', type=Path)
    parser.add_argument('--summary-scores', type=Path)
    args = parser.parse_args()
    if args.output.exists():
        parser.error('Preserve the earlier report; choose a new output path')
    observations, expected = load_observations(args.results, args.corpus,
        speaker_scores=args.speaker_scores,summary_scores=args.summary_scores)
    report = build_report(observations, expected, samples=args.samples)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2, allow_nan=False))
    print(json.dumps(dict(observed=report['observed'], expected=report['expected'],
                          groups=len(report['groups']), output=str(args.output))))


if __name__ == '__main__':
    main()
