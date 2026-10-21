"""Check the local precision judge on authored minimal pairs before using it.

Calibration results are diagnostics, not independent meeting accuracy. Expected
labels stay outside model prompts; all failures and raw answers are retained.
"""
import argparse
import hashlib
import json
from pathlib import Path
import time

from .batch_asr import bounded_inference
from .gemini import atomic_json
from .judge_corpus import Judge
from .probe_factuality import CALIBRATION_PAIRS
from .summary_quality import pack_jobs, parse_precision, precision_prompt


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output',type=Path,required=True)
    args=parser.parse_args()
    if args.output.exists():parser.error('Choose a new output directory')
    jobs=[]
    for category,source,correct,wrong in CALIBRATION_PAIRS:
        for claim,expected in [(correct,True),(wrong,False)]:
            facts=[dict(id='F1',statement=source)]
            claims=[dict(id='C1',claim=claim)]
            jobs.append(dict(id=f'{category}-{expected}',facts=facts,claims=claims,expected=expected,
                             prompt=precision_prompt(facts,claims)))
    args.output.mkdir(parents=True)
    judge=Judge(context_size=131072)
    results=[];began=time.perf_counter()
    try:
        for group in pack_jobs(jobs,context_size=131072):
            outputs=bounded_inference(judge.predict,[j['prompt'] for j in group],len(group))
            for job,(raw,error) in zip(group,outputs):
                try:
                    if error:raise ValueError(error)
                    rows=parse_precision(raw,job['facts'],job['claims'])
                    predicted=rows[0]['verdict']=='supported'
                except (ValueError,TypeError,KeyError,AttributeError) as exc:
                    rows=[];predicted=None;error=str(exc)
                record=dict(id=job['id'],expected=job['expected'],predicted=predicted,
                            items=rows,error=error,raw=raw)
                results.append(record)
                atomic_json(args.output/(job['id']+'.json'),record)
        with Path(judge.model).open('rb') as source:model_hash=hashlib.file_digest(source,'sha256').hexdigest()
        report=dict(cases=len(jobs),correct=sum(r['predicted']==r['expected'] for r in results),
                    false_approvals=sum(r['predicted'] is True and not r['expected'] for r in results),
                    false_rejections=sum(r['predicted'] is False and r['expected'] for r in results),
                    ungraded=sum(r['predicted'] is None for r in results),seconds=time.perf_counter()-began,
                    model_sha256=model_hash,context_size=131072,independent=False,acceptance_claim=False)
        atomic_json(args.output/'report.json',report)
        print(json.dumps(report))
    finally:judge.close()


if __name__=='__main__':main()
