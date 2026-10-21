"""Frozen summary-only challenge; the assistant, not an LLM judge, grades reports.

Gold assertions never enter inference inputs. Existing runs are immutable. Cloud
requests use the same free-only, loopback, token-reserving gateway as comparison.
"""
import argparse
import hashlib
import json
import math
import os
import re
from pathlib import Path
import shutil
import subprocess
import time

from .privatemode_compare import Gateway, MODELS, ROOT, digest, read_key

CORPUS = Path(__file__).with_suffix('')


def wilson_lower(successes, total, z=1.96):
    if total <= 0:
        return None
    rate=successes/total
    return (rate+z*z/(2*total)-z*math.sqrt(rate*(1-rate)/total+z*z/(4*total*total)))/(1+z*z/total)


def cases(corpus=CORPUS):
    values = [json.loads(p.read_text()) for p in sorted(Path(corpus).glob('*.json'))]
    for case in values:
        assert case['split'] in ('development', 'validation', 'holdout')
        assert case['difficulty'] in ('normal', 'hard', 'very_hard')
        assert len({f['id'] for f in case['facts']}) == len(case['facts'])
        assert all(f['evidence'] and all(1 <= n <= len(case['transcript'].splitlines()) for n in f['evidence']) for f in case['facts'])
    return values


def prepare(output, corpus=CORPUS):
    output.mkdir(parents=True, exist_ok=False)
    (output/'inputs').mkdir()
    manifest = []
    for case in cases(corpus):
        path = output/'inputs'/f"meeting-{case['id']}.json"
        path.write_text(json.dumps({k:case[k] for k in ('transcript','language')}, ensure_ascii=False, indent=2))
        manifest.append({k:case[k] for k in ('id','split','difficulty','language','domain')} | {
            'input_sha256':digest(path), 'gold_sha256':hashlib.sha256(json.dumps(case, sort_keys=True, ensure_ascii=False).encode()).hexdigest()})
    (output/'manifest.json').write_text(json.dumps(manifest, indent=2))
    return manifest


def run(args):
    selected = [c for c in cases() if c['split'] in args.splits]
    lock_hash=None
    if 'holdout' in args.splits and args.prompt_policy=='grounded':
        if args.candidate_lock is None:
            raise ValueError('Grounded holdout runs require a passing pre-holdout candidate lock')
        lock=json.loads(args.candidate_lock.read_text())
        if (lock.get('native_sha256')!=digest(args.native_runner)
                or lock.get('corpus_sha256')!=digest(args.frozen/'manifest.json')
                or lock.get('all_preholdout_passed') is not True):
            raise ValueError('Candidate or corpus changed after locking')
        lock_hash=digest(args.candidate_lock)
    args.output.mkdir(parents=True, exist_ok=False)
    (args.output/'inputs').mkdir()
    (args.output/'notes').mkdir()
    frozen_manifest = json.loads((args.frozen/'manifest.json').read_text())
    pinned = {c['id']:c for c in frozen_manifest}
    for case in selected:
        path = args.frozen/'inputs'/f"meeting-{case['id']}.json"
        if digest(path) != pinned[case['id']]['input_sha256']:
            raise ValueError('Frozen input changed')
        if hashlib.sha256(json.dumps(case, sort_keys=True, ensure_ascii=False).encode()).hexdigest() != pinned[case['id']]['gold_sha256']:
            raise ValueError('Gold changed after freezing')
        shutil.copyfile(path, args.output/'inputs'/path.name)
    config = dict(models=args.models, cases=[c['id'] for c in selected], native_sha256=digest(args.native_runner),
        corpus_sha256=digest(args.frozen/'manifest.json'), free_plan_confirmed=args.confirmed_free_plan,
        prompt_policy=args.prompt_policy, max_completion_tokens=args.max_completion_tokens,
        quota_tokens=args.quota_tokens, reference_answers_supplied=False,candidate_lock_sha256=lock_hash)
    (args.output/'configuration.json').write_text(json.dumps(config, indent=2))
    key = read_key(args.key_file)
    started = time.perf_counter()
    with Gateway(args.endpoint, key, args.output/'usage.json', args.quota_tokens,
                 capture_dir=args.output/'synthetic-response-captures') as gateway:
        env = dict(os.environ, OVERTALK_EVAL_APP_DIR=str(args.output),
            OVERTALK_EVAL_INPUT_DIR=str(args.output/'inputs'), OVERTALK_EVAL_OUT=str(args.output/'notes'),
            OVERTALK_EVAL_PROVIDER='privatemode', OVERTALK_EVAL_FREE_PLAN_CONFIRMED='1',
            OVERTALK_EVAL_PRIVATEMODE_ENDPOINT=f'http://127.0.0.1:{gateway.server.server_port}/v1',
            OVERTALK_EVAL_MAX_COMPLETION_TOKENS=str(args.max_completion_tokens),
            OVERTALK_EVAL_MODES='app', OVERTALK_EVAL_TOPIC_PLAN='0', OVERTALK_EVAL_QUOTED_DECISIONS='0',
            OVERTALK_EVAL_BASELINE_PROMPT='1' if args.prompt_policy == 'baseline' else '0', TOKIO_WORKER_THREADS='2')
        for name in ('OVERTALK_EVAL_READING_WINDOW','OVERTALK_EVAL_DEBUG_DIR'):
            env.pop(name, None)
        for case in selected:
            for model in args.models:
                if gateway.budget.stopped:
                    break
                with (args.output/'native.log').open('a') as log:
                    subprocess.run([str(args.native_runner.resolve()), 'summary::eval_long_meetings::eval_long_meetings', '--exact', '--ignored', '--nocapture'],
                        env=dict(env, OVERTALK_EVAL_MEETINGS=case['id'], OVERTALK_EVAL_MODELS=model),
                        cwd=ROOT, stdout=log, stderr=log, check=True, timeout=900)
        used = gateway.budget.used
    notes = list((args.output/'notes').glob('*.md'))
    result = dict(completed=len(notes), expected=len(selected)*len(args.models),
        failed=[p.name for p in notes if p.read_text().startswith('ERROR:')],
        seconds=time.perf_counter()-started, quota_used_or_reserved=used, grading='pending direct assistant review')
    (args.output/'result.json').write_text(json.dumps(result, indent=2))
    print(json.dumps(result))


