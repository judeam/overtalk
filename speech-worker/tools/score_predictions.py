"""Score private predictions; refuse to imply diarization quality from utterance crops."""
from __future__ import annotations
import argparse,json,sys,statistics
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parent))
from benchmark_asr import normalized

def score(manifest,predictions):
    import jiwer
    expected={r['audio_path']:r for r in manifest};seen=set();refs=[];hyps=[];allrefs=[];allhyps=[];failed=[];timings=[];silence=[]
    for row in predictions:
        key=row['audio_path']
        if key not in expected or key in seen:raise ValueError('Predictions contain an unknown or duplicate recording')
        seen.add(key);reference=normalized(expected[key]['text']);allrefs.append(reference)
        if row.get('prediction') is None:
            failed.append({'audio_path':key,'error':row.get('error','No prediction')});allhyps.append('');continue
        hypothesis=normalized(row['prediction']);allhyps.append(hypothesis);refs.append(reference);hyps.append(hypothesis)
        if not reference:silence.append(len(hypothesis.split()))
        if row.get('elapsed_seconds') is not None:timings.append(row['elapsed_seconds'])
    if seen!=set(expected):raise ValueError('Predictions are incomplete; finish evaluation before scoring')
    def metrics(reference,hypothesis):
        words=jiwer.process_words(reference,hypothesis);chars=jiwer.process_characters([r.replace(' ','') for r in reference],[h.replace(' ','') for h in hypothesis])
        return {'wer':words.wer,'cer':chars.cer,'substitutions':words.substitutions,'deletions':words.deletions,'insertions':words.insertions,'reference_words':words.hits+words.substitutions+words.deletions}
    return {'recordings':len(expected),'meetings':len({r.get('session_id') for r in manifest}),'completed':len(refs),'failed':failed,'accepted_predictions':metrics(refs,hyps) if refs else None,'all_recordings_failed_as_empty_predictions':metrics(allrefs,allhyps),'elapsed_p50_seconds':statistics.median(timings) if timings else None,'silence_hallucination_words':sum(silence),'normalization':'nfkc-casefold-punctuation-space-v1','limitations':'A small utterance pilot. No continuous DER/JER, speaker attribution, bootstrap significance, or universal superiority claim.'}

def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--manifest',type=Path,required=True);p.add_argument('--predictions',type=Path,required=True);p.add_argument('--output',type=Path,required=True);a=p.parse_args()
    load=lambda path:[json.loads(line) for line in path.read_text().splitlines() if line.strip()]
    result=score(load(a.manifest),load(a.predictions));a.output.write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2))
if __name__=='__main__':main()
