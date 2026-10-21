"""Whole-meeting SegLST scoring: cpWER, tcpWER, DER and JER with declared policies.

Input JSON arrays: {session_id,speaker,start_time,end_time,words}. Use continuous
held-out meetings, never cropped AMI utterances. Speaker IDs may be anonymous.
"""
import argparse,dataclasses,json,math
from pathlib import Path

def validated(rows):
    from benchmark_asr import normalized
    output=[]
    for row in rows:
        start,end=row['start_time'],row['end_time']
        if isinstance(start,bool) or isinstance(end,bool) or not all(isinstance(t,(float,int)) and math.isfinite(t) for t in (start,end)) or not 0<=start<end:raise ValueError('Invalid segment timestamps')
        if not all(isinstance(row[k],str) and row[k] for k in ['session_id','speaker']):raise ValueError('Missing session or speaker')
        output.append({**row,'words':normalized(row.get('words',''))})
    return output

def score(reference,hypothesis,der_collar=0.25,tcp_collar=5.0,skip_overlap=False):
    from meeteval.io import SegLST
    from meeteval.wer.api import cpwer,tcpwer
    from pyannote.core import Annotation,Segment
    from pyannote.metrics.diarization import DiarizationErrorRate,JaccardErrorRate
    reference,hypothesis=validated(reference),validated(hypothesis)
    sessions={r['session_id'] for r in reference}
    if {r['session_id'] for r in hypothesis}!=sessions:raise ValueError('Whole-meeting scoring requires a prediction for every reference session')
    cp=cpwer(SegLST(reference),SegLST(hypothesis));tcp=tcpwer(SegLST(reference),SegLST(hypothesis),collar=tcp_collar)
    der=DiarizationErrorRate(collar=der_collar,skip_overlap=skip_overlap);jer=JaccardErrorRate(collar=der_collar,skip_overlap=skip_overlap)
    results={}
    def annotation(rows,session):
        a=Annotation(uri=session)
        for index,row in enumerate(rows):
            if row['session_id']==session:a[Segment(row['start_time'],row['end_time']),index]=row['speaker']
        return a
    for session in sorted(sessions):
        ref,hyp=annotation(reference,session),annotation(hypothesis,session)
        results[session]={'cpwer':dataclasses.asdict(cp[session]),'tcpwer':dataclasses.asdict(tcp[session]),'der':der(ref,hyp),'jer':jer(ref,hyp)}
    return {'sessions':results,'aggregate_der':abs(der),'aggregate_jer':abs(jer),'policy':{'der_collar_seconds':der_collar,'skip_overlap':skip_overlap,'tcpwer_collar_seconds':tcp_collar,'pseudo_word_times':'meeteval character-based default; real word alignment preferred','normalization':'nfkc-casefold-punctuation-space-v1'}}

def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--reference',type=Path,required=True);p.add_argument('--hypothesis',type=Path,required=True);p.add_argument('--output',type=Path,required=True);p.add_argument('--der-collar',type=float,default=.25);p.add_argument('--tcp-collar',type=float,default=5);p.add_argument('--skip-overlap',action='store_true');a=p.parse_args()
    if min(a.der_collar,a.tcp_collar)<0:raise ValueError('Collars must be nonnegative')
    result=score(json.loads(a.reference.read_text()),json.loads(a.hypothesis.read_text()),a.der_collar,a.tcp_collar,a.skip_overlap);a.output.write_text(json.dumps(result,indent=2,default=str)+'\n')
if __name__=='__main__':main()
