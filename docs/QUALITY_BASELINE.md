# Local quality baseline

The tested installation, on an NVIDIA RTX 5090 Laptop GPU with 24 GB, uses the following quality profile after real runtime checks. “Quality” names the chosen configuration, not a claim that one model wins every task or hardware class. Smaller profiles remain available when cold starts, memory or latency matter.

| Job | Installed choice | Practical boundary |
| --- | --- | --- |
| Saved speech | Qwen3-ASR 1.7B + ForcedAligner 0.6B | Automatic language handling; review transcript and alignment. |
| Anonymous speakers | Nemotron-3 Diarization | At most eight voices; names require manual review. |
| Local summaries and cited questions | Qwen3.8 27B calibrated IQ4_XS | 16K total context, bounded hierarchical summaries, 14.25 GB weights. |
| Semantic retrieval | Nemotron 3 Embed 8B | 4,096-dimensional normalized mean pooling; OpenMDW 1.1 weights. |
| Evidence reranking | Qwen3 Reranker 4B | Official yes/no framing, batches of two; Apache 2.0 weights. |

Large speech, retrieval and generation workers release their models before the next kind loads. Recording lifecycle locks block knowledge work during capture. This coordinates Overtalk's workers, not unrelated GPU programs. To share the GPU with other programs, workers can run under a launcher named by `OVERTALK_GPU_LAUNCHER` ([GPU notes](GPU_ACCELERATION.md#sharing-the-gpu-with-the-desktop)). Cold loading can take tens of seconds. There is no automatic cloud fallback.

## Runtime and checkpoints

Keep Qwen's Transformers 4 environment, NeMo, and the modern Transformers 5 environment separate. The modern profile uses Python 3.12, compatible PyTorch, and `speech-worker/requirements-modern.txt` (Transformers 5.6.2). The tested CUDA runtime is Torch 2.14.0+cu130. Follow [speech setup](../speech-worker/README.md) for native platform and optional speech dependencies.

```sh
python3.12 -m venv speech-worker/.venv-modern
# Install the official PyTorch build for your device first.
speech-worker/.venv-modern/bin/pip install -r speech-worker/requirements-modern.txt
speech-worker/.venv-modern/bin/python speech-worker/tools/download_models.py \
  --model nemotron3-embed-8b --directory speech-worker/models/nemotron-embed-8b \
  --revision d1f2f25730bbd775b99b29185134bc86653bf2d1
speech-worker/.venv-modern/bin/python speech-worker/tools/download_models.py \
  --model qwen3-reranker-4b --directory speech-worker/models/reranker-4b \
  --revision 22e683669bc0f0bd69640a1354a6d0aebcfeede5
```

In **Settings → Library & backups**, enable semantic search, select the modern Python executable and the two checkpoint directories, save, check models, then build the index. Health checks establish local files/dependencies; indexing tests actual model loading. Search falls back visibly to keyword results if the semantic path fails. Retrieval embeds up to 6,000 characters per source and 4,096 tokens; full-text search preserves the complete text. Corpus filtering precedes ranking. The exact dense scan is bounded at 10,000 eligible vectors.

In **Settings → Notes & AI**, the default **Qwen 3.8 27B · calibrated IQ4_XS** uses 14,252,844,960-byte Agention Precision weights, roughly 25% smaller than the original Q4_K_M. Revision and SHA-256 are pinned in the app. The 11.77 GB GSQ-RCO option and original Q4_K_M reference remain explicit alternatives. See [the quantization review](research/2026-09-30-quantization-review.md) for provenance, published evidence and local checks.

The helper pins llama-cpp-2 0.1.157 with Qwen35 runtime support, reads layer count from GGUF metadata and bounds prefill to 512 tokens. First-run defaults select the strongest supported profile that fits detected RAM and CUDA capacity. Saved manual selections are preserved; runtimes/checkpoints must still be installed before first use.

## Experimental alternatives and evidence

Cohere Transcribe 2B requires its upstream gated access agreement and a supported forced language; its app adapter has protocol tests but no real weight validation here. MOSS's reviewed Apache runtime loads locally and emits joint speaker segments, but rejected two pilot clips; retain its experimental label. Photon is excluded from the strict offline baseline because its current SDK sends metadata telemetry and lacks a verified disable switch. Models with noncommercial or incompatible use terms are not substituted into the commercial baseline.

Real [speech pilot results](research/2026-09-30-speech-pilot.md) support Qwen over Whisper Turbo only on 24 short English clips. [Generation smoke checks](research/2026-09-30-quality-generation-smoke.json) cover five synthetic bilingual citation/abstention/injection cases per model; both 27B and 4B passed. A bilingual retrieval fixture ranked relevant English/German evidence above unrelated text. These prove integration and narrow behavior, not state-of-the-art ranking or summary quality. Exact citations prove a quotation exists, not that an answer logically follows it.

See the [competitive landscape](research/2026-09-30-competitor-landscape.md), [speech review](research/2026-09-30-speech-sota-review.md), [knowledge review](research/2026-09-30-knowledge-workflow-sota.md), and [library/backup guide](LIBRARY.md). Remaining gaps include CalDAV synchronization, mobile clients, conflict-safe multi-device sync, verified cross-meeting speaker identity, full evaluation-region scoring and Windows/macOS validation. Overtalk does not claim to lead every open-source competitor.

## Automatic defaults

Hardware detection reads total RAM and the first visible NVIDIA device's memory with a bounded query. A machine with at least 22 GiB total RAM, or 14 GiB RAM plus approximately 12 GiB CUDA capacity, receives Qwen3.8 27B generation. Lower capacities use the existing 4B/2B memory tiers. Generation can offload layers or run on CPU; RAM capacity alone does not promise fast decoding.

At 22 GiB CUDA capacity with adequate host RAM, retrieval selects Nemotron8B with Qwen Reranker4B. Smaller devices receive Qwen4B or 0.6B embeddings/reranking. CPU profiles use separately conservative RAM thresholds because their weights use more memory. Speech selects Qwen1.7B when capacity allows, otherwise 0.6B. Nemotron speaker analysis stays ungated; Community-1 and Cohere require their own access agreements. These are supported task-specific choices, not a largest-parameter rule or universal accuracy ranking.

Defaults enable the local profiles. Missing dependencies or checkpoints produce setup guidance, rather than quietly switching the selected speech engine to Whisper. File import and retranscription expose the configured local speech model, preserve deliberate overrides, and pass cancellation to the worker. Their VAD segments carry chunk timestamps; use saved-meeting speech analysis for forced alignment and speaker-aware replacement. Existing settings always override fresh-install recommendations.
