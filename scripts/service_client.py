"""Synchronous native-service client for isolated verification profiles.

Shared by security, workspace and performance checks; not shipped with the app.
Callers own the disposable profile directory and must close the client.
"""
import collections
import json
import os
import queue
import subprocess
import threading
import time


class Service:
    def __init__(self, binary, root):
        self.messages = queue.Queue()
        self.sequence = 0
        # Long-running commands (model downloads) may raise this per client.
        self.reply_timeout = 60
        # Events that arrive while a reply is awaited, kept for wait_for_event.
        self.events = collections.deque(maxlen=256)
        env = dict(os.environ, XDG_DATA_HOME=str(root / "data"),
                   XDG_CONFIG_HOME=str(root / "config"), XDG_CACHE_HOME=str(root / "cache"))
        env.setdefault("DISPLAY", ":1")
        env.setdefault("DBUS_SESSION_BUS_ADDRESS", "unix:path=/run/user/1000/bus")
        self.log = (root / "service.log").open("a")
        self.process = subprocess.Popen([str(binary), "--stdio"], stdin=subprocess.PIPE,
                                        stdout=subprocess.PIPE, stderr=self.log, env=env)
        def collect():
            for line in self.process.stdout:
                if line.startswith(b"OTRPC1 "):
                    self.messages.put(json.loads(line[7:]))
        threading.Thread(target=collect, daemon=True).start()
        deadline = time.monotonic() + 30
        while time.monotonic() < deadline:
            if self.messages.get(timeout=30).get("kind") == "ready":
                return
        raise RuntimeError("Service did not start")

    def call(self, command, **args):
        self.sequence += 1
        packet = dict(id=self.sequence, command=command, args=args)
        self.process.stdin.write(b"OTRPC1 " + json.dumps(packet).encode() + b"\n")
        self.process.stdin.flush()
        deadline = time.monotonic() + self.reply_timeout
        while time.monotonic() < deadline:
            response = self.messages.get(timeout=self.reply_timeout)
            if response.get("kind") == "reply" and response.get("id") == self.sequence:
                if response.get("error"):
                    raise RuntimeError(response["error"])
                return response.get("result")
            if "event" in response:
                self.events.append(response)
        raise TimeoutError(command)

    def wait_for_event(self, names, timeout):
        """Return the first event named in `names`, including one seen during a call."""
        for event in list(self.events):
            if event["event"] in names:
                self.events.remove(event)
                return event
        deadline = time.monotonic() + timeout
        while (remaining := deadline - time.monotonic()) > 0:
            try:
                message = self.messages.get(timeout=remaining)
            except queue.Empty:
                break
            if message.get("event") in names:
                return message
        raise TimeoutError(", ".join(sorted(names)))

    def close(self):
        if self.process.poll() is None:
            self.process.stdin.write(b'OTRPC1 {"id":999999,"command":"shutdown","args":{}}\n')
            self.process.stdin.flush()
            try:
                self.process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                self.process.terminate()
                self.process.wait(timeout=5)
        self.log.close()
