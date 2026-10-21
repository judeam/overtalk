"""Newline-delimited JSON protocol. stdout is reserved for responses."""

from __future__ import annotations

import contextlib
import base64
import binascii
import json
import math
import os
from pathlib import Path
import subprocess
import struct
import sys
import threading
import time
from typing import Any, TextIO

from .asr import ASREngine, SpeechError, join_word_text, local_audio_path

MAX_REQUEST_BYTES = 64 * 1024
NEMOTRON_LOCALES = {
    "en": "en-US", "es": "es-ES", "fr": "fr-FR", "it": "it-IT", "pt": "pt-BR",
    "nl": "nl-NL", "de": "de-DE", "tr": "tr-TR", "ru": "ru-RU", "ar": "ar-AR",
    "hi": "hi-IN", "ja": "ja-JP", "ko": "ko-KR", "vi": "vi-VN", "uk": "uk-UA",
    "pl": "pl-PL", "sv": "sv-SE", "cs": "cs-CZ", "nb": "nb-NO", "no": "nb-NO",
    "da": "da-DK", "bg": "bg-BG", "fi": "fi-FI", "hr": "hr-HR", "sk": "sk-SK",
    "zh": "zh-CN", "hu": "hu-HU", "ro": "ro-RO", "et": "et-EE",
}
NEMOTRON_SUPPORTED_LOCALES = {*NEMOTRON_LOCALES.values(), "en-GB", "es-US", "fr-CA", "pt-PT"}


def configure_parent_death_guard() -> None:
    """Isolated inference must stop when its supervising worker is cancelled.

    Arm inside the fresh child before importing ML runtimes. Avoid preexec_fn:
    forking a Torch process with threads and running Python in preexec can hang.
    """
    expected = os.environ.get("SONEFOLD_PARENT_PID")
    if expected is None:
        return
    if not expected.isdigit() or int(expected) < 1:
        raise SpeechError("Invalid internal speech-worker parent PID")
    parent_pid = int(expected)
    if sys.platform.startswith("linux"):
        import ctypes
        import signal
        if os.getppid() != parent_pid:
            os._exit(143)
        libc = ctypes.CDLL(None, use_errno=True)
        # PR_SET_PDEATHSIG. SIGKILL cannot be delayed by framework handlers.
        if libc.prctl(1, signal.SIGKILL, 0, 0, 0) != 0:
            raise SpeechError("Cannot supervise the isolated speech-worker lifetime")
        if os.getppid() != parent_pid:  # Close the parent-exit/arming race.
            os._exit(143)
        return
    if sys.platform == "win32":
        import ctypes
        from ctypes import wintypes
        kernel = ctypes.WinDLL("kernel32", use_last_error=True)
        kernel.OpenProcess.argtypes = (wintypes.DWORD, wintypes.BOOL, wintypes.DWORD)
        kernel.OpenProcess.restype = wintypes.HANDLE
        kernel.WaitForSingleObject.argtypes = (wintypes.HANDLE, wintypes.DWORD)
        kernel.WaitForSingleObject.restype = wintypes.DWORD
        kernel.CloseHandle.argtypes = (wintypes.HANDLE,)
        handle = kernel.OpenProcess(0x00100000, False, parent_pid)  # SYNCHRONIZE
        if not handle:
            raise SpeechError("Cannot supervise the isolated speech-worker parent")

        def wait_for_parent() -> None:
            kernel.WaitForSingleObject(handle, 0xFFFFFFFF)
            kernel.CloseHandle(handle)
            # WAIT_OBJECT_0 means parent exit; a failed wait also cannot leave
            # expensive inference running without a functioning supervisor.
            os._exit(143)
    else:
        def wait_for_parent() -> None:
            while os.getppid() == parent_pid:
                time.sleep(0.25)
            os._exit(143)
    threading.Thread(target=wait_for_parent, name="speech-parent-watch", daemon=True).start()


def streaming_language(value: Any, model: str) -> str:
    if value is None:
        return "auto"
    if not isinstance(value, str):
        raise SpeechError("Streaming language must be a language code/locale or null")
    language = value.strip().replace("_", "-")
    if language in {"", "auto"}:
        return "auto"
    if model == "multitalker-parakeet":
        if language.casefold() not in {"en", "en-us", "en-gb", "english"}:
            raise SpeechError("Multitalker Parakeet supports English; select Nemotron 3.5 for other locales")
        return "en-US"
    supported = {locale.casefold(): locale for locale in NEMOTRON_SUPPORTED_LOCALES}
    result = supported.get(language.casefold()) or NEMOTRON_LOCALES.get(language.casefold())
    if result is None:
        raise SpeechError(f"Nemotron 3.5 does not support '{language}' without language adaptation")
    return result


