"""Staged, warm-model candidate evaluation; original baseline remains immutable."""
import argparse
import hashlib
import importlib.metadata
import json
import os
from pathlib import Path
import subprocess
import shutil
import sys
import time

if os.environ.get('OVERTALK_EVAL_TRACE_STALLS') == '1':
    import faulthandler
    faulthandler.dump_traceback_later(60, repeat=True)

from .gemini import atomic_json
from .metrics import transcript_score
from .run_evaluation import RUNTIME, ROOT, transcript_text


def digest(path):
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def model_artifact_hashes(settings):
    files=[]
    for key in ('asr_model_dir','aligner_model_dir'):
        directory=Path(settings[key])
        candidates=list(directory.glob('*.safetensors'))+list(directory.glob('*.json'))+list(directory.glob('*.txt'))
        if not candidates:raise ValueError('Missing local model artifacts: '+key)
        files.extend(candidates)
    files.append(Path(settings['diarization_model_path']))
    files.append(Path.home()/'.local/share/com.judeam.sonefold/models/summary/Qwen3.5-4B-Q4_K_M.gguf')
    return {str(path):digest(path) for path in files}


def execution_environment():
    packages={}
    for name in ['torch','transformers','qwen-asr','vllm']:
        try: packages[name]=importlib.metadata.version(name)
        except importlib.metadata.PackageNotFoundError: pass
    adapter = ROOT / 'speech-worker/sonefold_speech/vendor/qwen_asr'
    if adapter.is_dir():
        checksum = hashlib.sha256()
        for path in sorted(adapter.rglob('*.py')):
            checksum.update(path.relative_to(adapter).as_posix().encode())
            checksum.update(path.read_bytes())
        packages['overtalk-qwen-adapter-sha256'] = checksum.hexdigest()
    return dict(python=sys.version,executable=sys.executable,packages=packages,
        variables={key:os.environ.get(key) for key in ['CUDA_MPS_ACTIVE_THREAD_PERCENTAGE',
            'OMP_NUM_THREADS','MKL_NUM_THREADS','OPENBLAS_NUM_THREADS','LLAMA_PREFILL_CHUNK']},asr_cpu_thread_cap=4)


