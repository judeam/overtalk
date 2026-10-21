"""Assemble controlled scripts from assistant-authored multilingual dialogue.

These are parameterized synthetic scenarios, not 200 independent human-authored
real meetings. Provenance and repetition diagnostics travel with every script.
"""
from collections import Counter
import hashlib
import json
from pathlib import Path
import random
import re

from .gemini import atomic_json
from .language_packs import PACKS
from .plan import build_plan, SCENARIOS
from .topics import TOPICS

PHASES = 'opening status question evidence proposal challenge alternative old_cost correct_cost shortcut reject commit owner_question owner_correct condition unknown clarify repeat_date example constraint check_understanding ack close'.split()
ESSENTIAL = {'opening','correct_cost','reject','commit','owner_correct','condition','unknown','close'}
PRIORITY = 'status evidence old_cost repeat_date proposal challenge alternative owner_question clarify question constraint check_understanding ack example shortcut'.split()
VOICES = ['Leda','Puck','Kore','Charon','Aoede','Fenrir','Zephyr','Orus']
SERIES = ['Orion','Cedar','Maple','Kestrel','Harbor','Willow','Atlas','Juniper','Birch','Beacon','Linden','Pine','Delta','Coral','Sparrow','Rowan','Heron','Aspen','Finch','Stone','Cobalt','Amber','Sierra','Olive']


def estimated_seconds(text, language):
    if language in {'zh','yue','ja','ko','th'}:
        rate={'zh':5.5,'yue':5.3,'ja':5,'ko':5.5,'th':10}[language]
        return len(re.sub(r'\s','',text))/rate
    return len(text.split())/2.25


def date_text(day, language):
    month=PACKS[language]['month']
    return f'{month}{day}日' if language in {'zh','yue','ja'} else f'{day} {month}'


