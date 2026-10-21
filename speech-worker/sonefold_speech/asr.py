"""Bounded local ASR with lazy model imports and a single cached ASR model."""

from __future__ import annotations

import gc
import importlib
import importlib.util
import math
import os
import shutil
import subprocess
import tempfile
import difflib
import re
import unicodedata
from collections import deque
from bisect import bisect_right
from contextlib import contextmanager
from pathlib import Path
from typing import Any, Iterator

QWEN_MODELS = {
    "qwen3-asr-1.7b": ("Qwen/Qwen3-ASR-1.7B", "SONEFOLD_QWEN3_ASR_1_7B_DIR"),
    "qwen3-asr-0.6b": ("Qwen/Qwen3-ASR-0.6B", "SONEFOLD_QWEN3_ASR_0_6B_DIR"),
}
SUPPORTED_MODELS = (*QWEN_MODELS, "parakeet-tdt-0.6b-v3", "cohere-transcribe", "moss-transcribe-diarize")
QWEN_LANGUAGES = {
    "zh": "Chinese", "en": "English", "yue": "Cantonese", "ar": "Arabic",
    "de": "German", "fr": "French", "es": "Spanish", "pt": "Portuguese",
    "id": "Indonesian", "it": "Italian", "ko": "Korean", "ru": "Russian",
    "th": "Thai", "vi": "Vietnamese", "ja": "Japanese", "tr": "Turkish",
    "hi": "Hindi", "ms": "Malay", "nl": "Dutch", "sv": "Swedish",
    "da": "Danish", "fi": "Finnish", "pl": "Polish", "cs": "Czech",
    "fil": "Filipino", "fa": "Persian", "el": "Greek", "hu": "Hungarian",
    "mk": "Macedonian", "ro": "Romanian",
}
PARAKEET_LANGUAGES = {
    "bg", "hr", "cs", "da", "nl", "en", "et", "fi", "fr", "de", "el",
    "hu", "it", "lv", "lt", "mt", "pl", "pt", "ro", "sk", "sl", "es",
    "sv", "ru", "uk",
}
ALIGNER_LANGUAGES = {
    "Chinese", "English", "Cantonese", "French", "German", "Italian",
    "Japanese", "Korean", "Portuguese", "Russian", "Spanish",
}
SAMPLE_RATE = 16000
WINDOW_SECONDS = 30


class SpeechError(RuntimeError):
    """A useful diagnostic that is safe to return through the JSON protocol."""


def local_audio_path(value: Any) -> Path:
    if not isinstance(value, str) or not value.strip() or "://" in value or value.startswith("data:"):
        raise SpeechError("audio_path must be an existing local audio file; URLs are not accepted")
    path = Path(value).expanduser().resolve()
    if not path.is_file():
        raise SpeechError("The local audio file does not exist")
    return path


def normalize_language(value: Any, model: str) -> str | None:
    if value is not None and not isinstance(value, str):
        raise SpeechError("language must be a language code/name or null")
    if model == "cohere-transcribe" and (value is None or value in {"", "auto"}):
        raise SpeechError("Select a language for Cohere transcription; Qwen supports automatic detection")
    if value is None or value == "" or value == "auto":
        return None
    if not isinstance(value, str):
        raise SpeechError("language must be a language code/name or null")
    language = value.strip()
    code = language.lower().replace("_", "-").split("-")[0]
    if model in {"cohere-transcribe", "moss-transcribe-diarize"}:
        from .modern_asr import COHERE_LANGUAGES
        code = {name.casefold(): c for c, name in QWEN_LANGUAGES.items()}.get(language.casefold(), code)
        if model=="cohere-transcribe" and code not in COHERE_LANGUAGES:
            raise SpeechError(f"Cohere does not support language '{language}'")
        return code
    names = {name.casefold(): name for name in QWEN_LANGUAGES.values()}
    if model in QWEN_MODELS:
        result = QWEN_LANGUAGES.get(code) or names.get(language.casefold())
        if result is None:
            raise SpeechError(f"Qwen3-ASR does not support language '{language}'")
        return result
    reverse = {name.casefold(): code for code, name in QWEN_LANGUAGES.items()}
    code = reverse.get(language.casefold(), code)
    if code not in PARAKEET_LANGUAGES:
        raise SpeechError(f"Parakeet v3 does not support language '{language}'")
    # This checkpoint detects its supported languages, rather than taking a hint.
    return None


