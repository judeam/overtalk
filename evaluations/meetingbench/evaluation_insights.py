"""Compare frozen evaluation outputs without conflating speed and quality."""
from pathlib import Path
import json

from .metrics import CHARACTER_PRIMARY
from .statistics_report import paired_difference


def speed_comparison(baseline_seconds, elapsed_seconds, *, baseline_cases, candidate_cases, resumed=False,
                     completed_cases=None, validation_complete=True):
    if elapsed_seconds <= 0 or baseline_seconds < 0:
        raise ValueError('Invalid elapsed measurement')
    measured = baseline_seconds / elapsed_seconds
    complete=(completed_cases is None or completed_cases==candidate_cases) and validation_complete
    return dict(baseline_seconds=baseline_seconds, candidate_seconds=elapsed_seconds,
                baseline_cases=baseline_cases,candidate_cases=candidate_cases,
                completed_cases=candidate_cases if completed_cases is None else completed_cases,
                validation_complete=validation_complete,
                matched_speedup=measured if complete and baseline_cases==candidate_cases and not resumed else None,
                lower_bound_speedup=measured if complete and 0<baseline_cases<=candidate_cases and not resumed else None,
                tenfold_demonstrated=complete and not resumed and 0<baseline_cases<=candidate_cases and measured>=10,
                resumed=resumed,
                note='Incomplete or failed work cannot establish speedup.' if not complete else
                     'Resumed runs reuse work and do not establish fresh-run speedup.' if resumed else
                     'A partial baseline gives only a conservative time-ratio bound, not a matched full-corpus comparison.'
                     if baseline_cases!=candidate_cases else 'Matched case set; timings may still include differing shared load.')


def compare_results(baseline, candidate):
    before={r['id']:r for r in baseline}; after={r['id']:r for r in candidate}
    groups={}; regressions=[]
    for key in sorted(before.keys() & after.keys()):
        a,b=before[key],after[key]
        if a.get('status')!='completed' or b.get('status')!='completed':continue
        chars=bool(set(a['languages']) & CHARACTER_PRIMARY)
        numerator,denominator=('character_errors','reference_characters') if chars else ('word_errors','reference_words')
        av,bv=a['transcript'],b['transcript']
        if av[denominator]!=bv[denominator]:raise ValueError('Reference counts changed between runs')
        metric='cer' if chars else 'wer'
        for group in ['all',a['difficulty'],'language:'+'+'.join(sorted(a['languages']))]:
            rows=groups.setdefault((group,metric),[[],[]])
            rows[0].append(dict(id=key,numerator=av[numerator],denominator=av[denominator]))
            rows[1].append(dict(id=key,numerator=bv[numerator],denominator=bv[denominator]))
        if bv[numerator]>av[numerator]:
            regressions.append(dict(id=key,metric=metric,extra_errors=bv[numerator]-av[numerator],
                reference_units=av[denominator],before=av[numerator]/av[denominator],after=bv[numerator]/bv[denominator]))
    return dict(comparisons=[dict(group=group,metric=metric,**paired_difference(*values))
                            for (group,metric),values in sorted(groups.items())],
                regressions=regressions,missing_candidate=sorted(before.keys()-after.keys()),
                additional_candidate=sorted(after.keys()-before.keys()),acceptance_claim=False)


def load_results(path):
    return [json.loads(p.read_text()) for p in Path(path).glob('*/result.json')]


