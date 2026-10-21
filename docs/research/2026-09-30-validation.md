# Validation record — 30 September 2026

This record describes checks actually run in the local checkout. The first sections preserve the earlier installation checkpoint; later sections record the library/quality checkpoint and the current audio/meeting build. It does not claim Windows/macOS installers or representative meeting accuracy.

| Check | Result |
| --- | --- |
| Native Rust library | 277 passed; 2 existing ignored tests; 1 audio-device enumeration test excluded |
| Frontend Bun tests | 61 passed, 393 assertions |
| Frontend TypeScript | `pnpm exec tsc --noEmit` passed |
| Next.js production export | All 11 routes built with locally bundled fonts |
| Renamed Python speech worker | 55 passed in the real NeMo environment |
| Scoped CLI/MCP companion | 14 passed |
| Local GGUF sidecar | 3 passed |
| Portable GGML regression checks | 14 passed |
| Whitespace | `git diff --check` passed |

## Commands and environment

Frontend checks ran in `frontend/` with Node 20.20.1, pnpm 9.15.9 and Bun 1.4.2:

```sh
pnpm exec tsc --noEmit
bun test
pnpm exec next build
```

Python tests ran from the root with `PYTHONPATH=speech-worker`, using the separately installed NeMo environment for tensor tests. The companion uses Python's standard library and temporary SQLite databases.

Native tests ran with `cargo test -p sonefold --lib --locked --offline -- --skip test_list_system_audio_devices`, four Cargo jobs and CPU nice +5. The local ORT 1.22 runtime was provided with `ORT_LIB_LOCATION` and `LD_LIBRARY_PATH`; development FFmpeg and the built CPU GGUF sidecar were prepared. Local-socket regression fixtures required execution outside the network-restricted sandbox. The physical audio enumeration test hangs in that sandbox, so it was excluded rather than reported as passing.

## Real inference evidence

Qwen3-ASR 1.7B plus forced alignment produced the complete expected English/German synthetic text on CPU and the RTX 5090 Laptop GPU, with 33 valid word spans. [CPU evidence](stt-evidence/real-cpu-smoke.json) and [CUDA evidence](stt-evidence/real-gpu-smoke.json) record checkpoint revisions and runtimes.

NeMo's speaker-tagged streaming protocol ran on CPU and CUDA with reviewed local checkpoints. CUDA prewarming happens before capture and resets per-recording state. The generated English and longer German clips produced final text; the 6.62-second German clip remained empty. Post-prewarm maximum CUDA audio request duration was below 0.6 seconds on these fixtures. See [pinned diarization evidence](2026-09-30-diarization-evidence.json).

All heavy GPU runs used a GPU-sharing launcher at 85% of GPU threads.

## Practical limits

The speech smoke tests establish integration and device compatibility, not real-meeting WER/DER, model superiority, peak VRAM or sustained throughput. No gated Community-1 model was downloaded without its upstream access agreement. Windows/macOS installers, long recordings, overlapping real voices and physical multi-device capture still need platform testing.

The visual preview uses synthetic IPC responses in an isolated headless browser. It contains no real recordings or user data and is separate from native hardware validation. The original `AGENTS.md` and `LICENSE.md` remain unchanged.

## Installed Linux test build

The CUDA x86_64 build targets SM 120 and uses application identity `com.judeam.sonefold`. Its AppImage was built successfully and the same unpacked application tree was installed under `~/.local/opt/sonefold/app/`; this avoids requiring FUSE. The `sonefold` launcher and desktop entry used a GPU-sharing launcher when one was available.

Native desktop startup opened the Sonefold window, initialized its own SQLite database and applied all 13 migrations. The test installation selects local Qwen3-ASR 1.7B with alignment and Qwen 3.5 4B summaries. NeMo checkpoints are stored persistently, with experimental live diarization disabled initially. No microphone recording or existing database import was performed during this check.

The pinned Qwen 3.5 4B Q4_K_M checkpoint also passed a real CUDA helper smoke test: a synthetic one-sentence source returned the correct owner, task, recipient and deadline without error. [Checkpoint and response evidence](2026-09-30-summary-evidence.json) includes the verified SHA-256. This is a compatibility check, not a broad summary-quality benchmark.