def speaker_counts(request: dict[str, Any]) -> dict[str, int | None]:
    counts = {}
    for name in ("num_speakers", "min_speakers", "max_speakers"):
        value = request.get(name)
        if value is not None and (isinstance(value, bool) or not isinstance(value, int) or not 1 <= value <= 32):
            raise SpeechError(f"{name} must be an integer from 1 to 32 or null")
        counts[name] = value
    minimum, maximum, number = (counts[name] for name in ("min_speakers", "max_speakers", "num_speakers"))
    if minimum is not None and maximum is not None and minimum > maximum:
        raise SpeechError("min_speakers must not exceed max_speakers")
    if number is not None and ((minimum is not None and number < minimum) or (maximum is not None and number > maximum)):
        raise SpeechError("num_speakers must be within the requested minimum/maximum")
    return counts


def analyze(request: Any, engine: ASREngine) -> dict[str, Any]:
    if not isinstance(request, dict):
        raise SpeechError("A request must be a JSON object")
    request_id = request.get("id")
    if request_id is None or isinstance(request_id, (dict, list, bool)):
        raise SpeechError("id must be a string or number")
    if request.get("offline", True) is not True:
        raise SpeechError("The speech worker only supports offline analysis")
    if str(request.get("operation", "")).startswith("knowledge_"):
        from .retrieval import RetrievalEngine
        if not hasattr(engine, "_retrieval"):
            engine._retrieval = RetrievalEngine()
        return {"id": request_id, "ok": True, **engine._retrieval.handle(request)}
    if request.get("operation") == "ping":
        return {"id": request_id, "ok": True, "version": "0.1.0"}
    if request.get("operation") == "health":
        return {"id": request_id, "ok": True, **engine.health(request.get("model", "qwen3-asr-1.7b"))}
    if request.get("operation") == "stream_health":
        model = request.get("asr_model", "nemotron-3.5")
        return {"id": request_id, "ok": True, **engine.stream_health(model),
                "supported_languages": sorted(NEMOTRON_SUPPORTED_LOCALES) if model == "nemotron-3.5" else ["en-US"]}
    if request.get("operation") == "load":
        if engine._stream_session is not None:
            raise SpeechError("Stop the active streaming session before loading another model")
        return {"id": request_id, "ok": True, **engine.load(request.get("model", "qwen3-asr-1.7b"), request.get("device"))}
    if request.get("operation") == "shutdown":
        if engine._stream_session is not None:
            engine._stream_session.close()
            engine._stream_session = None
            engine._stream_model_name = None
        engine.unload()
        return {"id": request_id, "ok": True, "shutdown": True}
    if request.get("operation") in {"stream_start", "stream_audio", "stream_stop"}:
        return {"id": request_id, "ok": True, **stream(request, engine), "error": None}
    if request.get("operation") == "diarize":
        path = local_audio_path(request.get("audio_path"))
        counts = speaker_counts(request)
        engine.configure_device(request.get("device"))
        engine._runtime()
        from .diarization import diarize as run_diarization
        # Separate interpreters receive the already normalized local audio file.
        from .asr import decodable_audio
        import soundfile
        with decodable_audio(path, soundfile) as decoded:
            result = run_diarization(str(decoded), model_dir=os.environ.get("SONEFOLD_DIARIZATION_MODEL_DIR"),
                                    device=engine.device, model=request.get("diarization_model", "nemotron-3"), **counts)
        return {"id": request_id, "ok": True, **result, "error": None}
    if request.get("operation") != "analyze":
        raise SpeechError("Unsupported operation; use analyze, health, load, stream_start/audio/stop, ping, or shutdown")
    if engine._stream_session is not None:
        raise SpeechError("Stop the active streaming session before analyzing another recording")
    diarize = request.get("diarize", False)
    if not isinstance(diarize, bool):
        raise SpeechError("diarize must be a boolean")
    word_timestamps = request.get("word_timestamps", True)
    if not isinstance(word_timestamps, bool):
        raise SpeechError("word_timestamps must be a boolean")
    if diarize and not word_timestamps:
        raise SpeechError("Speaker analysis requires word timestamps")
    counts = speaker_counts(request)
    engine.configure_device(request.get("device"))
    hints = {"context": request["context"]} if request.get("context") else {}
    diarization = None
    if diarize and request.get("model") != "moss-transcribe-diarize":
        # Detect turns first so languages without a word aligner can decode
        # actual speaker regions. Keep the two models out of GPU memory together.
        engine.unload()
        diarization = run_diarization_request(request, engine, counts)
        hints["speaker_turns"] = diarization.get("exclusive_turns") or diarization["turns"]
    try:
        result = engine.transcribe(request.get("audio_path"), request.get("model", "qwen3-asr-1.7b"), request.get("language"),
                                   word_timestamps=word_timestamps, **hints)
    finally:
        if diarization is not None:
            engine.unload()  # Also release failed inference before the notes model loads.
    result.setdefault("turns",[]);result.setdefault("exclusive_turns",[])
    if diarization is not None:
        from .diarization import assign_speakers
        assigned = assign_speakers(result["segments"], diarization["turns"], diarization.get("exclusive_turns"))
        result["segments"] = split_speaker_turns(assigned, result.get("language"))
        result["speakers"] = diarization["speakers"]
        result["turns"] = diarization["turns"]
        result["exclusive_turns"] = diarization.get("exclusive_turns", [])
        result["diarization_model"] = diarization["model"]
    return {"id": request_id, "ok": True, **result, "error": None}


