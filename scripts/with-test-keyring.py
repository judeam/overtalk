"""Run a Linux CI command with a private, unlocked Secret Service keyring.

Run it inside `dbus-run-session`: the daemon serves that session bus only and
keeps its keyring in a temporary folder, so a desktop's own keyring is untouched.
"""
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time


def owned(name):
    reply = subprocess.run(
        ['dbus-send', '--session', '--print-reply', '--dest=org.freedesktop.DBus', '/org/freedesktop/DBus',
         'org.freedesktop.DBus.NameHasOwner', f'string:{name}'],
        capture_output=True, text=True)
    return reply.returncode == 0 and 'boolean true' in reply.stdout


def main():
    if len(sys.argv) < 2:
        raise SystemExit('Usage: with-test-keyring.py COMMAND [ARG ...]')
    if not os.environ.get('DBUS_SESSION_BUS_ADDRESS'):
        raise SystemExit('with-test-keyring.py must run inside dbus-run-session')
    with tempfile.TemporaryDirectory(prefix='overtalk-test-keyring-') as directory, \
            (Path(directory)/'keyring.log').open('w+') as log:
        # A new login keyring is created and unlocked with this throwaway password.
        password, writer = os.pipe()
        os.write(writer, b'overtalk-test')
        os.close(writer)
        daemon = subprocess.Popen(
            ['gnome-keyring-daemon', '--foreground', '--unlock', '--components=secrets'],
            stdin=password, stdout=log, stderr=log,
            env=dict(os.environ, XDG_DATA_HOME=directory, XDG_CONFIG_HOME=directory))
        os.close(password)
        try:
            deadline = time.monotonic() + 15
            while not owned('org.freedesktop.secrets'):
                if daemon.poll() is not None or time.monotonic() >= deadline:
                    log.seek(0)
                    raise RuntimeError('Private test keyring failed: ' + log.read()[-4000:])
                time.sleep(.05)
            return subprocess.call(sys.argv[1:])
        finally:
            daemon.terminate()
            try:
                daemon.wait(timeout=10)
            except subprocess.TimeoutExpired:
                daemon.kill()
                daemon.wait()


if __name__ == '__main__':
    raise SystemExit(main())