def cached_checkpoint(repo_id: str, env_name: str, *, optional: bool = False) -> Path | None:
    explicit = os.environ.get(env_name)
    if explicit:
        path = Path(explicit).expanduser().resolve()
        if not path.is_dir():
            raise SpeechError(f"{env_name} must point to a local model directory")
        from .model_security import validate_checkpoint
        validate_checkpoint(path)
        return path
    try:
        hub = importlib.import_module("huggingface_hub")
        path = Path(hub.snapshot_download(repo_id=repo_id, local_files_only=True)).resolve()
        from .model_security import validate_checkpoint
        validate_checkpoint(path)
        return path
    except (ImportError, OSError) as exc:
        if optional:
            return None
        raise SpeechError(f"Model '{repo_id}' is not cached. Download it separately and set {env_name}") from exc


def choose_device(torch: Any, requested: str) -> tuple[str, Any]:
    if not isinstance(requested, str):
        raise SpeechError("device must be auto, cpu, cuda, or cuda:<index>")
    if requested == "auto":
        requested = "cuda:0" if torch.cuda.is_available() else "cpu"
    if requested == "cpu":
        try:
            budget = max(1, min(8, int(os.environ.get("SONEFOLD_CPU_THREADS", "8"))))
        except ValueError:
            budget = 8
        torch.set_num_threads(min(budget, os.cpu_count() or 1))
        return requested, torch.float32
    if requested == "cuda":
        requested = "cuda:0"
    if not requested.startswith("cuda:") or not requested[5:].isdigit():
        raise SpeechError("SONEFOLD_SPEECH_DEVICE must be auto, cpu, cuda, or cuda:<index>")
    index = int(requested[5:])
    if not torch.cuda.is_available() or index >= torch.cuda.device_count():
        raise SpeechError(f"CUDA device '{requested}' is unavailable; install a compatible PyTorch build or use cpu")
    with torch.cuda.device(index):
        dtype = torch.bfloat16 if torch.cuda.is_bf16_supported() else torch.float16
    return requested, dtype


def _number(value: Any, name: str) -> float:
    if isinstance(value, bool) or not isinstance(value, (int, float)) or not math.isfinite(value):
        raise SpeechError(f"Invalid {name} returned by speech model")
    return float(value)


def parse_words(items: Any, offset: float, duration: float, *,
                coalesce_zero_duration: bool = False) -> list[dict[str, Any]]:
    """Validate spans, optionally retaining collapsed aligner tokens as phrases.

    Qwen's frame-quantized aligner can give a short token identical start/end
    times. Join its text to an adjacent genuine span, without interpolating a
    timestamp. Other engines keep strict validation. No usable span still fails.
    """
    words = []
    pending = []
    previous_start = 0.0
    for item in items:
        get = item.get if isinstance(item, dict) else lambda key, default=None: getattr(item, key, default)
        text = str(get("text", get("word", ""))).strip()
        start = _number(get("start_time", get("start")), "word start")
        end = _number(get("end_time", get("end")), "word end")
        collapsed = coalesce_zero_duration and start == end
        if (start < 0 or end < start or (end == start and not collapsed)
                or end > duration + 0.1 or start > duration
                or (start == duration and not collapsed) or start < previous_start):
            raise SpeechError("Speech model returned out-of-order or out-of-range word timestamps")
        previous_start = start
        if text:
            if collapsed:
                pending.append(text)
                continue
            absolute_start, absolute_end = round(offset + start, 3), round(offset + min(end, duration), 3)
            if absolute_end <= absolute_start:
                raise SpeechError("Speech model returned a word span below the timestamp resolution")
            word = {"text": " ".join([*pending, text]), "start": absolute_start,
                    "end": absolute_end, "speaker": None}
            if pending:
                word["alignment_coalesced"] = True
                pending.clear()
            words.append(word)
    if pending:
        if not words:
            raise SpeechError("Word alignment returned no positive-duration speech spans")
        words[-1]["text"] += " " + " ".join(pending)
        words[-1]["alignment_coalesced"] = True
    return words