def stream(request: dict[str, Any], engine: ASREngine) -> dict[str, Any]:
    operation = request["operation"]
    if operation == "stream_start":
        if engine._stream_session is not None:
            raise SpeechError("A streaming session is already active; stop it before starting another")
        asr_path = os.environ.get("SONEFOLD_STREAMING_ASR_MODEL_PATH")
        diar_path = os.environ.get("SONEFOLD_DIARIZATION_MODEL_DIR")
        if not asr_path or not diar_path:
            raise SpeechError("Set SONEFOLD_STREAMING_ASR_MODEL_PATH and SONEFOLD_DIARIZATION_MODEL_DIR to local NeMo checkpoints")
        counts = speaker_counts(request)
        if counts["num_speakers"] is not None or counts["min_speakers"] is not None:
            raise SpeechError("Streaming diarization accepts a maximum speaker count, not an exact/minimum count")
        maximum = counts["max_speakers"] or 8
        if maximum > 8:
            raise SpeechError("The streaming Nemotron diarizer supports at most eight speakers")
        asr_model = request.get("asr_model", "nemotron-3.5")
        if asr_model not in {"multitalker-parakeet", "nemotron-3.5"}:
            raise SpeechError("Streaming ASR must be multitalker-parakeet or nemotron-3.5")
        language = streaming_language(request.get("language"), asr_model)
        engine.configure_device(request.get("device"))
        engine.unload()
        engine._runtime()
        from .streaming import StreamingSession
        engine._stream_session = StreamingSession(asr_path, diar_path, device=engine.device,
                                                  max_speakers=maximum, asr_model=asr_model, language=language)
        engine._stream_model_name = asr_model
        return {"started": True, "is_partial": True, "model": asr_model, "language": language}
    if engine._stream_session is None:
        raise SpeechError("Start a streaming session before sending audio")
    if operation == "stream_stop":
        try:
            return engine._stream_session.finish()
        finally:
            engine._stream_session.close()
            engine._stream_session = None
            engine._stream_model_name = None
    sample_rate = request.get("sample_rate", 16000)
    if isinstance(sample_rate, bool) or sample_rate != 16000:
        raise SpeechError("Streaming audio must be mono 16000 Hz PCM")
    if "pcm16_base64" in request:
        if "samples" in request:
            raise SpeechError("Provide either pcm16_base64 or samples")
        try:
            raw = base64.b64decode(request["pcm16_base64"], validate=True)
            if len(raw) % 2 or not 0 < len(raw) <= 32000:
                raise ValueError("Invalid PCM block size")
            samples = [value[0] / 32768.0 for value in struct.iter_unpack("<h", raw)]
        except (TypeError, ValueError, binascii.Error) as exc:
            raise SpeechError("pcm16_base64 must contain 1–16000 little-endian signed PCM16 samples") from exc
    else:
        samples = request.get("samples")
        import math
        if (not isinstance(samples, list) or not 0 < len(samples) <= 16000 or
                any(isinstance(value, bool) or not isinstance(value, (int, float)) or
                    not math.isfinite(value) or not -1 <= value <= 1 for value in samples)):
            raise SpeechError("samples must contain 1–16000 finite mono float samples in [-1,1]")
    return engine._stream_session.accept_audio(samples, sample_rate=sample_rate)


def split_speaker_turns(segments: list[dict[str, Any]], language: str | None) -> list[dict[str, Any]]:
    """Split aligned phrases when speakers change, keeping unknown words visible."""
    result = []
    for segment in segments:
        words = segment.get("words")
        if not words or len({word.get("speaker") for word in words}) < 2:
            result.append(segment)
            continue
        groups: list[list[dict[str, Any]]] = []
        for word in words:
            if not groups or groups[-1][-1].get("speaker") != word.get("speaker"):
                groups.append([])
            groups[-1].append(word)
        for group in groups:
            confidence = [word.get("speaker_confidence") for word in group if word.get("speaker_confidence") is not None]
            result.append({**segment, "text": join_word_text(group, segment.get("language") or language),
                           "start": group[0]["start"], "end": group[-1]["end"],
                           "speaker": group[0].get("speaker"), "words": group,
                           "speaker_confidence": sum(confidence) / len(confidence) if confidence else None,
                           "overlap": any(word.get("overlap", False) for word in group),
                           "speaker_ambiguous": any(word.get("speaker_ambiguous", False) for word in group)})
    return result