def summary_work_completed(reports, expected_reports):
    # An uncertain evidence verdict is a completed diagnostic, never an accuracy
    # pass. Missing or malformed grading work remains an incomplete evaluation.
    return len(reports)==expected_reports and all(
        not row['precision']['counts'].get('ungraded') and not row['recall']['counts'].get('ungraded')
        for row in reports)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--backend', choices=['transformers','vllm'], default='transformers')
    parser.add_argument('--batch-size', type=int, default=32)
    parser.add_argument('--gpu-fraction', type=float, default=.4)
    parser.add_argument('--initial-decode-tokens', type=int, default=256)
    parser.add_argument('--notes-workers', type=int, default=4)
    parser.add_argument('--cases', nargs='*')
    parser.add_argument('--native-runner', type=Path, required=True)
    parser.add_argument('--helper',type=Path,help='Candidate helper; defaults to the frozen installation')
    parser.add_argument('--resume', action='store_true')
    parser.add_argument('--quality', action=argparse.BooleanOptionalAction, default=True)
    parser.add_argument('--score', action=argparse.BooleanOptionalAction, default=True)
    args = parser.parse_args()
    if not 1<=args.notes_workers<=8:parser.error('Use one to eight native notes workers')
    started = time.perf_counter()
    if args.output.exists() and not args.resume:
        parser.error('Choose a new directory or use explicit --resume')
    if args.output.exists() and args.resume and not (args.output/'configuration.json').is_file():
        parser.error('Cannot resume artifacts without their original configuration')
    settings = json.loads((Path.home()/'.local/share/com.judeam.sonefold/speech-runtime.json').read_text())
    references = []
    for path in sorted((RUNTIME/'corpus').glob('*/reference.json')):
        reference = json.loads(path.read_text())
        if not args.cases or reference['id'] in args.cases:
            references.append(reference)
    if not references or args.cases and set(args.cases) != {r['id'] for r in references}:
        parser.error('One or more requested cases are missing')
    args.output.mkdir(parents=True, exist_ok=True)
    manifest = [dict(id=r['id'], audio_path=r['audio_path']) for r in references]
    installation=json.loads((RUNTIME/'evaluation-install.json').read_text())['path']
    helper_path=args.helper.resolve() if args.helper else Path(installation)/'bin/llama-helper'
    config = dict(backend=args.backend, batch_size=args.batch_size, gpu_fraction=args.gpu_fraction,
        environment=execution_environment(),
        quality=args.quality,score=args.score,
        initial_decode_tokens=args.initial_decode_tokens,
        notes_workers=args.notes_workers,
        model='qwen3-asr-1.7b', diarization='nemotron-3', summary='qwen3.5:4b',
        native_runner_sha256=digest(args.native_runner),
        native_helper_sha256=digest(helper_path),
        audio_sha256={r['id']:digest(r['audio_path']) for r in references},
        reference_sha256={r['id']:digest(RUNTIME/'corpus'/r['id']/'reference.json') for r in references},
        model_sha256=model_artifact_hashes(settings),
        reference_hashes={r['id']:r['script_sha256'] for r in references},
        code={name:digest(Path(__file__).with_name(name+'.py')) for name in ['fast_evaluation','throughput_asr','warm_diarization',
            'evaluation_insights','statistics_report','timed_speaker_metrics','summary_quality','judge_corpus',
            'claim_packets','batch_asr','metrics','batched_notes','notes_proxy','vllm_setup']},
        speech_code={name:digest(ROOT/'speech-worker'/'sonefold_speech'/name) for name in ['asr.py','__main__.py','diarization.py']})
    cfg = args.output/'configuration.json'
    if cfg.exists() and json.loads(cfg.read_text()) != config:
        raise ValueError('Candidate configuration changed; choose a new output directory')
    atomic_json(cfg, config)
    for group,prefix in [('code',Path(__file__).parent),('speech_code',ROOT/'speech-worker/sonefold_speech')]:
        destination=args.output/'source-snapshot'/group
        destination.mkdir(parents=True,exist_ok=True)
        for name in config[group]:
            source=prefix/(name+'.py' if group=='code' else name)
            shutil.copyfile(source,destination/source.name)
    atomic_json(args.output/'manifest.json', manifest)
    # When this parent runs under a GPU-sharing launcher, the isolated NeMo
    # interpreter inherits that launcher's environment rather than starting uncapped work.
    diar_log = (args.output/'diarization.log').open('a')
    try:
        subprocess.run([settings['diarization_python'], '-u', '-m', 'evaluations.meetingbench.warm_diarization',
            '--manifest', str(args.output/'manifest.json'), '--output', str(args.output/'diarization'),
            '--model', settings['diarization_model_path']], cwd=ROOT, stdout=diar_log, stderr=diar_log, check=True)
    finally:
        diar_log.close()
    diar_seconds = time.perf_counter()-started
    os.environ.update(SONEFOLD_QWEN3_ASR_1_7B_DIR=settings['asr_model_dir'],
                      SONEFOLD_QWEN3_ALIGNER_DIR=settings['aligner_model_dir'],
                      HF_HUB_OFFLINE='1', TRANSFORMERS_OFFLINE='1', HF_HUB_DISABLE_TELEMETRY='1')
    from .throughput_asr import ThroughputEngine
    from sonefold_speech.diarization import assign_speakers
    from sonefold_speech.__main__ import split_speaker_turns
    engine = ThroughputEngine(backend=args.backend,batch_size=args.batch_size,gpu_fraction=args.gpu_fraction,
        progress=lambda state:atomic_json(args.output/'progress.json',dict(state,case=active_case[0])),
        initial_decode_tokens=args.initial_decode_tokens)
    active_case=[None]
    speech_started = time.perf_counter()
    try:
        for ref in references:
            active_case[0]=ref['id']
            folder=args.output/'results'/ref['id'];folder.mkdir(parents=True,exist_ok=True)
            if (folder/'transcript.json').exists():
                continue
            diar = json.loads((args.output/'diarization'/(ref['id']+'.json')).read_text())
            if diar['status'] != 'completed':
                atomic_json(folder/'result.json',dict(id=ref['id'],status='failed',error=diar['error']))
                continue
            result, timing = engine.transcribe_batched(ref['audio_path'],speaker_turns=diar['turns'])
            rows = split_speaker_turns(assign_speakers(result['segments'],diar['turns']),result.get('language'))
            rows = [dict(row, speaker_id=row.get('speaker'), audio_start_time=row['start'],
                         audio_end_time=row['end']) for row in rows]
            atomic_json(folder/'transcript.json',dict(rows=rows,turns=diar['turns'],speakers=diar['speakers'],
                warnings=result['warnings'],timing=timing))
            from .review_preview import interval_diagnostics
            atomic_json(folder/'result.json',dict(id=ref['id'],status='speech_completed',difficulty=ref['difficulty'],
                languages=ref['languages'],duration_seconds=ref['duration_seconds'],
                transcript=transcript_score('\n'.join(t['text'] for t in ref['turns']),'\n'.join(r['text'] for r in rows),ref['languages']),
                speakers=dict(true_voices=len(ref['people']),detected_voices=len(diar['speakers']),
                    passages=len(rows),unassigned=sum(not r.get('speaker_id') for r in rows),
                    **interval_diagnostics([(t['speaker'],t['start_seconds'],t['end_seconds']) for t in ref['turns']],
                        [(t['speaker'],t['start'],t['end']) for t in diar['turns']])),
                timing=timing,diarization_seconds=diar['seconds']))
            print(json.dumps(dict(stage='speech',id=ref['id'],seconds=timing['total_seconds'])),flush=True)
    finally:
        engine.unload()
    speech_seconds = time.perf_counter()-speech_started
    # One native process generates both summary controls for every meeting,
    # preserving the current application pipeline and warm summary weights.
    inputs=args.output/'summary-inputs';inputs.mkdir(exist_ok=True)
    names=[]; all_names=[]
    note_provenance_path=args.output/'note-provenance.json'
    note_provenance=json.loads(note_provenance_path.read_text()) if note_provenance_path.exists() else {}
    for ref in references:
        folder=args.output/'results'/ref['id']
        if not (folder/'transcript.json').exists():continue
        rows=json.loads((folder/'transcript.json').read_text())['rows']
        for kind, text in [('asr',transcript_text(rows)),('reference','\n'.join(
                f"[{int(t['start_seconds'])//60:02}:{int(t['start_seconds'])%60:02}] [{t['speaker_name']}] {t['text']}" for t in ref['turns']))]:
            name=ref['id']+'-'+kind;all_names.append(name)
            atomic_json(inputs/f'meeting-{name}.json',dict(transcript=text,language='en'))
            prior=note_provenance.get(name,{})
            note=args.output/'notes'/f'{name}__qwen3.5-4b__app.md'
            metadata=note.with_suffix('.json')
            if not (note.exists() and metadata.exists()
                    and prior.get('input')==digest(inputs/f'meeting-{name}.json')
                    and prior.get('output')==digest(note) and prior.get('metadata')==digest(metadata)):
                names.append(name)
    env=dict(os.environ,OVERTALK_EVAL_APP_DIR=str(Path.home()/'.local/share/com.judeam.sonefold'),
        OVERTALK_EVAL_INPUT_DIR=str(inputs.resolve()),OVERTALK_EVAL_OUT=str((args.output/'notes').resolve()),
        OVERTALK_EVAL_MEETINGS=','.join(names),OVERTALK_EVAL_MODELS='qwen3.5:4b',OVERTALK_EVAL_MODES='app',
        SONEFOLD_LLAMA_HELPER=str(helper_path),
        LD_LIBRARY_PATH='/tmp/overtalk-ort-test-libs:'+installation+'/lib')
    summary_started=time.perf_counter()
    if names:
        from .batched_notes import run_native_notes
        run_native_notes(args.native_runner.resolve(),names,env,args.output,ROOT,workers=args.notes_workers)
    for name in all_names:
        note=args.output/'notes'/f'{name}__qwen3.5-4b__app.md'
        note_provenance[name]=dict(input=digest(inputs/f'meeting-{name}.json'),output=digest(note),metadata=digest(note.with_suffix('.json')))
    atomic_json(note_provenance_path,note_provenance)
    summary_seconds=time.perf_counter()-summary_started
    for ref in references:
        folder=args.output/'results'/ref['id'];result_path=folder/'result.json'
        if not result_path.exists():continue
        result=json.loads(result_path.read_text())
        if result['status']=='failed':continue
        statuses={}
        for kind in ['asr','reference']:
            note_path=args.output/'notes'/f"{ref['id']}-{kind}__qwen3.5-4b__app.md"
            markdown=note_path.read_text()
            statuses[kind]='failed' if markdown.startswith('ERROR:') else 'completed'
            note_timing=json.loads(note_path.with_suffix('.json').read_text())
            atomic_json(folder/f'{kind}-summary.json',dict(status=statuses[kind],data=dict(markdown=markdown),
                wall_seconds=note_timing['seconds']))
        result.update(status='completed' if all(v=='completed' for v in statuses.values()) else 'failed',summary_status=statuses)
        atomic_json(result_path,result)
    baseline=[]
    for reference in references:
        path=RUNTIME/'results'/reference['id']/'result.json'
        if path.exists():
            previous=json.loads(path.read_text())
            if previous.get('status')=='completed':baseline.append(previous)
    baseline_seconds=sum(r.get('timing',{}).get('total_seconds',0) for r in baseline)
    elapsed=time.perf_counter()-started
    from .evaluation_insights import compare_results,load_results,speed_comparison,markdown_report
    baseline_count=len(baseline)
    completed_count=sum(r.get('status')=='completed' for r in load_results(args.output/'results'))
    speed=speed_comparison(baseline_seconds,elapsed,baseline_cases=baseline_count,
                           candidate_cases=len(references),resumed=args.resume,completed_cases=completed_count)
    timing=dict(cases=len(references),seconds=elapsed,diarization_seconds=diar_seconds,
        speech_seconds=speech_seconds,summary_seconds=summary_seconds,matched_baseline_seconds=baseline_seconds,
        observed_speedup=speed['matched_speedup'],speed_comparison=speed,
        baseline_cases=baseline_count,
        caveats=['Shared-load timings; not an isolated hardware benchmark.',
                 'New speaker/summary/statistical grading is timed separately from the matched inference workload.'])
    atomic_json(args.output/'timing.json',timing)
    comparison=compare_results(load_results(RUNTIME/'results'),load_results(args.output/'results'))
    atomic_json(args.output/'comparison.json',comparison)
    grading_started=time.perf_counter()
    if args.score:
        metrics_python=RUNTIME/'metrics-venv/bin/python'
        if not metrics_python.exists():raise RuntimeError('Install requirements-metrics.txt in the separate metrics environment')
        with (args.output/'metrics.log').open('a') as log:
            subprocess.run([str(metrics_python),'-m','evaluations.meetingbench.timed_speaker_metrics',
                '--results',str(args.output/'results'),'--corpus',str(RUNTIME/'corpus'),
                '--output',str(args.output/'speaker-scores')],cwd=ROOT,stdout=log,stderr=log,check=True)
    if args.quality:
        with (args.output/'quality.log').open('a') as log:
            subprocess.run([sys.executable,'-u','-m','evaluations.meetingbench.summary_quality',
                '--results',str(args.output/'results'),'--corpus',str(RUNTIME/'corpus'),
                '--output',str(args.output/'summary-scores'),'--resume'],cwd=ROOT,
                env=dict(os.environ,OVERTALK_EVAL_JUDGE_HELPER=str(helper_path)),stdout=log,stderr=log,check=True)
    from .statistics_report import load_observations,build_report
    observations,expected=load_observations(args.output/'results',RUNTIME/'corpus',
        speaker_scores=args.output/'speaker-scores' if args.score else None,
        summary_scores=args.output/'summary-scores' if args.quality else None)
    expected=[row for row in expected if row['id'] in {r['id'] for r in references}]
    statistics=build_report(observations,expected)
    atomic_json(args.output/'statistics.json',statistics)
    quality_reports=[]
    for path in (args.output/'summary-scores').glob('*.json'):
        record=json.loads(path.read_text())
        if 'precision' in record:quality_reports.append(record)
    speech_scores=[json.loads(p.read_text()) for p in (args.output/'speaker-scores').glob('*.json')
                   if p.name!='configuration.json']
    validation_complete=(not args.score or (len(speech_scores)==len(references)
        and all(r.get('status')=='scored' for r in speech_scores))) and (not args.quality or (
        summary_work_completed(quality_reports,2*len(references))))
    total_elapsed=time.perf_counter()-started
    full_speed=speed_comparison(baseline_seconds,total_elapsed,baseline_cases=baseline_count,
                               candidate_cases=len(references),resumed=args.resume,completed_cases=completed_count,
                               validation_complete=validation_complete)
    timing.update(grading_seconds=time.perf_counter()-grading_started,total_evaluation_seconds=total_elapsed,
                  full_evaluation_speed_comparison=full_speed,
                  tenfold_demonstrated=full_speed['tenfold_demonstrated'])
    atomic_json(args.output/'timing.json',timing)
    (args.output/'report.md').write_text(markdown_report(statistics,timing,comparison,quality_reports))


if __name__ == '__main__':
    main()
