#!/usr/bin/env python3
"""Verify model-management commands against a synthetic loopback Ollama server.

Uses a temporary app profile; never contacts a real model registry or modifies
installed models. Tests the actual native dispatch and download lifecycle.
"""
from service_client import Service
import argparse
import http.server
import json
import tempfile
import threading
from pathlib import Path


class Ollama(http.server.BaseHTTPRequestHandler):
    mode = "success"
    requests = []

    def log_message(self, *_):
        pass

    def respond(self):
        size = int(self.headers.get("Content-Length", "0"))
        payload = json.loads(self.rfile.read(size)) if size else {}
        self.requests.append((self.path, payload))
        status = 200
        if self.path == "/api/tags":
            body = json.dumps({"models": [dict(name="synthetic", model="synthetic",
                size=1000, modified_at="2026-10-18T00:00:00Z")]})
        elif self.path == "/api/show":
            body = json.dumps(dict(modelfile="", details=dict(family="synthetic",
                parameter_size="4B"), model_info={"synthetic.context_length": 32768}))
            if self.mode == "redirect":
                status = 302
        elif self.path == "/api/pull":
            body = {"success": '{"completed":50,"total":100}\n{"status":"success"}',
                    "truncated": '{"completed":50,"total":100}\n',
                    "error": '{"error":"synthetic rejection"}\n'}[self.mode]
        else:
            body = "{}"
        data = body.encode()
        self.send_response(status)
        if status == 302:
            self.send_header("Location", "/redirect-target")
        self.send_header("Content-Length", str(len(data)))
        self.send_header("Content-Type", "application/json")
        self.end_headers()
        self.wfile.write(data)

    do_GET = respond
    do_POST = respond
    do_DELETE = respond


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--report", type=Path, required=True)
    args = parser.parse_args()
    checks = {}
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Ollama)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        with tempfile.TemporaryDirectory(prefix="overtalk-ollama-") as temp:
            service = Service(args.binary.resolve(), Path(temp))
            endpoint = f"http://127.0.0.1:{server.server_port}"
            try:
                models = service.call("get_ollama_models", endpoint=endpoint)
                assert models[0]["id"] == "synthetic", models
                checks["model_discovery"] = "passed"
                context = service.call("get_ollama_model_context", endpoint=endpoint, modelName="synthetic")
                assert context == 32768, context
                assert Ollama.requests[-1][1]["verbose"] is False
                requests = len(Ollama.requests)
                assert service.call("get_ollama_model_context", endpoint=endpoint, modelName="synthetic") == context
                assert len(Ollama.requests) == requests
                checks["bounded_metadata_and_cache"] = "passed"
                for mode in ["error", "truncated"]:
                    Ollama.mode = mode
                    try:
                        service.call("pull_ollama_model", endpoint=endpoint, modelName="synthetic")
                    except RuntimeError:
                        pass
                    else:
                        raise AssertionError(f"Accepted {mode} pull")
                    Ollama.mode = "success"
                    service.call("pull_ollama_model", endpoint=endpoint, modelName="synthetic")
                    checks[f"{mode}_pull_retry"] = "passed"
                service.call("delete_ollama_model", endpoint=endpoint, modelName="synthetic")
                assert Ollama.requests[-1] == ("/api/delete", {"name": "synthetic"})
                checks["delete_routing"] = "passed"
                Ollama.mode = "redirect"
                assert service.call("get_ollama_model_context", endpoint=endpoint, modelName="uncached") == 4000
                assert all(path != "/redirect-target" for path, _ in Ollama.requests)
                checks["redirect_refusal"] = "passed"
                for invalid in ["http://localhost.attacker.invalid", "https://user:password@example.invalid"]:
                    try:
                        service.call("delete_ollama_model", endpoint=invalid, modelName="synthetic")
                    except RuntimeError:
                        pass
                    else:
                        raise AssertionError("Accepted unsafe endpoint")
                checks["unsafe_endpoint_refusal"] = "passed"
            finally:
                service.close()
    finally:
        server.shutdown()
        server.server_close()
        thread.join()
    args.report.write_text(json.dumps(checks, indent=2) + "\n")
    print(json.dumps(checks, indent=2))


if __name__ == "__main__":
    main()