The finish reviewer scored all four listed persistence, accessibility, contrast and copy fixes resolved. That verdict covers the scored fixes in the supplied previews. A Higgsfield-generated concept image was selected as the brand reference; historical failed exact-comp measurements remain recorded, and no pixel-fidelity pass is claimed. `DESIGN.md` and its token sidecar describe the implemented system.

After the workspace moved to a new directory, the packaged batch and streaming workers both passed offline dependency/checkpoint health checks using the relocated environments. These health checks do not load models; the separate real-inference evidence above covers device compatibility.

The initial GitHub run exposed a leaked local-storage `window` fixture and shared module-mock dependencies. Fixtures now restore their globals, viewport-dependent hooks install their own window, and `pnpm test` runs all 15 suites in separate Bun processes. The final isolated run passed all 61 tests and 393 assertions; TypeScript remained clean.

## Final library and quality profile checkpoint

The final CUDA AppImage was rebuilt and installed on 30 September after the workspace, Python bridge, local-provider reload and GGUF layer-count fixes. Results supersede the earlier checkpoint above:

| Check | Current result |
| --- | --- |
| Native Rust library, CUDA feature | 297 passed, 2 ignored; physical audio-device enumeration excluded |
| GGUF helper | 4 passed |
| Frontend Bun tests | 68 passed across 17 isolated suites |
| Speech/retrieval worker unit tests | 63 passed |
| Scoped CLI/MCP companion | 16 passed |
| TypeScript and production export | Passed during final native packaging |
| Linux CUDA AppImage | Built successfully; matching application tree installed |
| Backup form | Invalid create/restore inputs prevented native pickers; restore required no confirmation field |
| Native speech setup | Installed UI's Check runtime reported ready; Qwen1.7B/CUDA/offline checkpoint found |
| Native retrieval setup | Nemotron8B and Qwen Reranker4B checkpoint health passed through the installed bridge |

Native executable SHA-256: `12c5ff735db9c56663b64169be4703b4f05d9ff393dd96a3671bb1c5441c54cb`. AppImage SHA-256: `cf2782b50457e58dc28ba5d64fd994a6217906a6a28f845e94ed6391dab7450b`. Installed packaged worker modules matched source bytes. The previous application tree, settings snapshot and database backup were retained for rollback.

The test installation selects Qwen3-ASR1.7B with alignment, Nemotron-3 speaker analysis, Qwen3.8 27B Q4_K_M generation, Nemotron3 Embed8B and Qwen3 Reranker4B. Models release between Sonefold GPU workloads; recording lifecycle locks exclude simultaneous knowledge work. See [the quality profile](../QUALITY_BASELINE.md), [matched speech pilot](2026-09-30-speech-pilot.md) and [bilingual synthetic generation smoke](2026-09-30-quality-generation-smoke.json). Both tested generation models passed five fixture checks; this does not establish relative summary quality. Cohere remains gated and unvalidated with actual weights. MOSS remains experimental after two rejected pilot clips.

