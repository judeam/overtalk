# Local meeting speaker diarization — 30 September 2026

## Scope and decision

This review compares published models and their official implementation paths for Sonefold's local Tauri desktop app. Sources were inspected on 30 September 2026. Model cards, documentation, and `main` branches are mutable; deployment evidence below records reviewed checkpoint and source revisions. Only generated speech was used for local compatibility checks; no private recording or representative accuracy benchmark was used.

**Recommended evaluation order:** Nemotron 3 Diarization for meetings with at most eight voices, then pyannote Community-1 for unconstrained speaker counts or explicit count hints. Use a full-recording final pass, independent of ASR. Keep live labels provisional. This is an engineering recommendation from available evidence, not a claim that one model wins every language, microphone, or overlap condition.

Nemotron 3's release is explicitly dated **23 September 2026**, corroborated by NVIDIA's dated announcement. It is therefore eligible for this review's cutoff. Its first-arrival ordering and streaming speaker cache address identity consistency within a session. [NVIDIA announcement](https://huggingface.co/blog/nvidia/nemotron-diarization)

## Model and technique comparison

| Candidate | Local deployment | Speaker count / operation | Fit for this repository |
| --- | --- | --- | --- |
| NVIDIA Nemotron 3 Diarization | Open-weight, OpenMDW 1.1; NeMo checkpoint | Eight speaker channels; offline and streaming | First candidate for the NVIDIA workstation; separately install its runtime. [Model card](https://huggingface.co/nvidia/Nemotron-3-Diarization) |
| pyannote Community-1 | Local weights, CC-BY-4.0; gated initial download | Automatic count; optional exact/lower/upper count hints | Strong general-purpose offline fallback; returns regular and exclusive turns. [Model card](https://huggingface.co/pyannote/speaker-diarization-community-1) |
| pyannote Precision-2 | Commercial service; self-hosted variant separately arranged | Automatic or constrained count; richer confidence output | Accuracy reference rather than a distributable local default. [Release](https://www.pyannote.ai/blog/precision-2), [implementation](https://github.com/pyannote/pyannote-audio) |
| NVIDIA Streaming Sortformer 4spk-v2 | CC-BY-4.0 NeMo checkpoint | Four speakers; online cache | Useful older baseline; unsuitable for a meeting with more than four distinct voices. [Model card](https://huggingface.co/nvidia/diar_streaming_sortformer_4spk-v2) |
| NeMo clustering + TitaNet + MSDD | Local cascaded pipeline | Configurable count; multiscale refinement | Useful high-count/tunable baseline, with more configuration and error sources. [Models](https://docs.nvidia.com/nemo-framework/user-guide/latest/nemotoolkit/asr/speaker_diarization/models.html), [configuration](https://docs.nvidia.com/nemo-framework/user-guide/latest/nemotoolkit/asr/speaker_diarization/configs.html) |
| WhisperX alignment | Local ASR/alignment orchestration | Language-specific word alignment plus separate diarization | Useful timestamp tooling, not a replacement diarization model. [Maintainer repository](https://github.com/m-bain/whisperX) |

### Accuracy evidence and its limits

NVIDIA reports DIHARD III full-set DER of **12.73%** for Nemotron 3 against **19.09%** for Streaming Sortformer v2.1 at a 30.4-second input buffer. At a 1.04-second buffer, Nemotron scores 13.18%. Its AMI, AliMeeting, and NOTSOFAR results use forced-aligned reference annotations; comparisons against original annotations are a different experiment. Advertised speed was measured on RTX PRO 5000, not this RTX 5090 Laptop GPU. [Nemotron model card](https://huggingface.co/nvidia/Nemotron-3-Diarization)

Pyannote's published September 2025 benchmark uses automatic count, zero forgiveness collar, and overlapping speech. Its AMI single distant microphone DER is **22.7% / 19.9% / 15.6%** for legacy 3.1 / Community-1 / Precision-2. DIHARD III is **21.4% / 20.2% / 14.7%**. These figures are useful comparisons within that protocol, not grounds to rank every vendor's table together. [Community-1 benchmark](https://huggingface.co/pyannote/speaker-diarization-community-1)

Sortformer's published latency is the required audio buffer, excluding computation. Older four-speaker models degrade when more speakers appear; the model does not become an unlimited-count model merely because only four people speak at once. [Streaming Sortformer card](https://huggingface.co/nvidia/diar_streaming_sortformer_4spk-v2)

TitaNet's 0.66% VoxCeleb verification EER measures a different task. Its very low legacy meeting DER uses oracle VAD in the cited configurations. Neither metric establishes fully automatic noisy meeting performance. TitaNet extracts voice embeddings; it does not supply names or complete turn segmentation alone. [TitaNet model card](https://huggingface.co/nvidia/speakerverification_en_titanet_large)

### Why multiscale segmentation and neural refinement matter

A clustering pipeline first detects speech, extracts embeddings, estimates clusters, then refines turn boundaries. NeMo's implementation uses MarbleNet VAD, TitaNet embeddings, and MSDD. Long windows improve voice representation but smear short turns; MSDD learns how much to weight different window lengths. [NeMo model documentation](https://docs.nvidia.com/nemo-framework/user-guide/latest/nemotoolkit/asr/speaker_diarization/models.html), [MSDD paper](https://arxiv.org/abs/2203.15974)

Spectral clustering is a viable baseline when built over learned speaker embeddings, not raw audio loudness or handcrafted frequency vectors. Count estimation, overlap, and short backchannels still require attention. NeMo's clustering configuration exposes count estimation and a configurable `max_num_speakers`; a configured upper bound is not evidence that all speakers were detected. [NeMo configuration](https://docs.nvidia.com/nemo-framework/user-guide/latest/nemotoolkit/asr/speaker_diarization/configs.html)

Community-1 uses learned segmentation and embeddings with Bayesian clustering. Its official source distinguishes regular overlapping output from exclusive output intended for transcript reconciliation. Retain both representations instead of throwing overlap information away. [Pipeline implementation](https://github.com/pyannote/pyannote-audio/blob/main/src/pyannote/audio/pipelines/speaker_diarization.py)

### Alignment, overlap, and named people

WhisperX aligns decoded words with a language-specific acoustic aligner. It then assigns diarization labels separately. Unsupported dictionary characters, some numbers, and unsupported languages can leave words unaligned; Whisper/WhisperX also document weak overlap handling. Preserve those words with unknown timestamps rather than silently dropping them. Native ASR word timestamps are preferable when sufficiently accurate; evaluate forced alignment when boundary errors dominate speaker assignment. [WhisperX limitations and API](https://github.com/m-bain/whisperX)

Diarization determines anonymous voices and time ranges. Named identity requires a separate user mapping or consented enrollment/verification. Never infer someone's legal name from a cluster ID. Cross-meeting biometric matching needs its own evaluation; cosine similarity is not a calibrated identity confidence score. Precision-2 publishes per-speaker confidence for correction workflows, but local Community-1 output should not be decorated with invented probabilities. [Precision-2 confidence](https://www.pyannote.ai/blog/precision-2), [pyannote feature definitions](https://docs.pyannote.ai/features)

NVIDIA's coupled streaming pipeline maintains per-speaker ASR streams: overlap-conditioned Multitalker Parakeet or activity-masked Nemotron 3.5. Interval cutting alone retains mixed voices. This makes coupled decoding a stronger candidate for heavy cross-talk. [NVIDIA ASR integration guide](https://huggingface.co/nvidia/Nemotron-3-Diarization/blob/main/ASR_INTEGRATION_GUIDE.md)

## Repository findings

The current capture path mixes microphone and system audio before live transcription (`audio/recording_manager.rs`). `audio/transcription/provider.rs` returns text, optional ASR confidence, and a partial flag; it has no words or diarization turns. `audio/retranscription.rs` uses VAD boundaries and replaces transcript rows in a transaction.

The existing `20251110000001_add_speaker_field.sql` column explicitly stores `mic`/`system` source attribution. It is not evidence of learned speaker identification. SQLite, API responses, `transcripts.json`, and paginated display each need explicit speaker metadata propagation.

The implementation added here separates `speaker_id` from the original source field, stores full overlapping model turns, joins editable meeting-local names on reads, and supports transactional merge. Unknown/ambiguous turns remain unassigned. Transcript text, IDs, and audio timing survive diarization and name edits. Full ASR reanalysis resets names because fresh clustering can reorder anonymous IDs.

## Local integration contract

Use a managed local process under `speech-worker/`. Keep newline-delimited JSON on stdout and diagnostic output on stderr. The native core owns cancellation, bounded queues, meeting-path validation, persistence, and Tauri events.

Request example:

```json
{"id":"job-123","operation":"analyze","audio_path":"/meeting/recording.wav","diarize":true,"diarization_model":"nemotron-3"}
```

Worker output carries second-based finite intervals:

```json
{"turns":[{"start":0.3,"end":2.1,"speaker":"SPEAKER_00","confidence":null}],"exclusive_turns":[],"speakers":[{"id":"SPEAKER_00","name":"Speaker 1"}]}
```

`diarization.py` loads a configured local `.nemo` checkpoint through NeMo, or a local Community-1 directory through pyannote. It never switches to hosted inference or downloads implicitly. Exact/minimum/maximum count constraints are accepted by Community-1; the Nemotron adapter rejects unsupported count constraints explicitly. Validate the runtime and checkpoint before enabling the action.

`assign_speakers` matches words or segments by the union of intersecting intervals per speaker. Require at least 55% temporal support and a 15-point lead over the next speaker; otherwise return unknown. Those thresholds are initial product policy and require tuning on representative fixtures. Use genuine exclusive turns when supplied, while separately retaining regular overlap. `speaker_confidence` stores temporal support, not biometric certainty. Index intervals to avoid scanning every turn for every word.

The native `apply_diarization_on_connection` helper participates in the caller's SQLite transaction. It validates all turns before writes, preserves transcript rows, replaces full-turn metadata, and upserts meeting-local names. Rename/merge commands scope every update to `meeting_id`; merging does not change text or timestamps. Emit `meeting-speakers-updated` after committed updates.

## Installation, privacy, and GPU policy

Community-1 requires accepting Hugging Face conditions and sharing contact details for initial model access. A complete local copy runs offline afterward. [Community-1 setup and offline usage](https://huggingface.co/pyannote/speaker-diarization-community-1)

Disable optional pyannote usage metrics with `PYANNOTE_METRICS_ENABLED=0`; its repository documents tracking model origin, pipeline class, recording duration, and speaker-count settings when metrics are enabled. Also set `HF_HUB_OFFLINE=1`, `TRANSFORMERS_OFFLINE=1`, and `HF_HUB_DISABLE_TELEMETRY=1`; preload local artifacts and keep HF credentials outside process requests and logs. Verify behavior with network blocked. [Pyannote telemetry documentation](https://github.com/pyannote/pyannote-audio)

The official setup uses a recent NeMo Speech checkout and CUDA extras. [Environment setup](https://huggingface.co/nvidia/Nemotron-3-Diarization/blob/main/ASR_INTEGRATION_GUIDE.md) Use an isolated environment when dependencies conflict with Qwen ASR. The official GitHub API returned source revision `00278b0bd95bb2b8174b88012aa21b003c59d2e9`, committed at `2026-09-30T05:45:59Z`; its Sortformer, `SpeakerTaggedASR`, and configuration helpers match this adapter. [Pinned NeMo source](https://github.com/NVIDIA-NeMo/Speech/tree/00278b0bd95bb2b8174b88012aa21b003c59d2e9)

Load ASR and batch diarization sequentially to conserve GPU memory. Limit CPU threads and queue depth. When the same GPU drives the desktop, run heavy inference under a GPU-sharing launcher ([GPU notes](../GPU_ACCELERATION.md#sharing-the-gpu-with-the-desktop)).

## Verification and remaining evaluation

Automated regression coverage includes silence, equal sequential speaker splits, simultaneous speech, interval unions, missing local models, invalid timestamps/counts, source preservation, unaligned words, meeting isolation, exclusive/overlap reconciliation, and atomic failure. Run Python tests from `speech-worker/` with `python3 -m unittest discover -s tests`; native tests use `cargo test -p sonefold --lib speakers::tests`.

A real offline CPU smoke test restored the ungated Nemotron `.nemo` at model revision `f667ed73aee57d40cc39428eb768b4fd87a0a29e` using that pinned NeMo source (`3.1.0+00278b0bd`), Python 3.12.12, and Torch/Torchaudio `2.7.1+cpu`. The adapter returned three valid intervals for a 6.46-second synthesized English fixture, with one anonymous speaker and the last speech interval ending at 6.10 seconds. Observed wall time was 4.95 seconds including imports and model restoration; this is an API compatibility check, not representative quality or throughput evidence. The actual stdio diarization operation also passed English/German fixtures. Four CPU threads and nice +5 were used. The isolated environment and weights remain ignored/outside tracked source.

Before selecting a default, evaluate consented local fixtures covering English/German/code-switching, distant microphone, headset/system mix, short interruptions, echo, five-to-eight voices, and nine-plus voices. Report DER with zero collar and overlap included, speaker-count MAE, word-level speaker attribution error, p95 finalization time, peak VRAM, and realtime factor. Match reference labels and splits across systems. DIHARD establishes why conversational diversity matters. [DIHARD III paper](https://arxiv.org/abs/2012.01477)

True live diarization needs per-recording speaker/decoder caches and pending PCM; NVIDIA's file replay driver does not supply a microphone connection. [Live integration guide](https://huggingface.co/nvidia/Nemotron-3-Diarization/blob/main/ASR_INTEGRATION_GUIDE.md) The added `streaming.py` uses persistent `SpeakerTaggedASR` sessions with structured timestamped snapshots, bounded PCM ingress, and stop-time flushing.

`speech_stream.rs` now connects continuous mixed capture before VAD to that worker. Its persistent anti-aliasing resampler removes filter delay and crops convolution padding to the real captured duration. Captured silence survives; recording pauses follow the saved audio clock. Both active input buffers are synchronized before mixing. Mixer sample loss or a full eight-window queue cancels live processing with a visible warning instead of advancing through missing audio. The recording remains available when audio auto-save is enabled.

The UI replaces cumulative revisions by stable, session-scoped segment IDs, including corrected or removed provisional words. Native transcript history, five-second atomic JSON/IndexedDB checkpoints, saved SQLite speaker metadata, and provisional flags all use the same revisions. Final Stop drains the queue and resampler before decoder finalization. ASR and streaming workers run in separate interpreters when configured. Tests cover state isolation, silence/duration preservation, overload, cumulative revisions, equal-frame IDs, overlap, saved speaker metadata, and meeting isolation.

The pinned NeMo integration's masked-ASR helper multiplied inactive log-mel frames by zero while applying the silence floor only where the original tensor equaled zero. The adapter fixes that condition: inactive frames receive the documented silence floor, and active zeros remain unchanged. Tensor regression tests cover batch/channel broadcasting, left-cache padding, and invalid mask shapes. [Inspected helper source](https://github.com/NVIDIA-NeMo/Speech/blob/00278b0bd95bb2b8174b88012aa21b003c59d2e9/nemo/collections/asr/parts/utils/multispk_transcribe_utils.py)

Actual CPU stdio streaming with Nemotron 3.5 revision `ea30d66debe3740a08b573244286791d423d6b3e` produced English partial/final speaker-tagged segments. A 6.62-second synthetic German fixture returned no text; a longer 16.96-second German fixture produced the latter sentences with substantial omissions and word errors. Direct unmasked ASR omitted the same earlier text, so masking alone does not explain the limitation. These cases establish runtime compatibility, not multilingual meeting accuracy. Microphone/system capture quality, long-session cache stability, speaker attribution under real overlap, finalization latency, and memory use still require representative hardware evaluation.

The same real stdio protocol also completed on the RTX 5090 Laptop GPU using Torch/Torchaudio `2.7.1+cu128` under a GPU-sharing launcher that limited the worker to 85% of GPU threads; English and longer German produced speaker-tagged final text, while short German remained empty. All final timestamps stayed within real captured duration. The cold first CUDA step took 13.77 seconds, exceeding the capture queue's budget. `stream_start` now warms the maximum decoder batch before capture, then discards every warmup speaker/decoder state and resets the first real step. Measured maximum audio-request times after this change were 0.597 / 0.034 / 0.525 seconds for English / short German / longer German. Startup still included model restoration and warming (25.7–55.5 seconds). These are single-voice compatibility measurements, not meeting throughput promises. All 55 worker regression tests passed in the real NeMo environment. [Recorded revisions, runtime, outputs, and limitations](2026-09-30-diarization-evidence.json)
