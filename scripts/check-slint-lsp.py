"""Ask the real Slint language server to check native UI files; no preview listener."""
import argparse
import json
import os
from pathlib import Path
import queue
import subprocess
import threading
import time
import tempfile

root = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("files", nargs="*")
args = parser.parse_args()
files = [Path(name).resolve() for name in args.files] or sorted((root / "native/ui").rglob("*.slint"))
executable = root / ".tools/slint/bin" / ("slint-lsp.exe" if os.name == "nt" else "slint-lsp")
if not executable.exists():
    raise SystemExit("Run python3 scripts/setup-native-tools.py first")
diagnostic_log = tempfile.TemporaryFile()
process = subprocess.Popen([str(executable)], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=diagnostic_log, cwd=root)
inbox = queue.Queue()


def read_messages():
    try:
        while True:
            header = process.stdout.readline()
            if not header:
                try:
                    code = process.wait(timeout=1)
                except subprocess.TimeoutExpired:
                    code = "stdout closed"
                diagnostic_log.seek(0)
                detail = diagnostic_log.read(16384).decode("utf-8", errors="replace")
                inbox.put({"reader_error": f"Slint LSP exited ({code}): {detail}"})
                return
            length = None
            while header.strip():
                if header.lower().startswith(b"content-length:"):
                    length = int(header.split(b":", 1)[1])
                header = process.stdout.readline()
            if length is None or length > 32 * 1024 * 1024:
                raise RuntimeError("Invalid LSP message length")
            inbox.put(json.loads(process.stdout.read(length)))
    except Exception as error:
        inbox.put({"reader_error": str(error)})


def send(message):
    body = json.dumps({"jsonrpc": "2.0", **message}).encode()
    process.stdin.write(f"Content-Length: {len(body)}\r\n\r\n".encode() + body)
    process.stdin.flush()


def receive(timeout=30):
    end = time.monotonic() + timeout
    while time.monotonic() < end:
        message = inbox.get(timeout=max(0.01, end - time.monotonic()))
        if "reader_error" in message:
            raise RuntimeError(message["reader_error"])
        if "method" in message and "id" in message:
            result = [None] * len(message.get("params", {}).get("items", [])) if message["method"] == "workspace/configuration" else None
            send({"id": message["id"], "result": result})
            continue
        return message
    raise TimeoutError("Slint LSP did not respond")


threading.Thread(target=read_messages, daemon=True).start()
try:
    send({"id": 1, "method": "initialize", "params": {"processId": os.getpid(), "rootUri": root.as_uri(), "capabilities": {}}})
    while receive().get("id") != 1:
        pass
    send({"method": "initialized", "params": {}})
    pending = {file.as_uri() for file in files}
    for file in files:
        send({"method": "textDocument/didOpen", "params": {"textDocument": {"uri": file.as_uri(), "languageId": "slint", "version": 1, "text": file.read_text(encoding="utf-8")}}})
    send({"id": 10, "method": "textDocument/documentSymbol", "params": {"textDocument": {"uri": files[-1].as_uri()}}})
    symbol_pending = True
    diagnostics = {}
    while pending or symbol_pending:
        message = receive()
        if message.get("id") == 10:
            symbol_pending = False
        if "reader_error" in message:
            raise RuntimeError(message["reader_error"])
        if message.get("method") != "textDocument/publishDiagnostics":
            continue
        params = message["params"]
        pending.discard(params["uri"])
        diagnostics[params["uri"]] = params["diagnostics"]
    # The server can publish an empty list before its completed analysis.
    while True:
        try:
            message = receive(timeout=0.3)
        except queue.Empty:
            break
        if message.get("method") == "textDocument/publishDiagnostics":
            diagnostics[message["params"]["uri"]] = message["params"]["diagnostics"]
    errors = 0
    for uri, items in diagnostics.items():
        for diagnostic in items:
            location = diagnostic["range"]["start"]
            print(f"{uri}:{location['line'] + 1}:{location['character'] + 1}: {diagnostic['message']}")
            errors += diagnostic.get("severity", 1) == 1
    print(f"Slint LSP checked {len(files)} files: {errors} errors")
    send({"id": 2, "method": "shutdown", "params": None})
    while receive().get("id") != 2:
        pass
    send({"method": "exit", "params": None})
    raise SystemExit(bool(errors))
finally:
    try:
        process.wait(timeout=3)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait()