Native startup exposed a separate WebKit font-cache loop. A fresh cache with the **original application data/configuration** resolved it; isolating only HTTP web cache did not. Four system WOFF font patterns lacked a family name, matching the failure mechanism in upstream [bug 325185](https://bugs.webkit.org/show_bug.cgi?id=325185). Sonefold's launcher now has its own cache directory plus the tested NVIDIA painting environment. No font files, shared caches, application preferences or database rows were deleted. Earlier web-cache directories were preserved as backups.

After that fix, the installed application rendered its original workspace and settings. The normal Qwen virtualenv interpreter, without the temporary shim, passed the actual Rust-to-Python health request under AppRun: dependencies found, checkpoint cached, `ready=true`, `device=cuda`, `offline=true`. The real UI Check runtime control displayed “Runtime dependencies and checkpoint were found,” and Use for transcription saved Qwen as the selected engine. Final restart verification then exposed a separate localSpeech provider lookup failure: the API-key repository recognized Parakeet but omitted the external local engine. Two regression tests reproduced it before the fix, covering all external model choices, key-free loading and preservation of cloud credentials. Both local engines now return no credential during configuration reload. A health check does not load weights; the separate real inference evidence covers model compatibility. No physical microphone recording was made.

The library finish reviewer scored the one listed backup-form fix resolved and returned ship. That verdict covers the supplied fix, not every surface or animation performance. The fresh design documenter verified current source hashes and palette evidence without changing DESIGN.md or its token sidecar. The headless previews use synthetic IPC; the native rendering and health results above came from the installed application. AGENTS.md and LICENSE.md remain byte-identical.

The final saved-audio integration check also corrected the importer/retranscriber and shared model picker: local speech now uses the external worker rather than the legacy Whisper path, an unavailable configured baseline requires an explicit alternate selection, and cancellation reaches the worker token. Batch jobs share recording/knowledge lifecycle coordination and release their models after completion. Providers without a confidence score retain an unknown score instead of a fabricated constant. Chunk timestamps remain VAD-based; saved-meeting speech analysis performs forced alignment and speaker-aware replacement.

Fresh-install recommendations now use bounded, read-only hardware detection for the supported generation, ASR and retrieval tiers. The quality profile is the default on hardware that meets its thresholds, not an optional manually selected speed downgrade. Existing user overrides are preserved, including during onboarding. Required runtimes/checkpoints must still be installed; no gated agreement is accepted automatically. Hardware-profile and dialog-selection regressions cover these choices. No Windows/macOS installer or general leaderboard superiority is claimed.

## Current audio, notes and model checkpoint

The corrected CUDA AppImage and matching application tree were installed after the desktop device, streaming playback, editor palette and citation-ID fixes. This checkpoint supersedes the earlier installer hashes and Q4_K_M default above.

- Native library: 301 tests passed, two existing tests ignored, one physical enumeration test excluded. After the citation fix, all 34 extension tests passed, including the new short-ID regression.
- Frontend: 73 tests passed across 19 isolated suites; TypeScript and the 12-route production export passed.
- Unchanged worker/helper checks: speech/retrieval 63, companion 16, GGUF helper four and portable GGML 14 passed at the preceding source checkpoint.
- Installed modules: all 14 packaged Python speech modules and the companion matched source bytes; the installed executable matched its recorded hash.
- Quantizations: calibrated AP-IQ4_XS, GSQ-RCO-IQ3_S and the Q4_K_M reference each passed five synthetic bilingual quotation/abstention/injection cases. Both smaller files were downloaded and SHA-256 verified. These narrow checks do not establish a general quality ranking.

The installation now selects Qwen3-ASR 1.7B/CUDA and Qwen3.8 27B AP-IQ4_XS. Fresh capable systems recommend 1.7B; the 0.6B option and deliberate saved overrides remain available. Native health found the offline checkpoint. A root-owned synthetic recording was imported and retranscribed through Qwen, with its expected text and valid timing. The 27B provider completed a native summary.

The final installed playback service loaded that 6.457-second recording, sought to two seconds and played to completion while muted. Desktop discovery returned six friendly outputs and four inputs, matching card/port descriptions and disabling inactive ports. Unit tests cover stable routing and inactive-source rejection; live physical multi-device recording remains untested.

The real installed retrieval pipeline used Nemotron8B embeddings and Qwen4B reranking, returning `hybrid-reranked` with no fallback warnings. Qwen27B answered “Alice will send the report on Friday” with an exact quote from the scoped synthetic transcript. Initial testing exposed copied `[S1]` summary labels; request-local `E1`/`E2` identifiers now resolve strictly to retrieved source revisions. Unknown IDs and wrong-source quotes remain rejected. Quote checks do not prove logical entailment.

Native WebKit confirmed the notes editor uses bundled Source Sans 3 and the same charcoal background as its surrounding paper, with no horizontal overflow. Synthetic wide/light/dark/compact previews passed separately. No user meeting screenshots or audio enter the repository. The synthetic meeting, saved answer and recording folder were removed after testing.

Native executable SHA-256: `98b3e4fdc5deaf71e77fd599661f5f5d4c75c0c5f7168d6e22397a6d578e1f80`. AppImage SHA-256: `21f10dfe1baf39b116dbb6f04d5e334db7be9dc64daafdade40e174701b16a47`. The prior installation remains available for rollback. Final startup uses the normal launcher without the temporary inspector. Original AGENTS.md and LICENSE.md remain byte-identical.