def build_script(spec):
    rng=random.Random(spec['seed']); primary=spec['languages'][0]; names=PACKS[primary]['names']
    count=spec['participants']; index=SCENARIOS.index(spec['scenario'])
    people=[dict(id=f'P{i+1}',name=names[i],voice='custom-existing' if i==0 and primary in {'en','hi'} else VOICES[i],
                 style=f"Adult {'woman' if i%2==0 else 'man'} speaking naturally in {', '.join(spec['languages'])}. Distinct {['warm medium','bright medium-high','clear high','resonant low','soft medium','dry low','light high','textured medium'][i]} register. Conversational workplace speech; never narrate instructions.") for i in range(count)]
    target=spec['target_spoken_seconds']; turns=[];facts=[]; agendas=[]; elapsed=0.; number=0
    # Estimate one full agenda, then spread the authored dialogue over enough
    # distinct agenda items, each with independent owners/quantities/deadlines.
    example={key:'Atlas' for key in ['item','owner','other']}
    example.update(date=date_text(24,primary),old_date=date_text(18,primary),amount='480',old_amount='1,800',units='12')
    full=sum(estimated_seconds(PACKS[primary][phase].format(**example),primary) for phase in PHASES)
    agenda_count=max(1,round(target/max(full,1)))
    hot_count = 0 if spec["difficulty"] == "normal" else 1 if spec["duration_seconds"] < 1200 else 2 if spec["difficulty"] == "hard" else 3
    hot_agendas = {round(i * (agenda_count - 1) / max(1, hot_count - 1)) for i in range(min(hot_count, agenda_count))}
    for agenda in range(agenda_count):
        owner=(agenda+1)%count; other=(owner+1)%count
        number+=1; code=f'{SERIES[(spec["seed"]+agenda)%len(SERIES)]} {number}'
        units=rng.randint(3,88);amount=rng.choice([320,450,680,850,960,1200,2400,4800])+agenda*10
        day=rng.randint(17,28); values={}
        for language in spec['languages']:
            values[language]=dict(item=f'{TOPICS[language][index]} {code}',owner=names[owner],other=names[other],
                units=str(units),amount=f'{amount} EUR',old_amount=f'{amount*3} EUR',
                date=date_text(day,language),old_date=date_text(day-4,language))
        selected=set(ESSENTIAL)
        if spec['difficulty']!='normal':
            selected.update(['question','evidence','proposal','challenge','alternative'])
        remaining_target=(target-elapsed)/(agenda_count-agenda)
        def size(phases):
            return sum(estimated_seconds(PACKS[primary][p].format(**values[primary]),primary) for p in phases)
        for phase in PRIORITY:
            if size(selected)<remaining_target:
                selected.add(phase)
        phases=[p for p in PHASES if p in selected]
        roles={'opening':0,'status':owner,'question':2%count,'evidence':owner,'proposal':3%count,
               'challenge':4%count,'alternative':0,'old_cost':5%count,'correct_cost':2%count,'shortcut':3%count,
               'reject':3%count,'commit':owner,'owner_question':4%count,'owner_correct':4%count,'condition':5%count,
               'unknown':6%count,'clarify':7%count,'repeat_date':owner,'example':owner,'constraint':other,
               'check_understanding':3%count,'ack':3%count,'close':7%count}
        sources={}; group_phases=[]
        if agenda in hot_agendas:
            options=[p for p in ['proposal','challenge','alternative'] if p in phases]
            group_phases=options[:min(len(options),spec['max_concurrent_speakers'])]
        for position,phase in enumerate(phases):
            who=roles[phase]
            if phase in group_phases:
                who=group_phases.index(phase)%count
            language=spec['languages'][(position+agenda)%len(spec['languages'])]
            # A language switch retains the same speaker/voice identity.
            text=PACKS[language][phase].format(**values[language]); tid=f'T{len(turns)+1:04d}'
            row=dict(id=tid,speaker=people[who]['id'],text=text,language=language,agenda=number,authored_phase=phase,
                     emotion='angry' if phase in group_phases else {'question':'curious','evidence':'concerned','correct_cost':'firm','unknown':'uncertain','ack':'relieved','close':'warm'}.get(phase,'calm'))
            if phase in group_phases: row['group']=f'agenda-{number}-debate'
            turns.append(row);sources[phase]=tid;elapsed+=estimated_seconds(text,language)
        def fact(suffix,kind,statement,source_phases,**extra):
            facts.append(dict(id=f'A{number}-{suffix}',kind=kind,statement=statement,
                sources=[sources[p] for p in source_phases if p in sources],agenda=number,**extra))
        item=values[primary]['item']
        fact('D1','decision',f'{item}: reduced scope, proceeding only after review.', ['condition'])
        fact('D2','decision',f'{item}: spending ceiling {amount} EUR, not {amount*3} EUR.', ['correct_cost'],quantity=amount,rejected_quantity=amount*3)
        fact('R1','rejected',f'{item}: skipping review is rejected.', ['reject'])
        fact('A1','action',f'{names[owner]} prepares the revised plan for {item} by {day} June.', ['commit','owner_correct'],owner=people[owner]['id'],due=f'2026-06-{day:02d}')
        fact('O1','open_issue',f'{item}: whether extra assistance is needed remains unresolved.', ['unknown'])
        agendas.append(dict(id=number,topic=item,estimated_spoken_seconds=round(sum(estimated_seconds(t['text'],t['language']) for t in turns if t['agenda']==number),1)))
    canonical=json.dumps(turns,ensure_ascii=False,sort_keys=True).encode()
    remote=[] if spec['difficulty']=='normal' else [people[-1]['id']]
    if spec['difficulty']=='really_hard' and count>2:remote.append(people[-2]['id'])
    return dict(**spec,title=f'[Synthetic {spec["difficulty"]}] {spec["scenario"]} — {spec["id"]}',
        count_toward_200=True,reference_status='Assistant-authored parameterized dialogue; no human/native-speaker or audible TTS validation yet',
        provenance='Composed from authored multilingual phases with unique agenda items, assignments, costs and dates. Templates are shared across cases; do not treat cases as fully independent natural meetings.',
        people=people,turns=turns,facts=facts,agendas=agendas,estimated_spoken_seconds=round(elapsed,1),script_sha256=hashlib.sha256(canonical).hexdigest(),
        audio_profile=dict(interruption_intensity=spec['interruption_intensity'],snr_db=spec['target_snr_db'],packet_loss=spec['packet_loss_fraction'],remote_speakers=remote,echo_speakers=[people[-1]['id']] if spec['difficulty']=='really_hard' else []))


def validate_script(script):
    ids={t['id'] for t in script['turns']};people={p['id'] for p in script['people']}
    assert len(ids)==len(script['turns'])
    assert {t['speaker'] for t in script['turns']}==people
    assert all(t['language'] in script['languages'] for t in script['turns'])
    assert all(f['sources'] and set(f['sources'])<=ids for f in script['facts'])
    assert all(not re.search(r'\{[a-z_]+\}',t['text']) for t in script['turns'])
    for group in {t.get('group') for t in script['turns']} - {None}:
        rows=[t for t in script['turns'] if t.get('group')==group]
        assert len({t['speaker'] for t in rows})==len(rows)
    assert script['estimated_spoken_seconds'] > 0


def main():
    root=Path(__file__).parent/'.runtime';plan=build_plan();scripts=[]
    for spec in plan['meetings']:
        script=build_script(spec);validate_script(script)
        path=root/'scripts'/(spec['id']+'.json');atomic_json(path,script);scripts.append(script)
    result=dict(scripts=len(scripts),languages=len({l for s in scripts for l in s['languages']}),
                difficulty=dict(Counter(s['difficulty'] for s in scripts)),
                estimated_spoken_hours=sum(s['estimated_spoken_seconds'] for s in scripts)/3600,
                planned_spoken_hours=plan['estimated_cost']['planned_spoken_hours'],
                turns=sum(len(s['turns']) for s in scripts),facts=sum(len(s['facts']) for s in scripts),
                status='draft corpus: structural checks passed; acoustic and native-language validation pending',
                provenance='Parameterized assistant-authored controlled scenarios; templates shared across cases')
    atomic_json(root/'script-inventory.json',result);print(json.dumps(result,indent=2))

if __name__=='__main__':main()