def word_segments(words: list[dict[str, Any]], language: str | None) -> list[dict[str, Any]]:
    segments = []
    group: list[dict[str, Any]] = []
    for word in words:
        if group and (word["start"] - group[-1]["end"] > 0.8 or
                      word["end"] - group[0]["start"] > 6 or len(group) >= 16):
            segments.append(_word_segment(group, language))
            group = []
        group.append(word)
        if word["text"].endswith((".", "!", "?", "。", "！", "？")):
            segments.append(_word_segment(group, language))
            group = []
    if group:
        segments.append(_word_segment(group, language))
    return segments


def join_word_text(words: list[dict[str, Any]], language: str | None = None) -> str:
    separator = "" if language in {"Chinese", "Cantonese", "Japanese"} else " "
    return "".join(word["text"] + (word.get("separator_after", separator) if index < len(words) - 1 else "")
                   for index, word in enumerate(words))


ALIGNMENT_WARNING = ("Some lines have approximate times: word timing could not be matched for part of the audio. "
                     "The text itself is unaffected.")
REPETITION_RETRY_WARNING = "Repetitive recognition was retried using shorter audio sections."
REPETITION_WARNING = "Repetitive recognition remains in a short audio section; review that passage against the recording."


def has_repetition_loop(text: str) -> bool:
    """Detect sustained phrase loops for re-decoding, never for deleting text."""
    tokens = re.findall(r"\w+", text.casefold())
    # Whole sentences can loop too. Compare each token with the token one
    # period earlier, avoiding repeated slice allocations for longer phrases.
    # Eight consecutive repetitions still trigger an actual audio retry.
    for size in range(4, min(65, len(tokens) // 8 + 1)):
        matched = 0
        for index in range(size, len(tokens)):
            matched = matched + 1 if tokens[index] == tokens[index - size] else 0
            if matched >= size * 7:
                return True
    return False


def _canonical(value: str) -> tuple[str, list[int]]:
    """Letters and digits only, case- and accent-folded, with each one's index in ``value``."""
    characters, positions = [], []
    for position, char in enumerate(value):
        for part in unicodedata.normalize("NFKD", char.casefold()):
            if unicodedata.category(part).startswith(("L", "N")) or part == "'":
                characters.append(part)
                positions.append(position)
    return "".join(characters), positions


def restore_aligned_text(words: list[dict[str, Any]], text: str, min_coverage: float = 0.9) -> list[dict[str, Any]]:
    """Preserve ASR punctuation/spacing stripped by the official Qwen aligner.

    The aligner may spell accents differently or drop a short word; such near matches keep
    their word timing as long as ``min_coverage`` of the letters line up.
    """
    normalized, positions = _canonical(text)
    kept = [(word, _canonical(str(word.get("text", "")))[0]) for word in words]
    kept = [(word, token) for word, token in kept if token]
    if not kept or not normalized:
        raise SpeechError("Word alignment did not cover the complete transcription")
    joined = "".join(token for _, token in kept)
    token_starts, cursor = [], 0
    for _, token in kept:
        token_starts.append(cursor)
        cursor += len(token)
    if joined == normalized:
        offsets = token_starts
    else:
        matcher = difflib.SequenceMatcher(None, joined, normalized, autojunk=False)
        blocks = matcher.get_matching_blocks()
        if sum(block.size for block in blocks) / max(len(normalized), len(joined)) < min_coverage:
            raise SpeechError("Word alignment did not cover the complete transcription")

        def mapped(index: int) -> int:
            for block in blocks:
                if block.a <= index < block.a + block.size:
                    return block.b + index - block.a
                if index < block.a:
                    return block.b
            return len(normalized) - 1

        offsets = [mapped(start) for start in token_starts]
    restored: list[dict[str, Any]] = []
    previous_start = -1
    for index, (word, _) in enumerate(kept):
        start = 0 if index == 0 else positions[min(offsets[index], len(positions) - 1)]
        if start <= previous_start:
            # Two aligned words landed on the same text: keep one word spanning both times.
            restored[-1] = {**restored[-1], "end": word.get("end", restored[-1].get("end"))}
            continue
        restored.append({**word, "_start": start})
        previous_start = start
    for index, word in enumerate(restored):
        start = word.pop("_start")
        end = restored[index + 1]["_start"] if index < len(restored) - 1 else len(text)
        display = text[start:end]
        trimmed = display.rstrip()
        restored[index] = {**word, "text": trimmed, "separator_after": display[len(trimmed):]}
    return restored


def _word_segment(words: list[dict[str, Any]], language: str | None) -> dict[str, Any]:
    return {"text": join_word_text(words, language),
            "start": words[0]["start"], "end": words[-1]["end"], "speaker": None,
            "words": words, "timing": "phrase" if any(w.get("alignment_coalesced") for w in words) else "word",
            "language": language}


@contextmanager
def decodable_audio(path: Path, sf: Any) -> Iterator[Path]:
    """Use native libsndfile when supported; convert M4A/MP4 locally otherwise."""
    try:
        sf.info(str(path))
    except (RuntimeError, OSError):
        ffmpeg = os.environ.get("SONEFOLD_FFMPEG_PATH") or shutil.which("ffmpeg")
        if not ffmpeg or not Path(ffmpeg).is_file():
            raise SpeechError("Install FFmpeg or set SONEFOLD_FFMPEG_PATH to decode this recording format")
        with tempfile.TemporaryDirectory(prefix="sonefold-speech-") as folder:
            normalized = Path(folder) / "audio.wav"
            try:
                subprocess.run(
                    [ffmpeg, "-nostdin", "-hide_banner", "-loglevel", "error", "-i", str(path),
                     "-map", "0:a:0", "-vn", "-ac", "1", "-ar", str(SAMPLE_RATE),
                     "-c:a", "pcm_s16le", str(normalized)],
                    check=True, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                    stderr=subprocess.PIPE, timeout=7200,
                )
            except (OSError, subprocess.SubprocessError) as exc:
                raise SpeechError("FFmpeg could not decode the local recording") from exc
            yield normalized
    else:
        yield path


def audio_windows(path: Path) -> Iterator[tuple[Any, float, float]]:
    """Decode one window at a time; align absolute time to source sample counts."""
    try:
        np = importlib.import_module("numpy")
        sf = importlib.import_module("soundfile")
        with decodable_audio(path, sf) as decoded, sf.SoundFile(str(decoded)) as audio:
            rate = audio.samplerate
            if rate < 1000 or rate > 384000 or audio.channels > 8 or len(audio) <= 0:
                raise SpeechError("Unsupported sample rate/channels or empty audio recording")
            while audio.tell() < len(audio):
                start_frame = audio.tell()
                samples = audio.read(rate * WINDOW_SECONDS, dtype="float32", always_2d=True).mean(axis=1)
                if not np.isfinite(samples).all():
                    raise SpeechError("Audio contains invalid samples")
                # Prefer a quiet boundary near the end of a full window, to avoid
                # cutting words while bounding activation memory.
                if len(samples) == rate * WINDOW_SECONDS and audio.tell() < len(audio):
                    frame = max(1, rate // 50)
                    tail_start = rate * (WINDOW_SECONDS - 3)
                    tail = samples[tail_start:]
                    usable = len(tail) // frame * frame
                    rms = np.sqrt(np.mean(tail[:usable].reshape(-1, frame) ** 2, axis=1))
                    quiet = np.flatnonzero(rms < 0.005)
                    if len(quiet):
                        split = tail_start + int(quiet[-1]) * frame
                        samples = samples[:split]
                        audio.seek(start_frame + split)
                duration = len(samples) / rate
                if rate != SAMPLE_RATE:
                    scipy = importlib.import_module("scipy.signal")
                    divisor = math.gcd(rate, SAMPLE_RATE)
                    samples = scipy.resample_poly(samples, SAMPLE_RATE // divisor, rate // divisor).astype("float32")
                yield samples, start_frame / rate, duration
    except ImportError as exc:
        raise SpeechError("Install speech-worker/requirements.txt to decode local audio") from exc
    except (OSError, RuntimeError) as exc:
        if isinstance(exc, SpeechError):
            raise
        raise SpeechError("The local audio file could not be decoded") from exc


def split_speaker_window(samples, offset: float, duration: float, boundaries: list[float]):
    """Partition real samples at speaker changes, without dropping any audio.

    This is a fallback for languages without word alignment, not invented word
    timing. Sub-half-second fragments stay with their surrounding audio.
    """
    frame_count = len(samples)
    cuts = [0]
    minimum = max(1, round(frame_count * min(0.5, duration) / duration))
    first = bisect_right(boundaries, offset)
    last = bisect_right(boundaries, offset + duration)
    for boundary in boundaries[first:last]:
        frame = round(frame_count * (boundary - offset) / duration)
        if frame - cuts[-1] >= minimum and frame_count - frame >= minimum:
            cuts.append(frame)
    if len(cuts) == 1:
        return [(samples, offset, duration)]
    cuts.append(frame_count)
    return [(samples[start:end], offset + duration * start / frame_count,
             duration * (end - start) / frame_count)
            for start, end in zip(cuts, cuts[1:])]


class ASREngine:
    """Reuse one ASR model; optional aligner loads only when its language fits."""

    def __init__(self, device: str | None = None):
        self.requested_device = device or os.environ.get("SONEFOLD_SPEECH_DEVICE", "auto")
        self.device = "cpu"
        self.dtype: Any = None
        self._torch: Any = None
        self._model: Any = None
        self._model_name: str | None = None
        self._aligner: Any = None
        self._aligner_checked = False
        self._omni_aligner: Any = None
        self._omni_checked = False
        self._stream_session: Any = None
        self._stream_model_name: str | None = None

    def _runtime(self) -> Any:
        if self._torch is None:
            try:
                self._torch = importlib.import_module("torch")
            except ImportError as exc:
                raise SpeechError("Install a CPU/CUDA-compatible PyTorch build in the speech worker environment") from exc
            self.device, self.dtype = choose_device(self._torch, self.requested_device)
        return self._torch

    def health(self, model: str = "qwen3-asr-1.7b") -> dict[str, Any]:
        """Report installation/cache readiness without importing ML frameworks."""
        if model not in SUPPORTED_MODELS:
            raise SpeechError(f"Unsupported ASR model '{model}'")
        dependencies = {}
        for name in ("torch", "numpy", "soundfile", "scipy", "huggingface_hub", "qwen_asr", "nemo", "transformers"):
            try:
                dependencies[name] = importlib.util.find_spec("sonefold_speech.vendor.qwen_asr" if name == "qwen_asr" else name) is not None
            except (ImportError, ValueError):
                dependencies[name] = False
        cached = False
        diagnostic = None
        if model in QWEN_MODELS or model in {"cohere-transcribe", "moss-transcribe-diarize"}:
            from .modern_asr import MODERN_MODELS
            repo, env_name = QWEN_MODELS[model] if model in QWEN_MODELS else MODERN_MODELS[model]
            try:
                checkpoint = cached_checkpoint(repo, env_name, optional=True)
                cached = bool(checkpoint and (checkpoint / "config.json").is_file() and
                              (any(checkpoint.glob("*.safetensors"))))
            except SpeechError as exc:
                diagnostic = str(exc)
            required = ("torch", "numpy", "soundfile", "scipy", "transformers", "qwen_asr") if model in QWEN_MODELS else ("torch", "numpy", "soundfile", "scipy", "transformers")
        else:
            checkpoint = os.environ.get("SONEFOLD_PARAKEET_V3_MODEL")
            cached = bool(checkpoint and Path(checkpoint).expanduser().is_file() and checkpoint.endswith(".nemo"))
            required = ("torch", "numpy", "soundfile", "scipy", "nemo")
        return {"version": "0.1.0", "offline": True, "model": model,
                "device": self.requested_device, "dependencies": dependencies,
                "model_cached": cached, "model_needs_download": not cached,
                "ready": cached and all(dependencies[name] for name in required),
                "loaded_model": self._model_name, "diagnostic": diagnostic}

    def stream_health(self, model: str = "nemotron-3.5") -> dict[str, Any]:
        """Inspect local files/modules without importing Torch or NeMo models."""
        if model not in {"multitalker-parakeet", "nemotron-3.5"}:
            raise SpeechError("Streaming ASR must be multitalker-parakeet or nemotron-3.5")
        dependencies, helpers = {}, False
        for name in ("torch", "numpy", "nemo", "omegaconf"):
            try:
                spec = importlib.util.find_spec(name)
                dependencies[name] = spec is not None
                if name == "nemo" and spec:
                    for directory in spec.submodule_search_locations or []:
                        path = Path(directory) / "collections/asr/parts/utils/multispk_transcribe_utils.py"
                        if path.is_file():
                            source = path.read_text(encoding="utf-8")
                            helpers = all(f"{kind} {symbol}" in source for kind, symbol in
                                          (("class", "SpeakerTaggedASR"), ("def", "configure_diar_streaming"),
                                           ("def", "validate_feature_frame_strides")))
                            if helpers:
                                break
            except (ImportError, ValueError, OSError):
                dependencies[name] = False

        def cached_file(variable: str) -> bool:
            value = os.environ.get(variable)
            if not value:
                return False
            path = Path(value).expanduser()
            if path.is_dir():
                files = list(path.glob("*.nemo"))
                return len(files) == 1 and files[0].is_file()
            return path.is_file() and path.suffix == ".nemo"

        asr_cached = cached_file("SONEFOLD_STREAMING_ASR_MODEL_PATH")
        diar_cached = cached_file("SONEFOLD_DIARIZATION_MODEL_DIR")
        return {"version": "0.1.0", "offline": True, "model": model, "device": self.requested_device,
                "dependencies": dependencies, "streaming_helpers": helpers,
                "model_cached": asr_cached, "diarization_model_cached": diar_cached,
                "model_needs_download": not (asr_cached and diar_cached),
                "ready": asr_cached and diar_cached and helpers and all(dependencies.values()),
                "loaded_model": self._stream_model_name if self._stream_session is not None else None,
                "diagnostic": "Checks dependencies and local checkpoints; model loading/device compatibility is not validated"}

    def load(self, model: str, device: Any = None) -> dict[str, Any]:
        self.configure_device(device)
        loaded = self._load(model)
        if model in QWEN_MODELS and getattr(self, "_warmed_model", None) != model:
            self._warm_up(loaded)
            self._warmed_model = model
        return {"model": model, "device": self.device, "offline": True}

    def _warm_up(self, loaded: Any) -> None:
        """One silent second through the model now, so GPU start-up does not delay the first real line."""
        try:
            import numpy as np
            loaded.transcribe(audio=(np.zeros(SAMPLE_RATE, dtype="float32"), SAMPLE_RATE), language=None,
                              return_time_stamps=False)
        except Exception:  # noqa: BLE001 - warming is an optimisation; real requests report real errors
            pass

    def configure_device(self, device: Any = None) -> None:
        """Validate per-request device changes before keeping a cached runtime."""
        if device is not None:
            if not isinstance(device, str) or not (device in {"auto", "cpu", "cuda"} or
                                                   (device.startswith("cuda:") and device[5:].isdigit())):
                raise SpeechError("device must be auto, cpu, cuda, or cuda:<index>")
            if device != self.requested_device:
                self.unload()
                self._torch = None
                self.requested_device = device

    def unload(self) -> None:
        self._warmed_model = None
        self._model = self._aligner = None
        self._model_name = None
        self._aligner_checked = False
        self._omni_aligner = None
        self._omni_checked = False
        gc.collect()
        if self._torch is not None and self.device.startswith("cuda"):
            self._torch.cuda.empty_cache()

    def _load(self, name: str) -> Any:
        if name == self._model_name:
            return self._model
        if name not in SUPPORTED_MODELS:
            raise SpeechError(f"Unsupported ASR model '{name}'")
        self.unload()
        self._runtime()
        if name in {"cohere-transcribe", "moss-transcribe-diarize"}:
            from .modern_asr import load_model
            self._model=load_model(name,self.device,self.dtype)
        elif name in QWEN_MODELS:
            repo, env_name = QWEN_MODELS[name]
            checkpoint = cached_checkpoint(repo, env_name)
            try:
                qwen = importlib.import_module("sonefold_speech.vendor.qwen_asr")
            except ImportError as exc:
                raise SpeechError("Install speech-worker/requirements-qwen.txt for Qwen3-ASR") from exc
            self._model = qwen.Qwen3ASRModel.from_pretrained(
                str(checkpoint), dtype=self.dtype, device_map=self.device,
                local_files_only=True, max_inference_batch_size=1, max_new_tokens=1024,
                trust_remote_code=False, use_safetensors=True,
            )
        else:
            configured = os.environ.get("SONEFOLD_PARAKEET_V3_MODEL")
            if not configured:
                raise SpeechError("Set SONEFOLD_PARAKEET_V3_MODEL to an already downloaded Parakeet v3 .nemo checkpoint")
            checkpoint = Path(configured).expanduser().resolve()
            if not checkpoint.is_file() or checkpoint.suffix != ".nemo":
                raise SpeechError("SONEFOLD_PARAKEET_V3_MODEL must point to a local .nemo file")
            from .nemo_runtime import prepare
            prepare()
            from .model_security import validate_nemo_checkpoint
            validate_nemo_checkpoint(checkpoint)
            try:
                nemo = importlib.import_module("nemo.collections.asr")
            except ImportError as exc:
                raise SpeechError("Install speech-worker/requirements-nemo.txt for the NeMo Parakeet engine") from exc
            self._model = nemo.models.ASRModel.restore_from(str(checkpoint), map_location=self.device)
            self._model.to(self.device)
            self._model.eval()
        self._model_name = name
        return self._model

    def _load_aligner(self) -> Any:
        if not self._aligner_checked:
            checkpoint = cached_checkpoint("Qwen/Qwen3-ForcedAligner-0.6B", "SONEFOLD_QWEN3_ALIGNER_DIR", optional=True)
            if checkpoint:
                qwen = importlib.import_module("sonefold_speech.vendor.qwen_asr")
                self._aligner = qwen.Qwen3ForcedAligner.from_pretrained(
                    str(checkpoint), dtype=self.dtype, device_map=self.device, local_files_only=True,
                    trust_remote_code=False, use_safetensors=True,
                )
            self._aligner_checked = True
        return self._aligner

    def _multilingual_words(self, samples: Any, text: str, offset: float,
                            duration: float, language: str | None) -> list[dict[str, Any]]:
        """Try the explicitly installed CPU fallback; never fetch optional weights."""
        configured = os.environ.get("SONEFOLD_OMNI_ALIGNER_DIR")
        if not configured:
            return []
        if not self._omni_checked:
            self._omni_checked = True
            try:
                from .omni_alignment import OmniAlignment
                self._omni_aligner = OmniAlignment(Path(configured).expanduser().resolve())
            except (ImportError, OSError, ValueError, RuntimeError):
                # One failed setup attempt per model lifetime, not per passage.
                self._omni_aligner = None
        if self._omni_aligner is None:
            return []
        try:
            return self._omni_aligner.align(samples, text, offset, duration, language)
        except (ValueError, RuntimeError):
            # Unsupported symbols/failed paths retain the normal approximate
            # timing warning and can still use real speaker-boundary retries.
            return []

    def transcribe(self, audio_path: Any, model: str = "qwen3-asr-1.7b", language: Any = None,
                   *, word_timestamps: bool = True, context: str = "",
                   speaker_turns: list[dict] | None = None) -> dict[str, Any]:
        if not isinstance(context, str) or len(context) > 4000:
            raise SpeechError("Vocabulary context must be at most 4000 characters")
        path = local_audio_path(audio_path)
        if model not in SUPPORTED_MODELS:
            raise SpeechError(f"Unsupported ASR model '{model}'")
        if model=="moss-transcribe-diarize":
            from .modern_asr import transcribe_joint
            return transcribe_joint(self,path)
        forced_language = normalize_language(language, model)
        from .diarization import speaker_change_boundaries
        boundaries = speaker_change_boundaries(speaker_turns or [])
        prefer_speaker_windows = bool(boundaries and forced_language and forced_language not in ALIGNER_LANGUAGES
                                      and model in QWEN_MODELS and word_timestamps
                                      and not os.environ.get("SONEFOLD_OMNI_ALIGNER_DIR"))
        segments: list[dict[str, Any]] = []
        warnings: set[str] = set()
        languages: list[str] = []
        texts: list[str] = []
        audio_seconds = 0.0
        # Decode validation precedes heavy model loading.
        windows = iter(audio_windows(path))
        retry_windows = deque()
        while True:
            window = retry_windows.popleft() if retry_windows else next(windows, None)
            if window is None:
                break
            samples, offset, duration = window[:3]
            guided = window[3] if len(window) > 3 else False
            audio_seconds = max(audio_seconds, offset + duration)
            if not samples.any():
                continue
            if prefer_speaker_windows and not guided:
                partitions = split_speaker_window(samples, offset, duration, boundaries)
                if len(partitions) > 1:
                    retry_windows.extendleft((*part, True) for part in reversed(partitions))
                    continue
            loaded = self._load(model)
            words = []
            if model in {"cohere-transcribe", "moss-transcribe-diarize"}:
                from .modern_asr import transcribe_window
                text,spans,notes=transcribe_window(loaded,samples,forced_language,model,offset,duration,self._torch)
                segments.extend(spans);warnings.update(notes)
                if text:texts.append(text)
                if forced_language and forced_language not in languages:languages.append(forced_language)
                continue
            if model in QWEN_MODELS:
                hints = {"context": context} if context else {}
                result = loaded.transcribe(audio=(samples, SAMPLE_RATE), language=forced_language,
                                           return_time_stamps=False, **hints)[0]
                text = result.text.strip()
                if has_repetition_loop(text):
                    if duration > 5:
                        # Re-read actual audio, rather than sanitizing a hallucination
                        # into apparently valid speech. Exact sample cuts retain time.
                        middle = len(samples) // 2
                        split = duration * middle / len(samples)
                        retry_windows.appendleft((samples[middle:], offset + split, duration - split))
                        retry_windows.appendleft((samples[:middle], offset, split))
                        warnings.add(REPETITION_RETRY_WARNING)
                        continue
                    warnings.add(REPETITION_WARNING)
                detected = result.language or forced_language
                if detected and detected not in languages:
                    languages.append(detected)
                if word_timestamps and text and detected in ALIGNER_LANGUAGES:
                    aligner = self._load_aligner()
                    if aligner:
                        try:
                            items = aligner.align(audio=(samples, SAMPLE_RATE), text=text, language=detected)[0]
                            words = restore_aligned_text(parse_words(items, offset, duration,
                                                                     coalesce_zero_duration=True), text)
                        except (SpeechError, RuntimeError, ValueError):
                            words = []
                if word_timestamps and text and not words:
                    words = self._multilingual_words(samples, text, offset, duration, detected)
                if word_timestamps and text and not words:
                    warnings.add(ALIGNMENT_WARNING)
                    if boundaries and not guided:
                        # Re-read the audio instead of dividing an unaligned
                        # string proportionally or assigning one speaker to it.
                        prefer_speaker_windows = True
                        partitions = split_speaker_window(samples, offset, duration, boundaries)
                        if len(partitions) > 1:
                            retry_windows.extendleft((*part, True) for part in reversed(partitions))
                            continue
            else:
                result = loaded.transcribe([samples], batch_size=1, timestamps=True)[0]
                text = result.text.strip()
                stamps = getattr(result, "timestamp", {}) or {}
                try:
                    words = parse_words(stamps.get("word", []), offset, duration)
                except SpeechError:
                    warnings.add(ALIGNMENT_WARNING)
                if word_timestamps and text and not words:
                    warnings.add(ALIGNMENT_WARNING)
            if not text:
                continue
            texts.append(text)
            if words:
                segments.extend(word_segments(words, detected if model in QWEN_MODELS else None))
            else:
                segments.append({"text": text, "start": round(offset, 3),
                                 "end": round(offset + duration, 3), "speaker": None,
                                 "timing": "turn" if guided else "chunk"})
        speaker_ids=sorted({s["speaker"] for s in segments if s.get("speaker")})
        return {"segments": segments, "speakers": [{"id":s,"name":s} for s in speaker_ids], "model": model,
                "text": "\n".join(texts), "audio_seconds": audio_seconds,
                "language": ",".join(languages) or None, "warnings": sorted(warnings)}
