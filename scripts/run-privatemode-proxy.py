#!/usr/bin/env python3
"""Run the pinned official attesting proxy, without keys in argv or history.

Docker is required (including its Linux VM on macOS/Windows). This does not
enable billing, install a system service, or change Overtalk's active provider.
"""
import argparse
import os
from pathlib import Path
import stat
import subprocess

IMAGE = 'ghcr.io/edgelesssys/privatemode/privatemode-proxy@sha256:3cd4da0ee6ceee91ca84dde4e3e3f0e94e8b354d7c1d46987e4331a5c2f8bcff'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--key-file', type=Path, required=True)
    parser.add_argument('--port', type=int, default=8788)
    parser.add_argument('--detach', action='store_true')
    args = parser.parse_args()
    info = args.key_file.lstat()
    if not stat.S_ISREG(info.st_mode) or args.key_file.is_symlink():
        parser.error('Use a regular key file, not a symlink')
    if os.name == 'posix' and (info.st_uid != os.getuid() or stat.S_IMODE(info.st_mode) & 0o077):
        parser.error('Key file must belong to you and have mode 600 or stricter')
    if not 1024 <= args.port <= 65535:
        parser.error('Choose a port between 1024 and 65535')
    command = ['docker', 'run', '--rm', '--name', f'overtalk-privatemode-{args.port}',
        '--read-only', '--cap-drop=ALL', '--security-opt=no-new-privileges',
        '--publish', f'127.0.0.1:{args.port}:8080',
        '--mount', f'type=bind,src={args.key_file.absolute()},dst=/run/secrets/api-key,readonly',
        '--tmpfs', '/tmp:rw,noexec,nosuid,size=16m',
        '--tmpfs', '/workspace:rw,noexec,nosuid,size=32m,mode=700' +
            (f',uid={os.getuid()},gid={os.getgid()}' if os.name == 'posix' else '')]
    if os.name == 'posix':
        command += ['--user', f'{os.getuid()}:{os.getgid()}']
    if args.detach:
        command += ['--detach']
    command += [IMAGE, '--workspace=/workspace', '--api-key=@/run/secrets/api-key',
        '--api-key-fallback=if-placeholder', '--listen-address=0.0.0.0', '--port=8080',
        '--nvidia-ocsp-allow-unknown=false', '--nvidia-ocsp-revoked-grace-period=0']
    subprocess.run(command, check=True)
    print(f'Proxy endpoint: http://127.0.0.1:{args.port}/v1')
    print(f'Stop it with: docker stop overtalk-privatemode-{args.port}')


if __name__ == '__main__':
    main()