def run_diarization_request(request: dict[str, Any], engine: ASREngine, counts: dict[str, Any]) -> dict[str, Any]:
    python = os.environ.get("SONEFOLD_DIARIZATION_PYTHON")
    if python:
        if not Path(python).is_file():
            raise SpeechError("SONEFOLD_DIARIZATION_PYTHON must point to a Python executable")
        environment = dict(os.environ)
        environment.pop("SONEFOLD_DIARIZATION_PYTHON", None)
        environment["SONEFOLD_PARENT_PID"] = str(os.getpid())
        root = str(Path(__file__).resolve().parent.parent)
        environment["PYTHONPATH"] = root + os.pathsep + environment.get("PYTHONPATH", "")
        child_request = {"id": request["id"], "operation": "diarize", "offline": True,
                         "audio_path": request["audio_path"], "device": engine.device,
                         "diarization_model": request.get("diarization_model", "nemotron-3"), **counts}
        process = subprocess.run([python, "-m", "sonefold_speech"],
                                 input=json.dumps(child_request) + "\n", text=True,
                                 stdout=subprocess.PIPE, stderr=sys.stderr, env=environment,
                                 timeout=7200, check=True)
        try:
            result = json.loads(process.stdout)
        except ValueError as exc:
            raise SpeechError("The isolated diarization worker returned an invalid response") from exc
        if not result.get("ok"):
            raise SpeechError(result.get("error") or "Isolated diarization failed")
        return result
    from .diarization import diarize as run_diarization
    from .asr import decodable_audio
    import soundfile
    with decodable_audio(local_audio_path(request["audio_path"]), soundfile) as decoded:
        return run_diarization(str(decoded), model_dir=os.environ.get("SONEFOLD_DIARIZATION_MODEL_DIR"),
                              device=engine.device, model=request.get("diarization_model", "nemotron-3"), **counts)


def serve(stdin: TextIO, stdout: TextIO, engine: ASREngine | None = None) -> None:
    def finite_float(value: str) -> float:
        result = float(value)
        if not math.isfinite(result):
            raise ValueError("Nonfinite JSON number")
        return result

    engine = engine or ASREngine()
    while line := stdin.readline(MAX_REQUEST_BYTES + 1):
        if not line.strip():
            continue
        request: Any = None
        try:
            if len(line.encode("utf-8")) > MAX_REQUEST_BYTES:
                # Drain the oversized line, so it cannot become a second request.
                while not line.endswith("\n"):
                    line = stdin.readline(MAX_REQUEST_BYTES + 1)
                    if not line:
                        break
                raise SpeechError("Request exceeds the 64 KiB limit")
            request = json.loads(line, parse_float=finite_float,
                                 parse_constant=lambda value: (_ for _ in ()).throw(ValueError("Nonfinite JSON number")))
            # Model libraries often print download/runtime messages. Redirect them
            # before importing or invoking any model to protect stdout framing.
            with contextlib.redirect_stdout(sys.stderr):
                response = analyze(request, engine)
        except Exception as exc:
            request_id = request.get("id") if isinstance(request, dict) else None
            if type(request_id) not in (str, int, float):
                request_id = None
            model = request.get("model") if isinstance(request, dict) else None
            message = str(exc) if isinstance(exc, (SpeechError, ValueError, ImportError)) else f"Speech analysis failed ({type(exc).__name__}): {exc}"
            response = {"id": request_id, "ok": False, "segments": [], "speakers": [],
                        "turns": [], "exclusive_turns": [],
                        "model": model if isinstance(model, str) else None,
                        "language": None, "error": message}
        stdout.write(json.dumps(response, ensure_ascii=False, allow_nan=False) + "\n")
        stdout.flush()
        if response.get("shutdown"):
            break


def main() -> None:
    configure_parent_death_guard()
    # Reserve a duplicate of the original stdout pipe for JSON. Redirect fd 1
    # permanently so native C/CUDA libraries also log to stderr, not just print().
    with os.fdopen(os.dup(sys.stdout.fileno()), "w", encoding="utf-8", buffering=1) as protocol:
        os.dup2(sys.stderr.fileno(), sys.stdout.fileno())
        sys.stdout = sys.stderr
        serve(sys.stdin, protocol)


if __name__ == "__main__":
    main()
