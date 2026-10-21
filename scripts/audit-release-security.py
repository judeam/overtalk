#!/usr/bin/env python3
"""Audit CI and shell entry points locally with verified specialist tools.

--download fetches the pinned Linux x86-64 analyzers into a temporary directory.
Scanning is offline; no source, environment or credentials are uploaded.
"""
import argparse
import hashlib
import io
import json
from pathlib import Path
import platform
import shutil
import subprocess
import tarfile
import tempfile
import urllib.request

TOOLS = {
    'actionlint': ('https://github.com/rhysd/actionlint/releases/download/v1.7.12/actionlint_1.7.12_linux_amd64.tar.gz',
                   '8aca8db96f1b94770f1b0d72b6dddcb1ebb8123cb3712530b08cc387b349a3d8'),
    'zizmor': ('https://github.com/zizmorcore/zizmor/releases/download/v1.31.1/zizmor-x86_64-unknown-linux-gnu.tar.gz',
               'd185f726bdaf92df78273217dfdd24ebcd6b0ef48cf3ce776ff5762b6e138c99'),
}


def download(directory):
    if platform.system() != 'Linux' or platform.machine() not in ('x86_64', 'AMD64'):
        raise RuntimeError('Pinned downloads support Linux x86-64; use --tool-dir elsewhere')
    for name, (url, expected) in TOOLS.items():
        with urllib.request.urlopen(url, timeout=60) as response:
            data = response.read(20_000_001)
        if len(data) > 20_000_000 or hashlib.sha256(data).hexdigest() != expected:
            raise RuntimeError(f'{name} download failed checksum/size verification')
        with tarfile.open(fileobj=io.BytesIO(data), mode='r:gz') as archive:
            members = [m for m in archive if m.isfile() and Path(m.name).name == name]
            if len(members) != 1 or members[0].size > 40_000_000:
                raise RuntimeError(f'Invalid {name} archive')
            target = directory / name
            target.write_bytes(archive.extractfile(members[0]).read())
            target.chmod(0o700)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument('--tool-dir', type=Path)
    group.add_argument('--download', action='store_true')
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    args.output = args.output.resolve()
    args.output.parent.mkdir(parents=True, exist_ok=True)
    shellcheck = shutil.which('shellcheck')
    if not shellcheck:
        parser.error('Install ShellCheck before running this audit')
    with tempfile.TemporaryDirectory(prefix='overtalk-release-audit-') as temporary:
        temp = Path(temporary)
        tool_dir = args.tool_dir.resolve() if args.tool_dir else temp
        if args.download:
            download(tool_dir)
        actionlint, zizmor = (str(tool_dir / name) for name in TOOLS)

        def run(command):
            return subprocess.run(command, cwd=root, capture_output=True, text=True, timeout=90)

        # Prove the tools detect violations before accepting an empty report.
        canary = temp / 'canary.yml'
        canary.write_text('name: Canary\non: push\npermissions: read-all\njobs:\n  check:\n    runs-on: ubuntu-latest\n    steps:\n      - uses: actions/checkout@11d5960a326750d5838078e36cf38b85af677262\n')
        probe = run([zizmor, '--offline', '--no-config', '--format=json-v1', str(canary)])
        if not any(row['ident'] == 'artipacked' for row in json.loads(probe.stdout)):
            raise RuntimeError('zizmor did not detect the credential-persistence canary')
        canary.write_text('name: Canary\non: nonexistent-event\njobs: {}\n')
        if run([actionlint, str(canary)]).returncode == 0:
            raise RuntimeError('actionlint did not reject the invalid-workflow canary')
        shell = temp / 'canary.sh'
        shell.write_text('#!/bin/sh\necho $unquoted\n')
        probe = run([shellcheck, '--format=json', str(shell)])
        if not any(row['code'] == 2086 for row in json.loads(probe.stdout)):
            raise RuntimeError('ShellCheck did not detect the word-splitting canary')

        workflows = sorted(str(p.relative_to(root)) for p in (root / '.github/workflows').glob('*.y*ml'))
        if not workflows:
            raise RuntimeError('No workflows were discovered')
        shells = [str(p.relative_to(root)) for p in [root / 'scripts/run-installed-linux.sh', *sorted((root / 'frontend').glob('*.sh')), *sorted((root / 'frontend/scripts').glob('*.sh'))]]
        commands = {
            'actionlint': [actionlint, f'-shellcheck={shellcheck}', '-pyflakes=', '-format', '{{json .}}', *workflows],
            'shellcheck': [shellcheck, '--format=json', *shells],
            'zizmor': [zizmor, '--offline', '--strict-collection', '--no-config', '--persona=auditor',
                       '--min-severity=low', '--format=json-v1', *workflows, '.github/dependabot.yml'],
        }
        report = {'canaries': True, 'workflow_count': len(workflows), 'shell_count': len(shells), 'checks': {}}
        for name, command in commands.items():
            result = run(command)
            report['checks'][name] = {'exit_code': result.returncode, 'findings': json.loads(result.stdout)}
        report['passed'] = all(v['exit_code'] == 0 and not v['findings'] for v in report['checks'].values())
        args.output.write_text(json.dumps(report, indent=2) + '\n')
        print(f"Release audit: {len(workflows)} workflows, {len(shells)} shell scripts, canaries passed; clean={report['passed']}")
        raise SystemExit(0 if report['passed'] else 1)


if __name__ == '__main__':
    main()
