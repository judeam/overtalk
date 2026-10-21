"""Gemini 3.8 TTS with resumable assets and a conservative pre-credit cost ledger.

Only synthetic voice descriptions are accepted; this tool does not clone people.
OAuth tokens stay in memory. Unknown/failed calls retain their full reservation.
"""
from __future__ import annotations

import base64
from contextlib import contextmanager
from datetime import datetime, timezone
import hashlib
import io
import json
import math
import os
import re
from pathlib import Path
import sqlite3
import subprocess
import threading
import time
import urllib.error
import urllib.request
import uuid
import wave

MODEL = "gemini-3.8-flash-tts"
MAX_AUDIO_TOKENS = 16384
AUDIO_MICRO_USD_PER_TOKEN = 18
INPUT_MICRO_USD_PER_TOKEN = 1


def atomic_json(path: Path, value: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(path.name + "." + uuid.uuid4().hex + ".tmp")
    temporary.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    temporary.chmod(0o600)
    os.replace(temporary, path)


class BudgetExceeded(RuntimeError):
    pass


class AuthenticationRejected(RuntimeError):
    pass


class Ledger:
    def __init__(self, path: Path, budget_usd: float = 100):
        self.path = path
        self.limit = math.floor(budget_usd * 1_000_000)
        path.parent.mkdir(parents=True, exist_ok=True)
        with self.connect() as db:
            db.execute("CREATE TABLE IF NOT EXISTS calls (id TEXT PRIMARY KEY, kind TEXT, asset_key TEXT, status TEXT, reserved INTEGER, charged INTEGER, usage TEXT, created REAL)")
        path.chmod(0o600)

    def connect(self):
        return sqlite3.connect(self.path, timeout=30)

    def reserve(self, kind: str, key: str, input_bytes: int) -> str:
        # Bytes upper-bound normal tokenization. Reserve the model's entire
        # documented audio output limit before each call, not a guessed duration.
        amount = MAX_AUDIO_TOKENS * AUDIO_MICRO_USD_PER_TOKEN + input_bytes * INPUT_MICRO_USD_PER_TOKEN
        with self.connect() as db:
            db.execute("BEGIN IMMEDIATE")
            used = db.execute("SELECT coalesce(sum(coalesce(charged,reserved)),0) FROM calls").fetchone()[0]
            if used + amount > self.limit:
                raise BudgetExceeded(f"Stopping before the ${self.limit / 1e6:.2f} API budget: ${used / 1e6:.2f} spent/reserved; next worst-case reservation ${amount / 1e6:.2f}")
            call_id = uuid.uuid4().hex
            db.execute("INSERT INTO calls VALUES (?,?,?,?,?,NULL,NULL,?)", (call_id, kind, key, "reserved", amount, time.time()))
            return call_id

    def settle(self, call_id: str, input_tokens: int, output_tokens: int, usage: dict):
        with self.connect() as db:
            reserved = db.execute("SELECT reserved FROM calls WHERE id=?", (call_id,)).fetchone()[0]
            charged = input_tokens * INPUT_MICRO_USD_PER_TOKEN + output_tokens * AUDIO_MICRO_USD_PER_TOKEN
            if charged > reserved:
                # Never conceal changed provider accounting or an output-limit breach.
                db.execute("UPDATE calls SET charged=?,status='accounting_error',usage=? WHERE id=?", (charged, json.dumps(usage), call_id))
                db.commit()
                raise RuntimeError("Provider usage exceeded the reserved maximum; generation stopped")
            db.execute("UPDATE calls SET charged=?,status='completed',usage=? WHERE id=?", (charged, json.dumps(usage), call_id))

    def report(self):
        with self.connect() as db:
            rows = db.execute("SELECT status,count(*),sum(coalesce(charged,reserved)) FROM calls GROUP BY status").fetchall()
        return {"budget_usd": self.limit / 1e6, "accounted_usd": sum(r[2] for r in rows) / 1e6,
                "calls": [{"status": r[0], "count": r[1], "usd": r[2] / 1e6} for r in rows],
                "basis": "USD before tax and promotional credits; unknown calls retain worst-case reservations"}

    def pending(self, within_seconds=600):
        with self.connect() as db:
            return db.execute("SELECT count(*) FROM calls WHERE status='reserved' AND created>?", (time.time()-within_seconds,)).fetchone()[0]

    def unknown(self, call_id):
        with self.connect() as db:
            db.execute("UPDATE calls SET status='unknown' WHERE id=? AND charged IS NULL", (call_id,))

    def reject_auth(self, call_id: str, evidence: dict):
        # An explicit UNAUTHENTICATED response rejected the request before model
        # execution. Network errors and other HTTP responses remain uncertain.
        if evidence.get("code") != 401 or evidence.get("status") != "UNAUTHENTICATED":
            raise ValueError("Only an explicit authentication rejection can release this reservation")
        with self.connect() as db:
            db.execute("UPDATE calls SET status='auth_rejected',charged=0,usage=? "
                       "WHERE id=? AND charged IS NULL AND status IN ('reserved','unknown')",
                       (json.dumps({"http_status": 401, "status": "UNAUTHENTICATED"}), call_id))


def config_from_env_file(path: Path) -> dict:
    allowed = {"GOOGLE_CLOUD_PROJECT", "GOOGLE_CLOUD_ACCOUNT", "GOOGLE_CLOUD_LOCATION"}
    config = {}
    for line in path.read_text(encoding="utf-8").splitlines():
        key, separator, value = line.partition("=")
        if separator and key.strip() in allowed:
            config[key.strip()] = value.strip().strip("\"'")
    for key in allowed:
        if key in os.environ:
            config[key] = os.environ[key]
    if not config.get("GOOGLE_CLOUD_PROJECT"):
        raise ValueError("GOOGLE_CLOUD_PROJECT is required")
    if not re.fullmatch(r"[a-z][a-z0-9-]{4,61}[a-z0-9]", config["GOOGLE_CLOUD_PROJECT"]):
        raise ValueError("Invalid Google Cloud project ID")
    if config.get("GOOGLE_CLOUD_LOCATION", "global") != "global":
        raise ValueError("Voice design requires the global location")
    return config


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        raise RuntimeError("Refusing to redirect an authenticated speech request")


def wav_bytes(data: bytes, mime: str) -> bytes:
    if mime.lower().startswith("audio/l16"):
        output = io.BytesIO()
        with wave.open(output, "wb") as audio:
            audio.setparams((1, 2, 24000, 0, "NONE", "not compressed"))
            audio.writeframes(data)
        data = output.getvalue()
    with wave.open(io.BytesIO(data), "rb") as audio:
        if audio.getnchannels() != 1 or audio.getsampwidth() != 2 or audio.getframerate() != 24000:
            raise ValueError("Unexpected Gemini PCM format")
        if audio.getnframes() < 2400:
            raise ValueError("Speech response is empty or too short")
    return data


class Gemini:
    def __init__(self, config: dict, root: Path, budget_usd=100, *, request_timeout=120):
        if not 10 <= request_timeout <= 600:
            raise ValueError("Request timeout must be between 10 and 600 seconds")
        self.config, self.root = config, root
        self.request_timeout = request_timeout
        root.mkdir(parents=True, exist_ok=True)
        root.chmod(0o700)
        self.ledger = Ledger(root / "cost.sqlite", budget_usd)
        self.token, self.token_until = None, 0.0
        self.token_lock = threading.Lock()
        self.opener = urllib.request.build_opener(NoRedirect())

    def access_token(self):
        with self.token_lock:
            if self.token and self.token_until > time.monotonic():
                return self.token
            # print-access-token may return a previously cached token with only
            # seconds left. Read the actual expiry and refresh before long calls.
            args = ["gcloud", "config", "config-helper", "--format=json", "--force-auth-refresh",
                    "--project", self.config["GOOGLE_CLOUD_PROJECT"]]
            if self.config.get("GOOGLE_CLOUD_ACCOUNT"):
                args.extend(["--account", self.config["GOOGLE_CLOUD_ACCOUNT"]])
            credential = json.loads(subprocess.check_output(args, text=True, timeout=45, stderr=subprocess.PIPE))["credential"]
            token = credential.get("access_token", "")
            if not token or any(c.isspace() for c in token):
                raise RuntimeError("gcloud did not return a valid token")
            expiry = credential.get("token_expiry")
            remaining = (datetime.fromisoformat(expiry.replace("Z", "+00:00")) - datetime.now(timezone.utc)).total_seconds() if expiry else 600
            if remaining <= 300:
                raise RuntimeError("Refreshed Google credential expires too soon for audio generation")
            self.token, self.token_until = token, time.monotonic() + min(45 * 60, remaining - 300)
            return token

    def request(self, kind: str, key: str, path: str, body: dict) -> tuple[dict, str, int]:
        for attempt in range(2):
            try:
                return self._request_once(kind, key, path, body)
            except AuthenticationRejected:
                if attempt:
                    raise
        raise AssertionError("Unreachable authentication retry")

    def _request_once(self, kind: str, key: str, path: str, body: dict) -> tuple[dict, str, int]:
        gate = getattr(self, "request_gate", None)
        if gate is not None:
            gate()
        token = self.access_token()  # Authentication failures cannot consume a reservation.
        payload = json.dumps(body, ensure_ascii=False).encode()
        deadline = time.monotonic() + self.request_timeout + 10
        while True:
            try:
                call_id = self.ledger.reserve(kind, key, len(payload))
                break
            except BudgetExceeded:
                # Wait for in-flight reservations to settle before deciding the
                # cap is exhausted. Never increase or reduce the reservation.
                if not self.ledger.pending(self.request_timeout + 10) or time.monotonic() >= deadline:
                    raise
                time.sleep(1)
        project = self.config["GOOGLE_CLOUD_PROJECT"]
        request = urllib.request.Request(f"https://aiplatform.googleapis.com/v1beta1/projects/{project}/locations/global/{path}",
            data=payload, headers={"Authorization": "Bearer " + token, "Content-Type": "application/json", "x-goog-user-project": project})
        try:
            with self.opener.open(request, timeout=self.request_timeout) as response:
                result = json.load(response)
        except urllib.error.HTTPError as error:
            self.ledger.unknown(call_id)
            detail = error.read(16384).decode("utf-8", errors="replace")
            atomic_json(self.root / "errors" / (call_id + ".json"), {"http_status": error.code, "detail": detail, "kind": kind, "asset_key": key})
            # Do not print the request or credential-bearing headers.
            try:
                evidence = json.loads(detail).get("error", {})
                message = evidence.get("message", "")[:500]
            except ValueError:
                evidence = {}
                message = ""
            if error.code == 401 and evidence.get("code") == 401 and evidence.get("status") == "UNAUTHENTICATED":
                self.ledger.reject_auth(call_id, evidence)
                with self.token_lock:
                    # Other workers may already have refreshed this token.
                    if self.token == token:
                        self.token_until = 0
                raise AuthenticationRejected("Google rejected the audio credential; refreshed authentication will be retried once") from None
            raise RuntimeError(f"Gemini returned HTTP {error.code}: {message}; reservation retained for cost safety") from None
        except Exception:
            self.ledger.unknown(call_id)
            raise
        return result, call_id, len(payload)

    def account(self, result: dict, call_id: str, input_bytes: int, audio: bytes):
        usage = result.get("usageMetadata", result.get("usage", {}))
        input_tokens = usage.get("promptTokenCount", usage.get("input_tokens", input_bytes))
        output_tokens = usage.get("candidatesTokenCount", usage.get("output_tokens"))
        with wave.open(io.BytesIO(audio), "rb") as reader:
            audio_tokens = math.ceil(reader.getnframes() / reader.getframerate() * 25)
        output_tokens = max(audio_tokens, output_tokens or 0)
        self.ledger.settle(call_id, int(input_tokens), int(output_tokens), usage)

    @contextmanager
    def asset_lock(self, value):
        key = hashlib.sha256(json.dumps(value, sort_keys=True, ensure_ascii=False).encode()).hexdigest()
        folder = self.root / "locks"
        folder.mkdir(exist_ok=True)
        with (folder / (key + ".lock")).open("a+b") as lock:
            if os.name == "nt":
                import msvcrt
                lock.seek(0)
                if not lock.read(1):
                    lock.write(b"0")
                    lock.flush()
                while True:
                    lock.seek(0)
                    try:
                        msvcrt.locking(lock.fileno(), msvcrt.LK_NBLCK, 1)
                        break
                    except OSError:
                        time.sleep(.1)
                try:
                    yield
                finally:
                    lock.seek(0)
                    msvcrt.locking(lock.fileno(), msvcrt.LK_UNLCK, 1)
            else:
                import fcntl
                fcntl.flock(lock, fcntl.LOCK_EX)
                try:
                    yield
                finally:
                    fcntl.flock(lock, fcntl.LOCK_UN)

    def voice(self, identity: dict) -> str:
        with self.asset_lock(["voice", identity]):
            return self._voice(identity)

    def _voice(self, identity: dict) -> str:
        key = hashlib.sha256(json.dumps(identity, sort_keys=True).encode()).hexdigest()
        path = self.root / "voices" / (key + ".json")
        if path.exists():
            return json.loads(path.read_text())["id"]
        body = {"store": True, "voice": {"type": "VOICE_TYPE_PROMPTED",
            "displayName": "OvertalkEval-v1-" + key[:12], "languageCode": identity["language"],
            "prompted": {"input": identity["description"]}}}
        result, call_id, size = self.request("voice_design", key, "voices", body)
        sample = result.get("sample_audio", result.get("sampleAudio", {}))
        data = wav_bytes(base64.b64decode(sample["data"], validate=True), sample.get("mime_type", sample.get("mimeType", "audio/l16")))
        if not str(result.get("id", "")).startswith("voice_"):
            raise RuntimeError("Voice creation did not return a stored voice ID")
        self.account(result, call_id, size, data)
        path.parent.mkdir(parents=True, exist_ok=True)
        path.with_suffix(".wav").write_bytes(data)
        atomic_json(path, {"id": result["id"], "identity": identity, "call_id": call_id})
        return result["id"]

    def speak(self, text: str, voice: str, style: str) -> tuple[Path, float]:
        with self.asset_lock(["speech", text, voice, style]):
            return self._speak(text, voice, style)

    def _speak(self, text: str, voice: str, style: str) -> tuple[Path, float]:
        body = {"contents": [{"role": "user", "parts": [{"text": text, "speechMetadata": {"style": style}}]}],
                "generationConfig": {"responseModalities": ["AUDIO"], "speechConfig": {"voiceConfig": {"voice": voice}}}}
        key = hashlib.sha256(json.dumps(body, sort_keys=True, ensure_ascii=False).encode()).hexdigest()
        path = self.root / "clips" / (key + ".wav")
        if not path.exists():
            result, call_id, size = self.request("speech", key, f"publishers/google/models/{MODEL}:generateContent", body)
            part = next(p["inlineData"] for p in result["candidates"][0]["content"]["parts"] if "inlineData" in p)
            data = wav_bytes(base64.b64decode(part["data"], validate=True), part["mimeType"])
            if not result.get("modelVersion", MODEL).startswith(MODEL):
                raise RuntimeError("Speech model mismatch")
            self.account(result, call_id, size, data)
            if result["candidates"][0].get("finishReason") == "MAX_TOKENS":
                atomic_json(self.root / "errors" / (call_id + ".json"), {"reason": "truncated_audio", "asset_key": key})
                raise RuntimeError("TTS reached its output limit; truncated audio cannot enter the evaluation corpus")
            path.parent.mkdir(parents=True, exist_ok=True)
            temporary = path.with_suffix(".tmp")
            temporary.write_bytes(data)
            os.replace(temporary, path)
            atomic_json(path.with_suffix(".json"), {"body": body, "call_id": call_id, "model": result.get("modelVersion", MODEL), "sha256": hashlib.sha256(data).hexdigest()})
        with wave.open(str(path), "rb") as audio:
            duration = audio.getnframes() / audio.getframerate()
        return path, duration
