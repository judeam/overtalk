# Overtalk local speech worker

Optional private reprocessing for the Overtalk service: Qwen3-ASR 1.7B / 0.6B, original NeMo Parakeet v3, and speaker diarization. The native Parakeet/Whisper engines remain available without Python. Research and model tradeoffs are in [the dated review](../docs/research/2026-09-30-stt.md).

Use Python 3.12 in an isolated environment. Install a [PyTorch build](https://pytorch.org/get-started/locally/) compatible with the chosen CPU/GPU first. Qwen3-ASR 1.7B plus its aligner passed a [CUDA compatibility smoke test](../docs/research/stt-evidence/real-gpu-smoke.json) on an NVIDIA RTX 5090 Laptop GPU with Torch 2.14.0+cu130 and driver 615.71.09. This validates Qwen inference, not representative meeting accuracy or throughput. A separately pinned NeMo environment also passed live CPU/CUDA compatibility smoke tests; see the diarization runtime evidence. Then install one ASR runtime. On Debian and Ubuntu, `python3.12 -m venv` needs the `python3.12-venv` package (`sudo apt install python3.12-venv`):

```sh
python3.12 -m venv ~/.venvs/overtalk-speech
~/.venvs/overtalk-speech/bin/pip install -r speech-worker/requirements-qwen.txt
# Or requirements-nemo.txt for the original .nemo Parakeet checkpoint.
```

The original Qwen checkpoints use Overtalk's Apache-2.0 adapter in
`sonefold_speech/vendor/qwen_asr`; do not install `qwen-asr==0.0.6`, whose dependency
pins restore vulnerable Transformers/Accelerate versions. `requirements-qwen.txt`
uses patched dependencies. No remote model code or kernel is allowed. Existing
safetensor model files are reused.

For NeMo, use a separate Python 3.12 environment selected by
`SONEFOLD_DIARIZATION_PYTHON`. The reviewed source is NVIDIA NeMo Speech commit
`00278b0bd95bb2b8174b88012aa21b003c59d2e9` (Apache-2.0). Its training dependency
caps predate the security fixes. After installing that source and its inference
extras, install `requirements-nemo.txt` without allowing NeMo to downgrade those
packages, then run `python speech-worker/patch_nemo_runtime.py` with that
interpreter. The patch disables training telemetry for Overtalk inference,
adapts the removed Neptune re-export, and bounds attention tiles for laptop GPUs.
It rejects changed source patterns. The runtime forces restricted checkpoint
loading. Keep the upstream cap conflicts documented; do not call the unmodified
training package's resolver a validated inference lockfile.
[The attack-surface audit](../docs/SECURITY-ATTACK-SURFACE.md) describes the
checkpoint-loading guards and ML dependency pin checks.


Download model weights **explicitly before analysis**. Review the upstream model license and revision, then use the model-only utility (it never reads recordings):

```sh
~/.venvs/overtalk-speech/bin/python speech-worker/tools/download_models.py \
  --model qwen3-asr-1.7b --revision REVIEWED_COMMIT_SHA --directory ~/overtalk-models/qwen-1.7b
```

Supported download names: `qwen3-asr-1.7b`, `qwen3-asr-0.6b`, `qwen3-aligner-0.6b`, `parakeet-tdt-0.6b-v3`, `nemotron-3-diarization`, `multitalker-parakeet`, `nemotron-3.5-streaming`. The utility records the resolved SHA in `sonefold-model.json`. Verified original Qwen revisions are `7278e1e70fe206f11671096ffdd38061171dd6e5` (1.7B) and `c7cbfc2048c462b0d63a45797104fc9db3ad62b7` (aligner), both dated 2026-01-30. Community-1 is gated: accept its terms and follow its [official offline setup](https://huggingface.co/pyannote/speaker-diarization-community-1) to obtain a complete local pipeline, including its component models. No downloads occur during worker startup, health checks, or analysis.

When Overtalk starts the worker, it sets these variables from **Settings → Transcription → Advanced speech** and finds FFmpeg itself. Values already present in Overtalk's environment for these names are also passed through. Export them yourself only when running the worker by hand:

| Variable | Meaning |
| --- | --- |
| `SONEFOLD_QWEN3_ASR_1_7B_DIR` / `SONEFOLD_QWEN3_ASR_0_6B_DIR` | Original Qwen checkpoint directory; otherwise use an already complete Hugging Face cache. |
| `SONEFOLD_QWEN3_ALIGNER_DIR` | Optional original `Qwen3-ForcedAligner-0.6B` directory. |
| `SONEFOLD_PARAKEET_V3_MODEL` | Local Parakeet v3 `.nemo` file, not Overtalk's ONNX folder. |
| `SONEFOLD_SPEECH_DEVICE` | `auto`, `cpu`, `cuda`, or `cuda:0`; CPU uses float32, CUDA BF16 where supported. |
| `SONEFOLD_FFMPEG_PATH` | FFmpeg executable; otherwise resolve from `PATH`. Needed for MP4/M4A formats unsupported by libsndfile. |
| `SONEFOLD_DIARIZATION_MODEL_DIR` | Nemotron `.nemo` file/directory or complete local Community-1 pipeline. |
| `SONEFOLD_DIARIZATION_PYTHON` | Optional separate Python executable for diarization. |
| `SONEFOLD_STREAMING_ASR_MODEL_PATH` | Local multitalker Parakeet or Nemotron 3.5 `.nemo` file for live sessions. |

The bundled Qwen adapter expects the original checkpoint names, **not** the later `*-hf` conversions. ASR supports more languages than the forced aligner. Missing/unsupported alignment produces approximate chunk intervals and a visible warning; word times are never invented. A collapsed Qwen word interval is coalesced with an adjacent timed phrase, retaining its text and the surrounding real timestamps instead of discarding the whole passage's alignment. Such groups use `timing:"phrase"`. Windows are at most 30 seconds, with a quiet boundary preferred in the last three seconds; this bounds activation memory but may affect long-context accuracy. Compressed audio normalization uses a local temporary WAV, which is removed after processing. Diarization counts require Community-1; Nemotron-3 estimates up to eight speakers. Speaker labels describe anonymous meeting participants, and ambiguous/overlapping speech can remain unknown. Names are supplied using the meeting's Speakers tab; diarization alone does not identify people by name.

Run from the repository root:

```sh
PYTHONPATH=speech-worker ~/.venvs/overtalk-speech/bin/python -m sonefold_speech
```

For CPU work select `SONEFOLD_SPEECH_DEVICE=cpu`. On Linux, when the service starts the worker on a GPU, it runs it under the optional launcher named by `OVERTALK_GPU_LAUNCHER` ([GPU notes](../docs/GPU_ACCELERATION.md#sharing-the-gpu-with-the-desktop)).

Send one JSON request per line. stdout contains only JSON responses; diagnostics go to stderr. Supported operations: `health` (batch dependency/cache status, no model loads), `stream_health` (streaming helpers/checkpoints, no model loads), `load` (prewarm one model), `analyze`, `stream_start`, `stream_audio`, `stream_stop`, `ping`, `shutdown`. Readiness checks establish installed dependencies and checkpoint presence; loading validates actual model/device compatibility. `offline:false` and remote audio URLs are rejected.

`analyze` defaults to word timestamps. Text-only callers can pass `"word_timestamps":false` to skip Qwen's separate forced aligner; speaker analysis requires timestamps. The native VAD provider already supplies segment intervals and uses this text-only path. Successful native batch jobs keep the worker warm for up to two idle minutes. Cancellation, failed jobs, settings changes, summary generation and quality retrieval release it immediately. Runtime health checks do not renew the idle timeout. Model loading has its own progress message; durations shown beside segments describe audio length.

Sustained Qwen phrase loops trigger re-decoding of shorter sections of the original audio, down to five seconds. This does not delete repetitions from text or invent replacement speech. If repetition remains at that limit, the result includes a review warning. The retry is bounded and uses the same local model.

After configuring the local checkpoint environment variables above, measure startup and warm inference without saving transcripts:

```sh
~/.venvs/overtalk-speech/bin/python speech-worker/tools/profile_latency.py \
  --audio /absolute/test.wav --device cuda --no-word-timestamps
```

See [the measured RTX 5090 Laptop cold-start investigation](../docs/research/2026-10-01-asr-latency.md). A first GPU inference also initializes kernels; repeated short clips do not establish long-meeting throughput or accuracy.

```json
{"id":"meeting-1","operation":"analyze","audio_path":"/absolute/meeting.m4a","model":"qwen3-asr-1.7b","language":"de","diarize":true,"diarization_model":"nemotron-3","offline":true}
```

Responses include `id`, `ok`, `segments` (`text`, `start`, `end`, `speaker`), `speakers`, `model`, `language`, `warnings`, and `error`. Diarized recordings also return the full `turns` and `exclusive_turns` timelines. Times are absolute seconds from recording start. Aligned segments additionally contain `words` and `timing:"word"`; unavailable/invalid word alignment falls back to marked chunk timing. Original ASR punctuation/spacing is retained despite the Qwen aligner's cleaned words. One ASR model is cached; switching/unloading releases it, and ASR is unloaded before diarization. The separate diarization subprocess is bounded by a two-hour timeout, exits after the request, and stops when its supervising worker exits. A failed request returns `ok:false`; subsequent requests remain available. Requests are limited to 64 KiB.

For actual live speaker recognition, run the persistent worker in the separate NeMo environment and configure both local `.nemo` checkpoints. Start one session, send consecutive mono 16 kHz blocks, then finalize:

```json
{"id":"start","operation":"stream_start","asr_model":"nemotron-3.5","language":"de","max_speakers":8,"offline":true}
{"id":"audio-1","operation":"stream_audio","sample_rate":16000,"pcm16_base64":"BASE64_LITTLE_ENDIAN_PCM16"}
{"id":"stop","operation":"stream_stop"}
```

Each block contains at most 16000 samples (one second). Float `samples` arrays are also accepted, but must fit the request-size limit. **Streaming responses are cumulative snapshots:** replace/upsert segments by their stable `id`; never append the full snapshot text after every block. Finalization emits a final snapshot and releases the session. Starting a session unloads batch ASR, and batch analysis is blocked until the stream stops. Nemotron 3.5 accepts 32 supported locales out of the box; ISO hints such as `de` map to `de-DE`. The eight adaptation-only locales are rejected. `multitalker-parakeet` is English-only and adds speaker-target conditioning for overlapping speech. Its extra speaker streams increase memory/compute. Anonymous IDs do not identify real people.

Verify the protocol and adapters without models or a GPU:

```sh
PYTHONPATH=speech-worker python3 -m unittest discover -s speech-worker/tests -v
```

Use `tools/benchmark_asr.py` with a private held-out JSONL manifest (`audio_path`, human `text`, optional `language`) to measure local WER/CER and real-time factor. Output files include private predictions; keep them outside the repository. The tool's simple documented normalization is **not** the Open ASR Leaderboard's exact normalization. Full evaluation requirements, silence/overlap tests and timing metrics are in the research report. Real [CPU](../docs/research/stt-evidence/real-cpu-smoke.json) and [CUDA](../docs/research/stt-evidence/real-gpu-smoke.json) smoke artifacts record exact synthetic English/German transcription and valid word spans with pinned Qwen weights. They verify adapter/CPU/CUDA compatibility; meeting quality, timing accuracy, throughput, and peak memory remain unmeasured.

## Modern quality profile

Retrieval, optional Cohere and experimental MOSS use Python 3.12 with the patched Transformers version in `requirements-modern.txt`. The original Qwen checkpoints use the bundled adapter and can share these patched libraries; the optional vLLM experiment remains separately managed. [Quality baseline](../docs/QUALITY_BASELINE.md) records setup, pins and actual validation. Cohere needs upstream gated access and a supported forced language; MOSS retains strict output rejection and a 30-minute analysis limit. Both are explicitly selected alternatives, not silent defaults.

The AppImage bridge removes bundled `PYTHONHOME`, `PYTHONPATH` and bundle-owned library search paths before launching external Python with `-E`. A worker startup exit now reports its status separately from the 64 MiB response ceiling. Runtime checking saves the fields you edited before testing them. Preserve the original transcript when health or analysis fails.

Install `requirements-eval.txt` to run `tools/score_predictions.py` and `tools/score_speakers.py`. Keep private manifests/predictions outside Git. The first tool refuses incomplete/duplicate predictions and includes rejected clips in a separate operational score; the second requires continuous meetings and declares collar, overlap and approximate word-timing policies.

## Optional multilingual word timing

Qwen's word aligner covers 11 languages, while its recognizer covers 30. Without
usable word timing, analysis first uses detected speaker changes to re-read
bounded audio spans. It preserves genuine uncertainty during overlapping speech.

An opt-in CPU fallback uses Meta's Apache-2.0 **OmniASR CTC 300M v1** checkpoint
through the existing Torch/Transformers environment. It aligns the original Qwen
text; it does not correct words or establish a person's identity. This adds about
1.3 GB of weights plus inference memory. It is not enabled or downloaded by default.

1. Install `speech-worker/requirements-alignment.txt` into the Qwen environment.
2. Download `omniASR-CTC-300M.pt` and `omniASR_tokenizer.model` from
   [`facebook/omniASR-CTC-300M` at revision
   `8e35f0cc28fa6099e0c14d56db85ce0423baa691`](https://huggingface.co/facebook/omniASR-CTC-300M/tree/8e35f0cc28fa6099e0c14d56db85ce0423baa691).
3. Set `SONEFOLD_OMNI_ALIGNER_DIR` to that local directory before starting the worker.

Both files must match the SHA-256 hashes in `sonefold_speech/omni_alignment.py`.
Loading rejects altered files, uses `weights_only=True`, maps every tensor
strictly, and never executes model repository code. Unsupported text, failed
alignment, or missing optional dependencies retain the ordinary fallback and
approximate-timing warning. Unloading also releases this CPU model.

This is an experimental opt-in pending broader language/boundary validation;
the three-case calibration is not a 99% accuracy result. See the
[primary-source review](../docs/research/2026-10-15-meeting-accuracy.md).
