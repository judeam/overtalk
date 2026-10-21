"""CPU calibration of an independent English entailment classifier.

These authored minimal pairs test specific meeting-note mistakes. They are not
the meeting benchmark and cannot certify 99% factuality. In particular, a model
probability is not a calibrated correctness probability. No notes are modified.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import time

from .gemini import atomic_json
from .resources import peak_rss_kib


CALIBRATION_PAIRS = [
    ('owner', 'Alice will send the report. Bob will review it.',
     'Alice will send the report.', 'Bob will send the report.'),
    ('deadline', 'Send the revised plan by Friday. The event date is still undecided.',
     'The revised plan is due Friday.', 'The event will take place Friday.'),
    ('correction', 'The old cap was 3600 euros. We agreed to reduce the cap to 1200 euros.',
     'The agreed cap is 1200 euros.', 'The agreed cap is 3600 euros.'),
    ('negation', 'We will not launch tonight. We have not set another launch date.',
     'Tonight\'s launch is cancelled.', 'We will launch tonight.'),
    ('conditional', 'The release can proceed only if the security review passes. It has not passed yet.',
     'The release requires a successful security review.', 'The release is approved unconditionally.'),
    ('future_commitment', 'Daniel: I will prepare the revised plan by 27 June.',
     'Daniel committed to prepare the revised plan.', 'Daniel has prepared the revised plan.'),
    ('open_question', 'We do not have enough information to decide whether to hire extra support.',
     'Hiring extra support remains undecided.', 'We decided to hire extra support.'),
    ('assignee_correction', 'The plan belongs to Daniel, not Asha. Asha will help review it.',
     'Daniel owns the plan and Asha helps review it.', 'Asha owns the plan and Daniel helps review it.'),
    ('count', 'There are 46 orders under review. Only 12 are suspected duplicate charges.',
     'Twelve orders may have duplicate charges.', 'All 46 orders have duplicate charges.'),
    ('estimate', 'The fix should take two or three days, but no deadline has been agreed.',
     'The estimate is two or three days.', 'The fix has an agreed deadline in two days.'),
    ('subject', 'The search release is approved. The export release is deferred.',
     'Search is approved for release.', 'Export is approved for release.'),
    ('speaker_reference', 'Chair: Maya, can you write it? Maya: No. Leon: I will write it. Chair: Thanks, Leon.',
     'Leon agreed to write it.', 'Maya agreed to write it.'),
    ('not_guaranteed', 'The target is Monday, but it depends on the supplier; Monday is not a promise.',
     'Monday is a conditional target.', 'Monday is a guaranteed completion date.'),
    ('units', 'The transfer limit is 320 euros, not 230 euros.',
     'The transfer limit is 320 euros.', 'The transfer limit is 230 euros.'),
    ('scope', 'The first batch contains 42 cleared crates. Six other crates remain quarantined.',
     'Six crates remain quarantined.', 'All crates are cleared for shipment.'),
]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--model-dir', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--pairs', type=Path,
                        help='Optional source/claim pairs; omit supported for unlabelled diagnostics')
    args = parser.parse_args()
    if args.output.exists():
        parser.error('Use a new output directory')
    provenance = json.loads((args.model_dir/'provenance.json').read_text())
    for name, expected in provenance['sha256'].items():
        with (args.model_dir/name).open('rb') as stream:
            if hashlib.file_digest(stream, 'sha256').hexdigest() != expected:
                raise ValueError('Model artifact changed: '+name)
    os.environ.update(HF_HUB_OFFLINE='1', TRANSFORMERS_OFFLINE='1', HF_HUB_DISABLE_TELEMETRY='1')
    import torch
    from transformers import AutoModelForSequenceClassification, AutoTokenizer
    torch.set_num_threads(4)
    tokenizer = AutoTokenizer.from_pretrained(args.model_dir, local_files_only=True, trust_remote_code=False)
    model = AutoModelForSequenceClassification.from_pretrained(
        args.model_dir, local_files_only=True, trust_remote_code=False, use_safetensors=True, attn_implementation='eager').eval()
    label_ids = {name.lower(): int(index) for index, name in model.config.id2label.items()}
    entailment = label_ids.get('entailment')
    if entailment is None:
        raise ValueError('Checkpoint does not identify its entailment label')
    args.output.mkdir(parents=True)
    atomic_json(args.output/'configuration.json', dict(
        model=provenance, device='cpu', threads=4, batch_size=4,
        script_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
        limits=['Authored English minimal pairs, not real-meeting acceptance data.',
                'No universal 99% claim or calibrated confidence. Not applied to the app.']))
    examples = [dict(category=category, source=source, claim=claim, supported=supported)
                for category, source, correct, wrong in CALIBRATION_PAIRS
                for claim, supported in [(correct, True), (wrong, False)]]
    if args.pairs:
        examples = json.loads(args.pairs.read_text())
        if not isinstance(examples, list) or not 1 <= len(examples) <= 256:
            raise ValueError('Expected one to 256 source/claim pairs')
        for row in examples:
            if (not isinstance(row, dict) or not isinstance(row.get('source'), str)
                    or not isinstance(row.get('claim'), str)
                    or row.get('supported') is not None and not isinstance(row['supported'], bool)):
                raise ValueError('Invalid source/claim pair')
    began = time.perf_counter()
    with torch.inference_mode():
        for start in range(0, len(examples), 4):
            batch = examples[start:start+4]
            encoded = tokenizer([x['source'] for x in batch], [x['claim'] for x in batch],
                                return_tensors='pt', padding=True, truncation=False)
            if encoded['input_ids'].shape[1] > 1024:
                raise ValueError('Calibration source exceeds token budget')
            probabilities = model(**encoded).logits.softmax(-1).tolist()
            for row, probability in zip(batch, probabilities):
                row.update(entailment_probability=probability[entailment],
                           predicted_supported=probability[entailment] >= .5)
    elapsed = time.perf_counter()-began
    labelled = [row for row in examples if isinstance(row.get('supported'), bool)]
    result = dict(cases=len(examples), labelled=len(labelled),
                  correct=sum(x['predicted_supported'] == x['supported'] for x in labelled),
                  false_approvals=sum(x['predicted_supported'] and not x['supported'] for x in labelled),
                  false_rejections=sum(not x['predicted_supported'] and x['supported'] for x in labelled),
                  seconds=elapsed, peak_rss_kib=peak_rss_kib(),
                  rows=examples, acceptance_claim=False)
    atomic_json(args.output/'report.json', result)
    print(json.dumps({k:v for k,v in result.items() if k != 'rows'}), flush=True)


if __name__ == '__main__':
    main()