def markdown_report(statistics, timing, comparison, quality_reports=()):
    def percent(value):return 'Unscored' if value is None else f'{value*100:.2f}%'
    def safe(value):return str(value).replace('|','\\|').replace('\n',' ')
    speed=timing.get('full_evaluation_speed_comparison',{})
    ratio=speed.get('matched_speedup')
    lines=['# Meeting evaluation report','',
           f"Processed observations: {statistics['observed']} / {statistics['expected']}.",
           'Synthetic references and automatic notes judgments are not independent validation. No 99% acceptance claim.', '',
           '## Performance','',
           '| Stage | Seconds |','| --- | ---: |',
           f"| Diarization | {timing.get('diarization_seconds',0):.2f} |",
           f"| Recognition, alignment and transcript assembly | {timing.get('speech_seconds',0):.2f} |",
           f"| Both notes controls | {timing.get('summary_seconds',0):.2f} |",
           f"| Enhanced grading and statistics | {timing.get('grading_seconds',0):.2f} |",
           f"| Complete measured run | {timing.get('total_evaluation_seconds',0):.2f} |",'',
           f"Matched complete-run speedup: {f'{ratio:.2f}×' if ratio is not None else 'Not established'}.",
           'The original baseline did not include the added grading work; both inference-only and full timings remain in timing.json.', '',
           '## Error rates by difficulty','',
           '| Difficulty | Metric | Meetings scored | Error rate | Meeting bootstrap 95% interval |',
           '| --- | --- | ---: | ---: | --- |']
    for row in statistics['groups']:
        if row['dimension']!='difficulty':continue
        interval=row['bootstrap_95']
        ci='Insufficient clusters' if interval is None else f'{percent(interval[0])}–{percent(interval[1])}'
        lines.append(f"| {safe(row['value'])} | {safe(row['metric'])} | {row['cases']} / {row['expected_cases']} | {percent(row['rate'])} | {ci} |")
    lines+=['','Word, character, speaker and notes error rates have separate denominators; lower is better.', '',
            '## Worst language groups','',
            '| Metric | Language combination | Meetings | Error rate |','| --- | --- | ---: | ---: |']
    for metric,rows in statistics['worst_language_groups'].items():
        for row in rows[:5]:
            lines.append(f"| {safe(metric)} | {safe(row['value'])} | {row['cases']} | {percent(row['rate'])} |")
    lines+=['','Small groups are descriptive and cannot establish a universal accuracy claim.','',
            '## Notes: correctness and completeness','',
            '| Input | Supported statements | Fully covered reference facts | Reports graded |',
            '| --- | ---: | ---: | ---: |']
    critical={};guards={}
    for source in ['asr','reference']:
        rows=[r for r in quality_reports if r.get('source')==source]
        p_total=sum(r['precision']['total_claims'] for r in rows)
        r_total=sum(r['recall']['total_facts'] for r in rows)
        supported=sum(r['precision']['counts'].get('supported',0) for r in rows)
        covered=sum(r['recall']['counts'].get('covered',0) for r in rows)
        lines.append(f"| {source} | {supported} / {p_total} | {covered} / {r_total} | {len(rows)} |")
        for row in rows:
            for kind,count in row.get('critical_error_counts',{}).items():
                critical[kind]=critical.get(kind,0)+count
            for kind,count in row.get('evidence_guard_counts',{}).items():
                guards[kind]=guards.get(kind,0)+count
    lines+=['','These are same-model diagnostics. Partial, unsupported and ungraded items never count as complete successes.', '',
            'Critical error flags: '+(', '.join(f'{safe(k)}: {v}' for k,v in sorted(critical.items())) or 'None recorded.'), '',
            'Judge approvals rejected by deterministic evidence checks: '+(', '.join(f'{safe(k)}: {v}' for k,v in sorted(guards.items())) or 'None recorded.'), '',
            '## Paired transcription regressions','',
            '| Meeting | Metric | Additional errors | Before | After |','| --- | --- | ---: | ---: | ---: |']
    for row in sorted(comparison['regressions'],key=lambda r:r['extra_errors'],reverse=True)[:20]:
        lines.append(f"| {safe(row['id'])} | {row['metric']} | {row['extra_errors']} | {percent(row['before'])} | {percent(row['after'])} |")
    if not comparison['regressions']:lines.append('| None observed in comparable completed cases | — | — | — | — |')
    lines+=['','All regressions, missing cases, source judgments and paired intervals remain in the adjacent JSON files.']
    return '\n'.join(lines)+'\n'
