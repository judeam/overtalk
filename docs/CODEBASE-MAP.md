# Codebase map

Start at the layer that owns the behavior. Both interfaces call the same Rust
service; a feature's rules should not be independently implemented in each UI.

| Responsibility | Entry / owner |
| --- | --- |
| Desktop startup and Slint bindings | `native/src/lib.rs`, `native/ui/app.slint` |
| Controller state, initialization, request transport | `native/src/controller.rs` |
| Meeting opening, cache, pagination, transcript rows | `native/src/controller/meeting.rs` |
| Library filtering, search and answers | `native/src/controller/library.rs` |
| Notes rendering, source selection, drafts | `native/src/controller/notes.rs` |
| Playback clock, follow state, speaker filters | `native/src/controller/playback.rs` |
| Timeline rows and waveform images | `native/src/controller/timeline.rs`; `native/src/timeline.rs` does calculations |
| Service replies / live events | `native/src/controller/replies.rs`, `events.rs` |
| Preferences and device selection | `native/src/controller/settings.rs` |
| Projects, integrations, dialogs, onboarding | Named siblings in `native/src/controller/` |
| Native IPC framing and command delivery | `crates/desktop-protocol/`, `frontend/src-tauri/src/native_service.rs` |
| Command registration / generated adapters | `frontend/src-tauri/src/lib.rs`, `scripts/generate-native-dispatch.py` |
| Command payloads / transcript conversion | `frontend/src-tauri/src/api/types.rs` |
| Recording lifecycle, import, retranscription | `frontend/src-tauri/src/audio/`; shared batch behavior in `common.rs` |
| Cloud model catalogs, cache and bounded responses | `frontend/src-tauri/src/model_catalog.rs`; provider folders retain filters, fallback lists and public DTOs |
| Web cloud-model discovery state and fallback lists | `frontend/src/hooks/useCloudModelCatalog.ts`; rejects stale account/provider results |
| Speech inference / loading | `parakeet_engine/engine.rs`, `whisper_engine/engine.rs` under the service |
| Model transfer / cancellation | Each engine's `engine/download.rs`; behavioral regressions in `engine/tests.rs` |
| Summary providers and grounded notes | `frontend/src-tauri/src/summary/` |
| Summary manager initialization / identity | `summary/summary_engine/manager_state.rs` under the service; startup and commands share one retryable initializer |
| Summary transport, processing and compaction regressions | Each summary owner's `tests.rs` child module |
| Projects, calendar, connections, review, jobs | `frontend/src-tauri/src/extensions/` |
| Shared copies (web page, .overtalk file) and opening them | `extensions/share.rs`; redaction rules in `extensions/privacy.rs`; see `docs/SHARING.md` |
| Security and data boundaries | `frontend/src-tauri/src/security/`; worker restrictions in `crates/worker-sandbox/` |
| Document parsing / export | `crates/meeting-documents/` |
| Isolated local inference | `speech-worker/sonefold_speech/`, `llama-helper/` |
| Voice embeddings and open-set speaker matching (Apache-2.0) | `crates/voice-memory/` |
| Screen capture helper (`overtalk-capture`) | `crates/capture-helper/` |
| Browser native-messaging bridge | `crates/browser-bridge/`; extension in `integrations/browser/` |
| Read-only library queries shared by the local API and CLI | `crates/library-reads/` |
| Command-line and MCP client (`overtalk-cli`) | `crates/cli/` |
| Assistant, script and webhook integrations | `integrations/` (CLI/MCP/API guide, browser extension, assistant skill, earlier `sonefold.py` client) |
| Optional hosted summary server (accounts, billing, Privatemode keys); never bundled in the desktop | `crates/cloud-gateway/` |
| Retained web routes / feature UI | `frontend/src/app/`, `frontend/src/components/` |
| Web state, transport, domain helpers | `frontend/src/hooks/`, `contexts/`, `services/`, `lib/` |
| Complete saved-transcript reads for web copy and notes | `frontend/src/services/transcriptService.ts` |
| Browser crash-recovery transactions / ordering | `frontend/src/services/indexedDBService.ts`; its tests use an isolated IndexedDB adapter |
| Shared speech model status / size display | `frontend/src/lib/speech-model.ts` |
| Meeting benchmark generation and grading | `evaluations/meetingbench/` (separate budget and private artifacts) |
| Word error rate on the AMI Meeting Corpus | `evaluations/ami/` |
| Disposable-profile integration client | `scripts/service_client.py`, used by `verify-*.py` and performance tools |
| Unix Tauri build / dev entry points | `frontend/{build,dev}-gpu.sh` → `frontend/scripts/run-gpu.sh` → `frontend/scripts/tauri-auto.js` |

The controller remains one state owner; feature modules are inherent implementations
of that same controller, not extra forwarding objects or a new event framework.
Generated adapters intentionally duplicate dispatch syntax while preserving distinct
native/web allowlists. Platform audio implementations, protocol rejection fixtures,
and inference vendors must not be collapsed based on text similarity alone.

## Finding things

Search a command name in the registration file, then its implementation and callers.
Search a Slint callback in `native/ui/` and its binding in `native/src/`. Search web
imports from the relevant App Router page; `frontend/knip.json` describes additional
real entry points. Run the quality scanner before and after changes, then inspect
its reports alongside compiler and behavior-level tests.

Historical `.backup`/`_old` implementations belong in Git history. Audit reports
are evidence snapshots, not alternative architecture specifications.

The old, uncompiled `audio_v2/` prototype has been removed. The active capture,
mixing and processing implementation is `frontend/src-tauri/src/audio/`.
The unused audio-metrics batcher and empty microphone capture placeholder are
also removed. Peak levels still come from the active pipeline's `LevelMeter`;
recording diagnostics retain the pipeline counters and overflow warnings.
The unused async logger and never-consumed audio buffer pool are removed too;
recording state has one derived default rather than duplicate constructors.
Logging goes through the application logger and its privacy filter. The obsolete
Windows `build_backup.bat` is gone; current platform build scripts remain.

`frontend/knip.json` keeps `prettier` as an explicit exception because it is a
manual contributor formatter, not a runtime import. Tests and independent CLI
entry points are listed narrowly; adding a new entry needs a real caller.
