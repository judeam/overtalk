"""Time-constrained speaker-attributed scoring with pinned MeetEval.

Reference bounds are synthetic clip intervals, not human word boundaries.
Keep that limitation alongside every score. No reference label enters inference.
"""
import argparse
import dataclasses
import hashlib
import importlib.metadata
import json
import math
from pathlib import Path
import time

from .metrics import CHARACTER_PRIMARY, normalize
from .gemini import atomic_json


def segments(rows, session, *, characters=False):
    output, unknown = [], 0
    for row in rows:
        start = row.get('start', row.get('start_seconds', row.get('audio_start_time')))
        end = row.get('end', row.get('end_seconds', row.get('audio_end_time')))
        if (isinstance(start, bool) or isinstance(end, bool)
                or not isinstance(start, (float, int)) or not isinstance(end, (float, int))
                or not math.isfinite(start) or not math.isfinite(end) or start < 0 or end <= start):
            raise ValueError('Every scored passage needs valid original time bounds')
        words = normalize(row['text'])
        if characters:
            words = ' '.join(words.replace(' ', ''))
        if not words:
            continue
        speaker = row.get('speaker_id', row.get('speaker'))
        if not speaker:
            unknown += len(words.split())
            speaker = '__unassigned__'
        output.append(dict(session_id=session, speaker=speaker, start_time=start,
                           end_time=end, words=words))
    return output, unknown


def score(reference, predicted, *, session='meeting', characters=False, collar=5.):
    import meeteval
    if not math.isfinite(collar) or not 0 <= collar <= 10:
        raise ValueError('Declare a collar between zero and ten seconds')
    ref, ref_unknown = segments(reference, session, characters=characters)
    hyp, unknown = segments(predicted, session, characters=characters)
    if ref_unknown:
        raise ValueError('Reference speakers must be annotated')
    if not ref:
        raise ValueError('Reference has no scored speech')
    if not hyp:
        hyp = [dict(session_id=session, speaker='__empty__', start_time=0., end_time=1., words='')]
    result = meeteval.wer.tcpwer(ref, hyp, collar=collar)[session]
    # Standard tcpWER permits an anonymous stream to match a reference voice.
    # Report it, but also expose a stricter score that cannot reward unassigned
    # words as though the system identified their speaker.
    assigned = [row for row in hyp if row['speaker'] != '__unassigned__']
    if not assigned:
        assigned = [dict(session_id=session, speaker='__empty__', start_time=0., end_time=1., words='')]
    strict = meeteval.wer.tcpwer(ref, assigned, collar=collar)[session]
    standard, attributed = dataclasses.asdict(result), dataclasses.asdict(strict)
    attributed['errors'] += unknown
    attributed['insertions'] += unknown
    attributed['error_rate'] = attributed['errors'] / attributed['length']
    return dict(metric='tcpCER' if characters else 'tcpWER', standard=standard,
                unassigned_aware=attributed, unassigned_units=unknown, collar_seconds=collar,
                reference_pseudo_word_timing='character_based',
                hypothesis_pseudo_word_timing='character_based_points',
                unassigned_policy='Unassigned words cannot match a reference voice; count as insertions in addition to missed reference words.',
                timing_status='synthetic clip / predicted passage bounds; not independently validated word timing',
                acceptance_claim=False)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--results', type=Path, required=True)
    parser.add_argument('--corpus', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--collar', type=float, default=5.)
    args = parser.parse_args()
    if args.output.exists():
        parser.error('Choose a new output directory')
    version = importlib.metadata.version('meeteval')
    if version != '0.4.3':
        parser.error('Use the pinned evaluation dependency meeteval==0.4.3')
    args.output.mkdir(parents=True)
    atomic_json(args.output / 'configuration.json', dict(meeteval=version, collar=args.collar,
        script_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
        source='https://github.com/fgnt/meeteval', acceptance_claim=False))
    for path in sorted(args.results.glob('*/result.json')):
        started = time.perf_counter()
        result = json.loads(path.read_text())
        key = result['id']
        ref_path = args.corpus / key / 'reference.json'
        hyp_path = path.parent / 'transcript.json'
        try:
            if result.get('status') != 'completed':
                raise ValueError('Processing did not complete')
            reference = json.loads(ref_path.read_text())
            predicted = json.loads(hyp_path.read_text())
            value = score(reference['turns'], predicted['rows'], session=key,
                          characters=bool(set(reference['languages']) & CHARACTER_PRIMARY), collar=args.collar)
            value.update(status='scored', id=key, difficulty=result['difficulty'], languages=result['languages'],
                         input_sha256={p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in [ref_path, hyp_path]})
        except Exception as error:
            value = dict(id=key, status='ungraded', error=str(error))
        value['seconds'] = time.perf_counter() - started
        atomic_json(args.output / f'{key}.json', value)
        print(json.dumps({k: value[k] for k in ['id', 'status', 'seconds']}), flush=True)


if __name__ == '__main__':
    main()
