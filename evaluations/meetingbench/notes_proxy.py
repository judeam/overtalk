#!/usr/bin/env python3
"""Private local stdio-to-socket adapter for the evaluation's shared helper."""
import json
import os
import socket
import sys

MAX_LINE = 32 * 1024 * 1024


def main():
    with socket.socket(socket.AF_UNIX) as connection:
        connection.connect(os.environ['OVERTALK_EVAL_HELPER_SOCKET'])
        with connection.makefile('rwb') as stream:
            while True:
                line = sys.stdin.buffer.readline(MAX_LINE + 1)
                if not line:
                    break
                if len(line) > MAX_LINE:
                    raise ValueError('Evaluation helper request is too large')
                stream.write(line)
                stream.flush()
                answer = stream.readline(MAX_LINE + 1)
                if not answer or len(answer) > MAX_LINE:
                    raise RuntimeError('Evaluation helper disconnected')
                sys.stdout.buffer.write(answer)
                sys.stdout.buffer.flush()
                if json.loads(line).get('type') == 'shutdown':
                    break


if __name__ == '__main__':
    main()
