"""Run a Linux CI command with a private null audio server, leaving host audio alone."""
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time


def main():
    if len(sys.argv) < 2:
        raise SystemExit('Usage: with-test-audio.py COMMAND [ARG ...]')
    with tempfile.TemporaryDirectory(prefix='overtalk-test-audio-') as directory:
        root = Path(directory)
        socket = root / 'native'
        config = root / 'default.pa'
        config.write_text(
            f'load-module module-native-protocol-unix socket={socket} auth-anonymous=1\n'
            'load-module module-null-sink sink_name=overtalk_test rate=16000 channels=1\n'
            'set-default-sink overtalk_test\n'
            'set-default-source overtalk_test.monitor\n'
        )
        with (root/'pulse.log').open('w+') as log:
            server = subprocess.Popen(['pulseaudio', '--daemonize=no', '--exit-idle-time=-1',
                                       '--use-pid-file=no', '-nF', str(config)], stdout=log, stderr=log)
            try:
                deadline = time.monotonic() + 15
                while not socket.exists():
                    if server.poll() is not None or time.monotonic() >= deadline:
                        log.seek(0)
                        raise RuntimeError('Private test audio server failed: ' + log.read()[-4000:])
                    time.sleep(.05)
                env = dict(os.environ, PULSE_SERVER='unix:' + str(socket))
                return subprocess.call(sys.argv[1:], env=env)
            finally:
                server.terminate()
                try:
                    server.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    server.kill()
                    server.wait()


if __name__ == '__main__':
    raise SystemExit(main())