def validate_review(case, report, review):
    """Fail closed: each required fact and each emitted claim needs a judgment."""
    if case.get('id') is not None and review.get('case') != case['id']:
        raise ValueError('Review belongs to a different case')
    if review.get('report_sha256') != hashlib.sha256(report.encode()).hexdigest():
        raise ValueError('Review belongs to a different report')
    expected = {f['id'] for f in case['facts']}
    facts = review['facts']
    if len(facts) != len(expected) or {f['id'] for f in facts} != expected:
        raise ValueError('Missing or duplicate required-fact judgments')
    if any(not f.get('reason') for f in facts):
        raise ValueError('Required-fact judgments need reasons')
    if any(f['status'] not in ('covered','partial','missing','contradicted') for f in facts):
        raise ValueError('Invalid fact judgment')
    # Claim inventory is explicit prose, not keyword matching or model self-scoring.
    claims = review['claims']
    if not claims or any(not c['text'].strip() or c['status'] not in ('supported','unsupported','contradicted','uncertain')
                         or not c.get('reason') for c in claims):
        raise ValueError('Every emitted claim needs a direct evidence judgment')
    units = report_units(report)
    if len(claims) != len(units) or {c.get('id') for c in claims} != {u['id'] for u in units}:
        raise ValueError('Every report unit must be audited exactly once')
    if any(c['status'] == 'supported' and not c.get('evidence') for c in claims):
        raise ValueError('Supported claims require source evidence')
    source_count=len(case.get('transcript','').splitlines())
    if source_count and any(type(n) is not int or not 1 <= n <= source_count
                            for c in claims for n in c.get('evidence',[])):
        raise ValueError('Review cites a nonexistent source')
    if not review.get('complete_claim_inventory') or not isinstance(review.get('citation_errors'), list):
        raise ValueError('Full claim/citation audit required')
    generated_ok = bool(report.strip()) and not report.startswith('ERROR:')
    return dict(facts=len(facts), covered=sum(f['status']=='covered' for f in facts),
        claims=len(claims), supported=sum(c['status']=='supported' for c in claims),
        passed=generated_ok and all(f['status']=='covered' for f in facts)
            and all(c['status']=='supported' for c in claims) and not review['citation_errors'])


def report_units(report):
    """Inventory all substantive lines, including tables, dates and quotations.

    This is an audit aid, not a semantic grader. The reviewer must split/check
    every assertion within each unit; any wrong part fails that unit.
    """
    units = []
    for line in report.splitlines():
        text = line.strip()
        if not text or re.match(r'^#{1,6} ', text) or re.fullmatch(r'[| :\-]+', text):
            continue
        if text.startswith('|') and re.search(r'\b(Task|Action|Owner|Assignee)\b', text, re.I) and re.search(r'\b(Date|Due|Deadline)\b',text,re.I):
            continue
        units.append(dict(id=f'C{len(units)+1}', text=text))
    return units


