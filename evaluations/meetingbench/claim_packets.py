"""Prepare every report statement for the experimental citation-support verifier.

This complements reference-fact coverage. Missing citations and oversized
passages remain explicitly ungraded in the denominator. Packets do not prove
truth or find omitted facts, and never modify a report or its source transcript.
"""
import argparse
import json
from pathlib import Path
import re

from .verify_notes import prompt_for


def report_statements(markdown):
    statements, paragraph = [], []
    header = None
    lines = markdown.splitlines()
    structural_headings = {
        'meeting notes', 'notes', 'summary', 'meeting summary', 'key decisions', 'decisions',
        'action items', 'actions', 'highlights', 'discussion highlights', 'additional highlights', 'open questions',
        'topics discussed', 'what each part covered', 'details mentioned', 'next steps',
        'attendees', 'date', 'blockers', 'yesterday', 'today', 'follow-up', 'follow up',
    }

    def flush():
        if paragraph:
            text = ' '.join(paragraph).strip()
            if text.lower() not in {'none noted in this section.', 'no additional highlights noted.'}:
                statements.append(text)
            paragraph.clear()

    for index, raw in enumerate(lines):
        line = raw.strip()
        if not line or line.startswith('#'):
            flush()
            header = None
            if line.startswith('#'):
                heading = re.sub(r'^#+\s*', '', line).strip().strip('*').strip()
                if heading and heading.casefold() not in structural_headings:
                    statements.append(heading)
            continue
        if line.startswith('|') and line.endswith('|'):
            flush()
            cells = [cell.strip() for cell in line[1:-1].split('|')]
            if all(re.fullmatch(r':?-+:?', cell.replace(' ', '')) for cell in cells):
                continue
            if index + 1 < len(lines) and re.match(r'^\s*\|\s*:?-+', lines[index + 1]):
                header = cells
                continue
            if header:
                if len(cells) != len(header):
                    raise ValueError('Malformed report table; do not silently omit its claims')
                text = '; '.join(f'{key}: {value}' for key, value in zip(header, cells)
                                 if key.casefold() not in {'source', 'time'})
                citations = ' '.join(re.findall(r'\[S\d+\]', line))
                statements.append(f'{text} {citations}'.strip())
            else:
                statements.append(line)
            continue
        header = None
        if re.match(r'^(?:[-*+] |\d+\. )', line):
            flush()
            paragraph.append(re.sub(r'^(?:[-*+] |\d+\. )', '', line))
        else:
            paragraph.append(line)
    flush()
    return [dict(id=f'C{i:04}', claim=text) for i, text in enumerate(statements, 1)]


def build_packets(markdown, numbered_transcript):
    source_by_id = {}
    for line in numbered_transcript.splitlines():
        match = re.match(r'^\[(S\d+)\]\s*(.*)$', line)
        if not match or match[1] in source_by_id:
            raise ValueError('Transcript requires unique source IDs on every line')
        source_by_id[match[1]] = match[2]
    claims = report_statements(markdown)
    jobs, ungraded = [], []
    for claim in claims:
        ids = list(dict.fromkeys(re.findall(r'\[(S\d+)\]', claim['claim'])))
        reason = None
        if not ids:
            reason = 'missing_citation'
        elif any(key not in source_by_id for key in ids):
            reason = 'unknown_citation'
        else:
            sources = [dict(id=key, text=source_by_id[key]) for key in ids]
            job = dict(id=claim['id'], claims=[claim], sources=sources)
            try:
                prompt_for(sources, [claim])
            except ValueError:
                reason = 'passage_exceeds_verifier_budget'
            else:
                jobs.append(job)
        if reason:
            ungraded.append(dict(**claim, reason=reason))
    return dict(claims=claims, jobs=jobs, ungraded=ungraded, total_statements=len(claims),
                independent=False, scope='citation_support_only',
                caveats=['One statement may contain several facts; support requires all of them.',
                         'This does not measure omitted reference facts or prove factual precision.',
                         'Ungraded statements stay in the denominator; do not count them as passing.'])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--summary', type=Path, required=True)
    parser.add_argument('--transcript', type=Path, required=True,
                        help='UTF-8 transcript with [S1], [S2], ... source IDs')
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    if args.output.exists():
        parser.error('Choose a new output directory')
    packet = build_packets(args.summary.read_text(), args.transcript.read_text())
    args.output.mkdir(parents=True)
    (args.output / 'manifest.json').write_text(json.dumps(packet, ensure_ascii=False, indent=2))
    (args.output / 'jobs.json').write_text(json.dumps(packet['jobs'], ensure_ascii=False, indent=2))
    print(json.dumps(dict(total=packet['total_statements'], prepared=len(packet['jobs']),
                          ungraded=len(packet['ungraded']))))


if __name__ == '__main__':
    main()
