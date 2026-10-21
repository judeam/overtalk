"""Audit exact optional-ML pins without downloading multi-gigabyte GPU wheels.

This checks the declared direct pins. Installed/transitive runtime audits are a
separate release check; pinned source checkouts require source review.
"""
import argparse
import json
from pathlib import Path
import re
import urllib.request


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    pins = set()
    for name in ['requirements-qwen.txt', 'requirements-modern.txt', 'requirements-nemo.txt']:
        for line in (root / 'speech-worker' / name).read_text().splitlines():
            match = re.fullmatch(r'([\w-]+)==([\w.]+)', line.strip())
            if match:
                pins.add(match.groups())
    findings = []
    for name, version in sorted(pins):
        url = f'https://pypi.org/pypi/{name}/{version}/json'
        with urllib.request.urlopen(url, timeout=30) as response:
            metadata = json.load(response)
        vulnerabilities = [v for v in metadata.get('vulnerabilities', []) if not v.get('withdrawn')]
        findings.append(dict(name=name, version=version, vulnerabilities=vulnerabilities))
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(findings, indent=2)+'\n')
    failures = [item for item in findings if item['vulnerabilities']]
    if failures:
        raise SystemExit('Vulnerable model dependencies: '+', '.join(item['name'] for item in failures))
    print(f'Checked {len(findings)} exact ML dependency pins: no known advisories')


if __name__ == '__main__':
    main()
