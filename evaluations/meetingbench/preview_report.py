"""Reproducible listening-sample report; preserves original and corrected runs."""
import html,json
from pathlib import Path
from .gemini import atomic_json
from .metrics import transcript_score


def main():
    root=Path(__file__).resolve().parents[2];runtime=Path(__file__).parent/'.runtime'
    output=root/'reports/meetingbench';output.mkdir(parents=True,exist_ok=True)
    descriptions={
        'normal-preview':('Normal: library workshop','Room, time and refreshment budget were retained. Names in the app were supplied from the known synthetic cast after inference.'),
        'hard-preview':('Hard: original overlap','Notes incorrectly suggest transporting unapproved crates and confuse task ownership.'),
        'hard-natural-preview':('Hard: natural interruptions','Voice assignment improves substantially. Notes still misstate whether the fuel surcharge is inside the ceiling, and treat completed quarantine as open.'),
        'very-hard-preview':('Very hard: original overlap','A phrase was hallucinated 111 times. Most transcript time was unassigned.'),
        'very-hard-natural-preview':('Very hard: natural interruptions','No repeated-phrase loop. Notes still confuse finance approval with the internal cost cap and list completed token rotation as open.'),
    }
    rows=[];cards=[]
    for case,(label,note) in descriptions.items():
        folder=runtime/case
        if not (folder/'installed.json').exists():continue
        script=json.loads((folder/'reference.json').read_text());transcript=json.loads((folder/'actual-transcript.json').read_text());installed=json.loads((folder/'installed.json').read_text())
        metrics=transcript_score(' '.join(t['text'] for t in script['turns']),' '.join(t['text'] for t in transcript),script['languages'])
        unknown=sum(not t.get('speaker_id') for t in transcript);c=script['construction']
        record=dict(case=case,label=label,meeting_id=installed['meeting_id'],duration_seconds=script['duration_seconds'],metrics=metrics,
                    unknown_passages=unknown,passages=len(transcript),detected_voices=len(installed['speaker_result']),actual_voices=len(script['people']),
                    overlap_fraction=c['overlap_fraction_of_speech_time'],longest_overlap_seconds=c.get('longest_overlap_seconds'),notes_review=note)
        rows.append(record)
        rate=100*metrics['strict_wer'];bar=min(100,rate)
        audio=f'../../evaluations/meetingbench/.runtime/{case}/meeting.wav'
        cards.append(f'''<article><h2>{html.escape(label)}</h2><audio controls preload="none" src="{audio}"></audio>
<div class="metrics"><div><strong>{rate:.1f}%</strong> raw word error<div class="bar"><span style="width:{bar}%"></span></div></div><div><strong>{unknown}/{len(transcript)}</strong> passages unassigned</div><div><strong>{100*c['overlap_fraction_of_speech_time']:.1f}%</strong> overlapping speech</div></div><p>{html.escape(note)}</p><small>App meeting: {html.escape(installed['meeting_id'])}</small></article>''')
    atomic_json(output/'listening-samples.json',dict(samples=rows,caveat='Script-relative errors; overlap ordering, TTS deviations and multilingual normalization affect scores. Names annotated from reference are not automatic name recognition.'))
    (output/'listening-samples.html').write_text('''<!doctype html><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Overtalk listening tests</title>
<style>body{background:#10161d;color:#e8eef4;font:16px/1.6 system-ui;margin:40px auto;padding:0 24px;max-width:1100px}h1{font-size:32px;margin-bottom:8px}h2{font-size:21px;margin-top:0}p,small{color:#a5b6c5}article{background:#19232e;border:1px solid #2d3c4b;border-radius:14px;padding:24px;margin:22px 0}audio{width:100%;height:40px}.metrics{display:flex;gap:32px;flex-wrap:wrap;margin:18px 0}.metrics>div{min-width:170px}strong{font-size:24px;display:block;color:#7ed8ce}.bar{height:5px;background:#2c3845;margin-top:8px}.bar span{display:block;background:#e985a2;height:5px}small{overflow-wrap:anywhere}</style>
<h1>Overtalk listening tests</h1><p>Original mixes and corrected natural interruptions. Press play to compare. Ordinary conversation no longer overlaps; only brief interruptions in marked angry arguments do.</p><p>Word error compares recognized text with the intended synthetic script. Overlap order, multilingual spelling and TTS deviations affect it. Speaker names displayed in the app are supplied from the known cast after inference.</p>'''+''.join(cards))
    print(output/'listening-samples.html')

if __name__=='__main__':main()
