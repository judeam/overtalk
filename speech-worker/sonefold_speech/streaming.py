"""Persistent local NeMo multitalker ASR + Nemotron-3 diarization sessions.

Follows NVIDIA's ASR_INTEGRATION_GUIDE and SpeakerTaggedASR API. Run in a
separate NeMo Speech environment when the batch ASR environment differs.
"""

from __future__ import annotations

from contextlib import nullcontext
import math
import os
from pathlib import Path
from threading import Lock


class PCMFrameBuffer:
    """Keep left audio context across calls and bound ingress memory."""
    def __init__(self, hop_samples: int, cache_samples: int, max_chunk_samples: int = 160000):
        if hop_samples < 1 or cache_samples < 0:
            raise ValueError("Invalid streaming frame geometry")
        self.hop_samples = hop_samples
        self.cache_samples = cache_samples
        self.frame_samples = hop_samples + cache_samples
        self.max_chunk_samples = max_chunk_samples
        self.pending = [0.0] * cache_samples
        self.total_samples = 0
        self.finished = False

    def append(self, samples, sample_rate: int = 16000):
        if self.finished:
            raise ValueError("This streaming session has finished")
        if sample_rate != 16000:
            raise ValueError("Streaming input must be mono PCM at 16000 Hz")
        if not hasattr(samples, "__len__") or len(samples) > self.max_chunk_samples:
            raise ValueError("Audio chunk exceeds the streaming input limit")
        normalized = []
        for sample in samples:
            if isinstance(sample, bool):
                raise ValueError("PCM samples must be finite numbers between -1 and 1")
            value = float(sample)
            if not math.isfinite(value) or not -1.0 <= value <= 1.0:
                raise ValueError("PCM samples must be finite numbers between -1 and 1")
            normalized.append(value)
        # Validate the entire block before mutating session state.
        self.pending.extend(normalized)
        self.total_samples += len(normalized)

    def frames(self):
        while len(self.pending) >= self.frame_samples:
            frame = self.pending[:self.frame_samples]
            self.pending = self.pending[self.hop_samples:]
            yield frame

    def finish(self):
        if self.finished:
            return None
        self.finished = True
        frame = list(self.pending) if self.total_samples else None
        self.pending.clear()
        return frame


def segments_from_seglst(records, *, duration: float | None = None, partial: bool = True):
    """Copy structured NeMo output; never parse ANSI console transcripts."""
    segments = []
    occurrences = {}
    for record in records:
        text = str(record.get("words", "")).strip()
        if not text:
            continue
        start, end = float(record["start_time"]), float(record["end_time"])
        if not math.isfinite(start) or not math.isfinite(end) or start < 0 or end <= start:
            raise RuntimeError("NeMo returned invalid streaming timestamps")
        if duration is not None:
            end = min(end, duration)
            if start >= end:
                continue  # Discard emissions wholly outside real captured audio.
        raw_speaker = record.get("speaker")
        try:
            number = int(str(raw_speaker).rsplit("_", 1)[-1])
        except (ValueError, TypeError):
            raise RuntimeError("NeMo returned an unsupported streaming speaker ID") from None
        if not 0 <= number < 8:
            raise RuntimeError("NeMo returned an out-of-range streaming speaker ID")
        speaker = f"SPEAKER_{number:02d}"
        key = f"{speaker}:{start:.3f}"
        occurrence = occurrences.get(key, 0)
        occurrences[key] = occurrence + 1
        segment_id = key if occurrence == 0 else f"{key}:{occurrence}"
        segments.append({"id": segment_id, "text": text, "start": start,
                         "end": end, "speaker": speaker, "speaker_confidence": None,
                         "overlap": False, "is_partial": partial})
    segments.sort(key=lambda segment: (segment["start"], segment["speaker"]))
    for index, segment in enumerate(segments):
        for other in segments[index + 1:]:
            if other["start"] >= segment["end"]:
                break
            if segment["speaker"] != other["speaker"]:
                segment["overlap"] = other["overlap"] = True
    return segments


def _checkpoint(value, description):
    if not value:
        raise RuntimeError(f"Configure a predownloaded local {description} .nemo checkpoint")
    path = Path(value).expanduser()
    if path.is_dir():
        candidates = sorted(path.glob("*.nemo"))
        if len(candidates) != 1:
            raise RuntimeError(f"The {description} directory must contain exactly one .nemo checkpoint")
        path = candidates[0]
    if not path.is_file() or path.suffix != ".nemo":
        raise RuntimeError(f"Configure a predownloaded local {description} .nemo checkpoint")
    return path


