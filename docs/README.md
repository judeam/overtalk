# Overtalk documentation

## Using Overtalk

- [Meeting workflows](MEETING-WORKFLOWS.md): setup check, upcoming calendar meetings, automatic notes, the meeting companion and recording notices.
- [Advanced workflows](ADVANCED_FEATURES.md): local speech models, speaker analysis, templates, exports and automation.
- [Library and backups](LIBRARY.md): search, semantic retrieval, encrypted backups and restore.
- [Sharing a meeting](SHARING.md): redacted copies as a web page or `.overtalk` file.
- [Account linking](CONNECTION-ACCOUNT-LINKING.md): connecting Linear, Atlassian, Google, Microsoft, Slack, GitHub and Notion.
- [Privatemode](PRIVATEMODE.md): encrypted cloud notes through a local Privatemode proxy.
- [Linux audio](LINUX_AUDIO.md): microphone, system audio and playback on PulseAudio and PipeWire.

## Building and contributing

- [Build Overtalk](BUILDING.md): prerequisites, building from source, environment variables and packaging.
- [Native UI](NATIVE-UI.md): the Slint interface, its builds and its validation.
- [GPU acceleration](GPU_ACCELERATION.md): CUDA, ROCm, Vulkan and Metal backends, and sharing the GPU with the desktop.
- [GPU builds on Linux](building_in_linux.md): GPU-accelerated builds of the web interface on Linux.
- [Quality tools](QUALITY-TOOLS.md): duplication, reachability and refactoring checks (October 2026 snapshot).
- [Dependency checks](DEPENDENCY-SECURITY.md): advisory checks for Rust, web and Python dependencies.

## Reference

- [Release status](RELEASE-STATUS.md): what has been checked for 0.1.0 and what must happen before a stable release.
- [Codebase map](CODEBASE-MAP.md): which module owns which behavior.
- [Platforms](PLATFORMS.md): what is verified on Linux, Windows and macOS, install layout, capture, worker isolation and signing.
- [Local quality baseline](QUALITY_BASELINE.md): the tested model profile and the automatic hardware defaults.
- [Attack-surface audit](SECURITY-ATTACK-SURFACE.md): listeners, credentials and model provenance (17 October 2026 snapshot).
- [Worker isolation](WORKER-ISOLATION.md): how inference and media workers are sandboxed (17 October 2026 snapshot).
- [Privacy operations](PRIVACY-OPERATIONS.md): a checklist for organizations deploying Overtalk.
- [Brand](brand/README.md): the Overtalk mark and its source files.
- [README media](media/README.md): how the screenshots in the README were made.

See also [SECURITY.md](../SECURITY.md) and [PRIVACY_POLICY.md](../PRIVACY_POLICY.md) at the repository root.

## Research

Dated research notes in [`research/`](research/). Each records its sources and
evidence as of its date; later code may differ.

- [Speech recognition](research/2026-09-30-stt.md) and its [evidence inventory](research/stt-evidence/SUMMARY.md) (30 September 2026).
- [Speech review](research/2026-09-30-speech-sota-review.md): newer ASR and diarization candidates (30 September 2026).
- [Speech pilot](research/2026-09-30-speech-pilot.md): Qwen, MOSS and Whisper on short AMI clips (30 September 2026).
- [Speaker diarization](research/2026-09-30-diarization.md) (30 September 2026).
- [Knowledge workflows](research/2026-09-30-knowledge-workflow-sota.md): search, cited answers, actions and backups (30 September 2026).
- [Quantization review](research/2026-09-30-quantization-review.md): Qwen3.8 27B quantized notes models (30 September 2026).
- [Competitor landscape](research/2026-09-30-competitor-landscape.md): open-source meeting notebooks (30 September 2026).
- [Validation record](research/2026-09-30-validation.md): checks run on the development build (30 September 2026).
- [ASR cold-start latency](research/2026-10-01-asr-latency.md) (1 October 2026).
- [Meeting accuracy](research/2026-10-15-meeting-accuracy.md): methods for improving multilingual meeting accuracy (15 October 2026).
