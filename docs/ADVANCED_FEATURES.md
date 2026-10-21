# Local speech and meeting workflows

This guide covers Overtalk's advanced workflows, with local model choices reviewed through **30 September 2026**.

## Model choices

Use **Qwen3-ASR 1.7B** for multilingual saved-recording analysis, with its forced aligner for supported word timestamps. Its 0.6B variant reduces model size. Keep native **Parakeet TDT v3 INT8** for a smaller installation and its 25 European languages; keep Whisper for language coverage and comparison. The [STT research](research/2026-09-30-stt.md) compares thirteen candidates, releases, licenses, published accuracy evidence and benchmark limitations. There is no universal winner for every microphone, language, or meeting.

The optional NeMo worker pairs **Nemotron 3.5 Streaming ASR** with **Nemotron-3 Diarization**, retaining decoder and speaker caches throughout capture. Multitalker Parakeet is the English overlap alternative. For saved recordings, **Community-1** supports exact speaker counts and regular/exclusive turns; **Nemotron-3** estimates at most eight distinct speakers. [Diarization research](research/2026-09-30-diarization.md) explains the scoring differences that prevent direct comparison of published DER figures.

Summaries retain existing local and BYOK providers. The verified 24 GB quality profile adds pinned Qwen3.8 27B; see [quality setup and limits](QUALITY_BASELINE.md). Their pipeline now uses source references, bounded chunking and evidence reductions, reserves the requested output budget, and rejects missing references or empty/reasoning-only output. Newer models can be compared through compatible local endpoints; they are not silently added to a GGUF catalog whose runtime has not demonstrated support.

## Available workflows

| Workflow | Where to use it |
| --- | --- |
| Qwen/NeMo local speech | Transcription settings → Advanced local speech; install the optional runtime and checkpoints first. |
| Continuous live speakers | Enable live speaker transcription and select the local runtime. Model preparation runs before capture. |
| Saved-recording analysis | Meeting details → Improve transcript and identify speakers. Failed or cancelled processing preserves the old transcript. |
| Transcript recovery | Restore previous transcript preserves names, overlap, partial state and audio-source metadata. Restoration also saves the current version. |
| Speaker review | Rename or merge anonymous speakers within a meeting. Ambiguous speech remains unknown. |
| Summary templates | Six built-ins plus create, duplicate, edit, import/export and delete for custom templates. Built-ins remain read-only. |
| Portable exports | Markdown, TXT, JSON, DOCX or PDF; choose summary, transcript or both. Includes every transcript page and current summary edits. |
| Assistants and scripts | Settings → Automation → Connections. Read grants, recording control and note editing are separate; access starts disabled. |
| Signed notifications | Opt-in `summary.completed` webhook, containing IDs unless summary content is explicitly enabled. |

Live snapshots replace previous revisions rather than duplicating text. A bounded queue stops live inference visibly if it loses samples or falls behind, while audio capture continues. Enable audio auto-save to retain a recording for later full analysis. Labels remain provisional during capture and describe anonymous voices, not verified identities. Fresh clustering requires naming again because cluster numbers can change. Regenerate summaries after changing a transcript.

## Installation and operation

Native Whisper/Parakeet requires no Python. Optional speech instructions, pinned Qwen revisions and separate NeMo/Community-1 environments are in [speech-worker/README.md](../speech-worker/README.md). Health checks inspect dependencies and local checkpoint presence; loading checks actual model/device compatibility. Analysis never downloads weights or uploads recordings. Community-1 requires accepting upstream terms before its initial download.

If you keep the Qwen environment and checkpoints inside the checkout, use `speech-worker/.venv-qwen/` and `speech-worker/models/`, which Git ignores. Enter the environment's absolute Python path and checkpoint directories in Settings → Transcription → Advanced speech. Follow the portable installation instructions for another machine. NeMo and Qwen use separate environments because their dependencies differ.

On Linux, GPU speech and local LLM workers can run under an optional GPU-sharing launcher named by `OVERTALK_GPU_LAUNCHER`; see [GPU notes](GPU_ACCELERATION.md#sharing-the-gpu-with-the-desktop). Cancellation terminates a worker's private process group gracefully, so a launcher can clean up after it.

The [integration guide](../integrations/README.md) documents CLI/MCP commands, the authenticated loopback API, permissions, webhook verification and retry limits. The API exposes no LAN pairing, administration or deletion routes.

## Validation and limits

Tests cover migrations, transactional recovery, speaker isolation/overlap, continuous resampling, cancellation, template safety, source provenance, real PDF/DOCX parsing, pagination and revocation. The frontend production build bundles fonts locally. See [the validation record](research/2026-09-30-validation.md) for commands and results.

Real Qwen English/German synthetic clips passed on CPU and on an NVIDIA RTX 5090 Laptop GPU, with valid aligned word intervals. Synthetic smoke tests verify integration, not real-meeting accuracy, speaker DER, model superiority, peak VRAM or sustained throughput. Live quality and every supported language need representative recordings. Windows/macOS bundles and long-running physical audio capture require platform/device testing.

The [Library guide](LIBRARY.md) covers indexed search, cited questions, reviewed actions, tags, recurring calendar imports, streamed playback and encrypted backup/recovery. These are available independently of commercial billing or support.