def score(args):
    selected = {c['id']:c for c in cases(args.corpus)}
    config = json.loads((args.run/'configuration.json').read_text())
    models=config.get('models', ['qwen3.5-4b'] if 'model_sha256' in config else [])
    if not models:
        raise ValueError('No model identity in frozen configuration')
    results = []
    for case_id in config['cases']:
        for model in models:
            stem = f'{case_id}__{model}__app'
            report_path = args.run/'notes'/f'{stem}.md'
            if not report_path.is_file():
                results.append(dict(case=case_id, model=model, reviewed=False, generated=False, passed=False))
                continue
            report = report_path.read_text()
            review_path = args.reviews/f'{stem}.json'
            if not review_path.is_file():
                results.append(dict(case=case_id, model=model, reviewed=False, passed=False))
                continue
            review = json.loads(review_path.read_text())
            results.append(dict(case=case_id,model=model,reviewed=True,
                **validate_review(selected[case_id], report, review)))
    # Unreviewed/failed cases stay in the denominator. Never infer other grades.
    summary = {}
    for model in models:
        rows=[r for r in results if r['model']==model]
        summary[model]=dict(total=len(rows),reviewed=sum(r['reviewed'] for r in rows),
            passed=sum(r['passed'] for r in rows),
            all_case_pass_rate_lower_95=wilson_lower(sum(r['passed'] for r in rows),len(rows)),
            covered_facts_in_reviewed=sum(r.get('covered',0) for r in rows),
            required_facts_in_reviewed=sum(r.get('facts',0) for r in rows),
            supported_units_in_reviewed=sum(r.get('supported',0) for r in rows),
            output_units_in_reviewed=sum(r.get('claims',0) for r in rows),results=rows)
        summary[model]['by_difficulty']={difficulty:dict(total=sum(selected[r['case']]['difficulty']==difficulty for r in rows),
            passed=sum(r['passed'] and selected[r['case']]['difficulty']==difficulty for r in rows))
            for difficulty in ('normal','hard','very_hard')}
        summary[model]['by_language']={language:dict(total=sum(selected[r['case']]['language']==language for r in rows),
            passed=sum(r['passed'] and selected[r['case']]['language']==language for r in rows))
            for language in sorted({selected[r['case']]['language'] for r in rows})}
    print(json.dumps(summary,indent=2))


def lock_candidate(run, output):
    config=json.loads((run/'configuration.json').read_text())
    expected={c['id'] for c in cases() if c['split'] in ('development','validation')}
    if set(config['cases'])!=expected or config['models']!=['glm-5.3-flash'] or config['prompt_policy']!='grounded':
        raise ValueError('Lock requires exactly all development and validation cases for the grounded Flash candidate')
    by_id={c['id']:c for c in cases()}
    reviewed={}
    for name in sorted(expected):
        stem=f'{name}__glm-5.3-flash__app'
        report=run/'notes'/f'{stem}.md'; review=run/'direct-review'/f'{stem}.json'
        if not validate_review(by_id[name],report.read_text(),json.loads(review.read_text()))['passed']:
            raise ValueError(f'Pre-holdout case {name} failed')
        reviewed[name]=dict(report_sha256=digest(report),review_sha256=digest(review))
    value=dict(native_sha256=config['native_sha256'],corpus_sha256=config['corpus_sha256'],
        preholdout_configuration_sha256=digest(run/'configuration.json'),
        all_preholdout_passed=True,reviews=reviewed,
        note='Locks the candidate for this holdout run; prior exposure is documented separately. This lock alone establishes neither blindness nor population accuracy.')
    with output.open('x') as file:file.write(json.dumps(value,indent=2)+'\n')
    print(json.dumps(dict(locked=True,cases=len(reviewed),native_sha256=value['native_sha256'])))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest='command', required=True)
    p = sub.add_parser('prepare'); p.add_argument('--output',type=Path,required=True);p.add_argument('--corpus',type=Path,default=CORPUS)
    p = sub.add_parser('run')
    p.add_argument('--frozen',type=Path,required=True); p.add_argument('--output',type=Path,required=True)
    p.add_argument('--native-runner',type=Path,required=True); p.add_argument('--key-file',type=Path,required=True)
    p.add_argument('--endpoint',default='http://127.0.0.1:8788/v1')
    p.add_argument('--models',nargs='+',choices=MODELS,default=['glm-5.3-flash'])
    p.add_argument('--splits',nargs='+',choices=['development','validation','holdout'],required=True)
    p.add_argument('--prompt-policy',choices=['baseline','grounded'],required=True)
    p.add_argument('--quota-tokens',type=int,default=150_000)
    p.add_argument('--max-completion-tokens',type=int,default=4096)
    p.add_argument('--confirmed-free-plan',action='store_true')
    p.add_argument('--candidate-lock',type=Path)
    p = sub.add_parser('score'); p.add_argument('--run',type=Path,required=True); p.add_argument('--reviews',type=Path,required=True);p.add_argument('--corpus',type=Path,default=CORPUS)
    p = sub.add_parser('lock'); p.add_argument('--run',type=Path,required=True); p.add_argument('--output',type=Path,required=True)
    args = parser.parse_args()
    if args.command == 'prepare':
        print(json.dumps({'cases':len(prepare(args.output,args.corpus))}))
    elif args.command == 'score':
        score(args)
    elif args.command == 'lock':
        lock_candidate(args.run,args.output)
    else:
        if not args.confirmed_free_plan or not 1 <= args.quota_tokens <= 1_000_000 or not 1 <= args.max_completion_tokens <= 16384:
            parser.error('Confirmed Free plan and bounded quota/completions required')
        args.output = args.output.resolve()
        run(args)


if __name__ == '__main__':
    main()