def mask_log_mel_features(chunk_audio, mask, threshold: float = .5, mask_value: float = -16.6355):
    """Replace inactive log-mel frames with silence, preserving active zeros.

    NeMo's pinned integration helper multiplies by zero but checks the original
    features when applying the silence floor. Zero is not silence in log-mel
    space. Keep its eight-to-one frame mapping and left-cache padding policy.
    """
    import torch
    if chunk_audio.ndim != 3 or mask.ndim != 2 or chunk_audio.shape[0] != mask.shape[0]:
        raise ValueError("Speaker masks need matching (B,C,T) features and (B,T) targets")
    active = (mask > threshold).repeat_interleave(8, dim=1)
    width = chunk_audio.shape[-1]
    if active.shape[1] > width:
        active = active[:, :width]
    elif active.shape[1] < width:
        active = torch.nn.functional.pad(active, (width - active.shape[1], 0), value=False)
    return chunk_audio.masked_fill(~active.unsqueeze(1), mask_value)


def _prewarm_streamer(adapter, recreate):
    """Initialize lazy CUDA decoders before capture, then discard all state."""
    try:
        # Initialize the maximum decoder batch, including silent speaker slots.
        # These artificial samples never enter a recording or its PCM clock.
        adapter.streamer._cache_gating = False
        silence = [0.0] * (adapter.hop_samples + adapter.cache_samples)
        adapter.process(silence, final=False)
        adapter.process(silence, final=True)
    finally:
        adapter.streamer = recreate()
        adapter.step = 0


class _NeMoAdapter:
    def __init__(self, asr_path, diar_path, device, max_speakers, asr_model, language):
        from .nemo_runtime import prepare
        prepare()
        try:
            import numpy as np
            import torch
            from omegaconf import OmegaConf
            import nemo.collections.asr as nemo_asr
            from nemo.collections.asr.models.sortformer_diar_models import SortformerEncLabelModel
            from nemo.collections.asr.parts.utils.multispk_transcribe_utils import (
                SpeakerTaggedASR, configure_diar_streaming, validate_feature_frame_strides,
            )
            from nemo.collections.asr.parts.utils.streaming_utils import CacheAwareStreamingAudioBuffer
        except ImportError as error:
            raise RuntimeError("Streaming speaker recognition needs an isolated, recent NeMo Speech ASR environment") from error
        if asr_model not in {"multitalker-parakeet", "nemotron-3.5"}:
            raise ValueError("Streaming ASR must be multitalker-parakeet or nemotron-3.5")
        if asr_model == "multitalker-parakeet" and language not in {None, "auto", "en", "en-US"}:
            raise ValueError("Multitalker Parakeet supports English; select Nemotron 3.5 for other locales")
        from .model_security import validate_nemo_checkpoint
        validate_nemo_checkpoint(Path(asr_path))
        validate_nemo_checkpoint(Path(diar_path))
        self.np, self.torch = np, torch
        self.device = device
        self.asr = nemo_asr.models.ASRModel.restore_from(restore_path=str(asr_path), map_location=device)
        self.diar = SortformerEncLabelModel.restore_from(restore_path=str(diar_path), map_location=device)
        self.asr.eval().to(device)
        self.diar.eval().to(device)
        masked = asr_model == "nemotron-3.5"
        if not masked and not hasattr(self.asr, "set_speaker_targets"):
            raise ValueError("The streaming checkpoint must support multitalker speaker-target injection")
        context = [56, 13] if masked else [70, 13]
        self.asr.encoder.set_default_att_context_size(context)
        if masked:
            if not hasattr(self.asr, "set_inference_prompt"):
                raise ValueError("Nemotron 3.5 checkpoint must support language prompts")
            self.asr.set_inference_prompt(language or "auto")
            if hasattr(self.asr.decoding, "set_strip_lang_tags"):
                self.asr.decoding.set_strip_lang_tags(True)
        validate_feature_frame_strides(asr_model=self.asr, diar_model=self.diar)
        cfg = OmegaConf.create({
            "device": device, "sample_rate": 16000, "deploy_mode": True, "streaming_mode": True,
            "max_num_of_spks": max_speakers, "batch_size": 1, "parallel_speaker_strategy": True,
            "masked_asr": masked, "mask_preencode": False, "single_speaker_mode": False,
            "cache_gating": True, "cache_gating_buffer_size": 2, "binary_diar_preds": True,
            "spkcache_len": None, "spkcache_update_period": 222, "fifo_len": 264,
            "diar_right_context": 0, "att_context_size": context, "use_amp": device.startswith("cuda"),
            "precision": "bf16", "online_normalization": False, "pad_and_drop_preencoded": False,
            "feat_len_sec": .01, "discarded_frames": 8, "word_window": 50, "sent_break_sec": 1.0,
            "fix_prev_words_count": 5, "update_prev_words_sentence": 5, "left_frame_shift": -1,
            "right_frame_shift": 0, "min_sigmoid_val": .01, "ignored_initial_frame_steps": 5,
            "generate_realtime_scripts": False, "print_sample_indices": [0], "colored_text": False,
            "verbose": False, "print_time": False, "log": False,
        })
        geometry = self.asr.encoder.streaming_cfg
        configure_diar_streaming(diar_model=self.diar, cfg=cfg,
                                 output_subsampling_factor=self.asr.encoder.subsampling_factor,
                                 diar_chunk_len=geometry.valid_out_len + geometry.cache_drop_size)
        cfg.spkcache_len = int(self.diar.sortformer_modules.spkcache_len)
        def new_streamer():
            streamer = SpeakerTaggedASR(cfg, self.asr, self.diar)
            if masked:
                streamer.mask_features = mask_log_mel_features
            return streamer
        self.streamer = new_streamer()
        self.audio = CacheAwareStreamingAudioBuffer(model=self.asr, online_normalization=False)
        feature_stride = float(self.asr.cfg.preprocessor.window_stride)
        self.hop_samples = round(geometry.valid_out_len * self.asr.encoder.subsampling_factor * feature_stride * 16000)
        cache = geometry.pre_encode_cache_size
        if isinstance(cache, (list, tuple)):
            cache = cache[-1]
        self.cache_samples = round(cache * feature_stride * 16000)
        self.step = 0
        if device.startswith("cuda"):
            _prewarm_streamer(self, new_streamer)

    def process(self, samples, *, final):
        pcm = self.np.asarray(samples, dtype=self.np.float32)
        signal, lengths = self.audio.preprocess_audio(pcm)
        signal = signal[:, :, :lengths[0]]
        drop = 0 if self.step == 0 else self.asr.encoder.streaming_cfg.drop_extra_pre_encoded
        amp = nullcontext()
        if self.device.startswith("cuda"):
            dtype = self.torch.bfloat16 if self.torch.cuda.is_bf16_supported() else self.torch.float16
            amp = self.torch.autocast(device_type="cuda", dtype=dtype)
        with self.torch.inference_mode(), amp:
            self.streamer.perform_parallel_streaming_stt_spk(step_num=self.step, chunk_audio=signal,
                chunk_lengths=lengths, is_buffer_empty=final, drop_extra_pre_encoded=drop)
        self.step += 1

    def records(self):
        states = self.streamer.instance_manager.batch_asr_states
        return [] if not states else list(states[0].seglsts)

    def close(self):
        self.streamer = self.audio = self.asr = self.diar = None
        from .diarization import unload_diarization
        unload_diarization()


class StreamingSession:
    """One recording's speaker cache, decoders, PCM buffer and timestamps.

    Return cumulative structured snapshots: consumers upsert IDs/revisions,
    rather than appending the cumulative text on every chunk.
    """
    def __init__(self, asr_path: str | None = None, diar_path: str | None = None, *, device: str = "cpu",
                 max_speakers: int = 8, asr_model: str = "multitalker-parakeet", language: str | None = None,
                 _adapter=None):
        if isinstance(max_speakers, bool) or not isinstance(max_speakers, int) or not 1 <= max_speakers <= 8:
            raise ValueError("Streaming supports one to eight speakers")
        if _adapter is None:
            asr = _checkpoint(asr_path or os.environ.get("SONEFOLD_STREAMING_ASR_MODEL_PATH"), "streaming ASR")
            diar = _checkpoint(diar_path or os.environ.get("SONEFOLD_DIARIZATION_MODEL_DIR"), "Nemotron-3 diarization")
            _adapter = _NeMoAdapter(asr, diar, device, max_speakers, asr_model, language)
        self.adapter = _adapter
        self.buffer = PCMFrameBuffer(_adapter.hop_samples, _adapter.cache_samples)
        self.lock = Lock()
        self.closed = False
        self.last_snapshot = {"segments": [], "speakers": [], "text": "", "is_partial": True}

    def _snapshot(self, partial):
        duration = None if partial else self.buffer.total_samples / 16000
        segments = segments_from_seglst(self.adapter.records(), duration=duration, partial=partial)
        ids = list(dict.fromkeys(segment["speaker"] for segment in segments))
        return {"segments": segments, "speakers": [{"id": speaker, "name": f"Speaker {int(speaker[-2:]) + 1}"} for speaker in ids],
                "text": "\n".join(f'{segment["speaker"]}: {segment["text"]}' for segment in segments),
                "is_partial": partial, "sample_rate": 16000}

    def accept_audio(self, samples, sample_rate: int = 16000):
        with self.lock:
            if self.closed:
                raise ValueError("Streaming session is closed")
            self.buffer.append(samples, sample_rate)
            for frame in self.buffer.frames():
                self.adapter.process(frame, final=False)
            self.last_snapshot = self._snapshot(True)
            return self.last_snapshot

    def finish(self):
        with self.lock:
            if self.closed:
                raise ValueError("Streaming session is closed")
            if self.buffer.finished:
                return self.last_snapshot
            final = self.buffer.finish()
            if final is not None:
                self.adapter.process(final, final=True)
            self.last_snapshot = self._snapshot(False)
            return self.last_snapshot

    def close(self):
        with self.lock:
            if not self.closed:
                self.adapter.close()
                self.buffer.pending.clear()
                self.closed = True
