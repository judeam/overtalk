# Open-source meeting-notebook landscape

Research date: **30 September 2026**. Public revisions and sources were inspected around **16:10–16:20 UTC**. This report evaluates Sonefold against complete workflows, not repository stars or model names. It does **not** establish an overall product, transcription, diarization or summary-quality lead.

## Findings that change the implementation priorities

1. **Meet2Notes is a close cross-platform challenger.** It combines local speech engines with searchable meeting history, tags, hybrid retrieval, a meeting assistant and timestamp-linked review. More model choices and MCP support alone do not distinguish Sonefold. Its current 0.7 documentation is substantially more specific than search-engine snippets describing 0.6.1. [Release README][m2n-readme], [library behavior][m2n-library], [retrieval design][m2n-rag]
2. **Anarlog is the strongest broad desktop workflow reference in this review.** Its community app remains MIT despite a commercially licensed enterprise directory. The current product is Anarlog, formerly Hyprnote; the team's separate char application is not this repository. Calendar, notes/transcript chat, export, CLI/MCP, mobile beta and optional sync broaden the workflow. Its desktop Windows/Linux availability must be distinguished from its built-in on-device speech paths, which the current provider documentation limits to supported Macs. [Repository][an-readme], [license boundary][an-licensing], [providers][an-providers]
3. **Current Screenpipe is source-available, not an OSS product under its old MIT terms.** Its license expressly restricts commercial and competing-product use. Earlier MIT versions keep their earlier license; the current revision does not. It remains a behavioral reference for searchable audio/screen history, not a source donor for Sonefold. [Current license][sp-license]
4. **Amical is primarily a dictation reference.** Its current roadmap marks microphone-plus-system-audio meeting transcription and MCP as planned, and voice-note summaries/tasks as in progress. Linux desktop support is explicitly absent. It should not receive the same completed-meeting feature score as Anarlog or Meet2Notes. [README][am-readme]
5. **Open Granola illustrates why README checkboxes are insufficient.** Its advertised all-platform capture and calendar integration encounter placeholder code in the inspected revision. Use the workflow ideas as proposals, not evidence of a shipping capability advantage. [Loopback source][og-loopback], [calendar source][og-calendar]

## Evidence rules and cutoff

The following labels apply throughout:

| Label | What it means | What it does not prove |
| --- | --- | --- |
| **D — documented** | A dated official README, product page or guide states the behavior. | That installation, permissions or a real meeting work on the target machine. |
| **S — source inspected** | Relevant implementation files were read at the pinned revision. | That all call paths, dependencies and releases execute correctly. |
| **P — planned / prototype** | Upstream marks the behavior planned, or checked source contains a placeholder. | A completed product capability. |
| **R — reported measurement** | Upstream publishes measurements and methodology. | Independent reproduction or comparable hardware/data/protocol. |
| **V — locally validated** | A recorded Sonefold check was actually run previously in this checkout. | Representative meeting quality unless the check used representative labeled meetings. |

No competitor executable, container, download installer, calendar integration or hosted account was run during this research. Public source retrieval is an inspection check, not runtime validation. No credentials, personal meetings or user recordings were used. Unverified capabilities remain unverified rather than being scored as absent.

The GitHub API was queried without authentication using `commits?until=2026-09-30T23:59:59Z&per_page=1`; it returned each default branch's latest available revision at inspection time. This is **not** a guarantee of the final state at the end of September 30. READMEs, licenses and selected source/docs were then retrieved by immutable SHA. First-party live product pages are additionally noted below; their content can subsequently change.

| Project | Inspected revision | Commit time, UTC |
| --- | --- | --- |
| Anarlog | [fad3e6dc][rev-an] | 2026-09-30 16:10:06 |
| Amical | [77424505][rev-am] | 2026-09-25 12:50:19 |
| Vexa | [dba990b4][rev-vx] | 2026-09-16 21:03:33 |
| Screenpipe | [0ed8b473][rev-sp] | 2026-09-30 16:09:30 |
| Scriberr | [a353078f][rev-sc] | 2026-09-20 16:42:33 |
| Meet2Notes | [aef6932c][rev-m2n] | 2026-09-29 19:28:18 |
| Open Granola | [9d62163d][rev-og] | 2026-07-23 05:10:48 |
| Parfait | [0b4c1a4e][rev-pa] | 2026-09-09 21:32:42 |
| Transcripted | [85bbcc09][rev-tr] | 2026-09-30 02:02:14 |
| WhisperX | [771b4a14][rev-wx] | 2026-09-26 20:53:59 |
| WhisperLiveKit | [363e4f6d][rev-wlk] | 2026-08-30 20:50:00 |

## Licensing and deployment matrix

Software licenses below do not relicense separately downloaded model weights, codecs, SDKs or bundled dependencies. Source visibility, a paid official build and an OSS code license are different properties.

| Project / scope | Source license checked | Deployment and platform boundary | External processing boundary |
| --- | --- | --- | --- |
| **Sonefold** | MIT application. | Tauri desktop source for Linux/Windows/macOS; a local Linux CUDA test installation is recorded. New-brand public installers and Windows/macOS validation remain incomplete. | Native/local workers and local LLM available; configured API summaries deliberately send text. [README](../../README.md), [validation](2026-09-30-validation.md) |
| **Anarlog community** | MIT outside `enterprise/**`, except nearer third-party licenses. Enterprise paths require a written commercial agreement. [Boundary][an-licensing] | Desktop downloads advertise macOS/Windows/Linux; Tauri source; optional hosted backend and mobile beta. Built-in Soniqo is macOS Apple Silicon in actual platform selection. [README][an-readme], [platform source][an-platform] | Local or selected hosted transcription; local/BYOK/managed intelligence. Sync/sharing/hosted services optional. Do not describe every configuration as offline. [Providers][an-providers] |
| **Amical** | MIT. [License][am-license] | Electron desktop macOS/Windows; no Linux native helper. Onboarding downloads a local model. Android and iOS-beta links are not proof of desktop meeting parity. [README][am-readme] | Local dictation; optional cloud/authentication/telemetry configuration exists. |
| **Vexa** | Apache-2.0. [License][vx-license] | Self-hosted meeting-bot/API stack or paid hosted service. Production Linux; Compose, Lite container and Kubernetes shapes. STT is a separate deploy unit; CPU configuration exists. [Deployment][vx-deploy] | A bot joins the meeting platform; owned inference/storage can be self-hosted. A public Meet/Teams/Zoom bot still contacts that platform: “air-gapped inference” is not an offline public meeting. |
| **Screenpipe, current** | Screenpipe Commercial License. Free personal/noncommercial, nonprofit/education/research use; paid commercial terms and competing-product restrictions. Official builds have their own terms. [License][sp-license] | Desktop application; macOS/Windows and Linux source support documented. Current signed app is paid. [README][sp-readme] | Local capture/history, configurable local/cloud AI, optional paid sync/integrations; analytics default is documented. |
| **Scriberr** | MIT, verified in license text. [License][sc-license] | Self-hosted Go/web application and PWA; Homebrew macOS/Linux; CPU or NVIDIA Docker. Default documented bind is `0.0.0.0`, not Sonefold's opt-in loopback control surface. [README][sc-readme] | Local transcription/diarization; Ollama or OpenAI-compatible chat/summary providers. |
| **Meet2Notes** | MIT. [License][m2n-license] | Python/FastAPI local web workspace; Windows bootstrap, Pinokio or source install. Default loopback bind; Windows WASAPI, macOS virtual/tap input, Linux monitor input. [Audio setup][m2n-audio] | Local engines or explicitly selected LiteLLM providers. MCP passes retrieved content to the chosen client; that client may use cloud AI. |
| **Parfait** | MIT. [License][pa-license] | Source-built menu-bar app; Apple Silicon/macOS 26 and Apple Intelligence. Signed download still marked coming soon in inspected README. [README][pa-readme] | On-device speech; Claude account often supplies notes/assistant by default when installed. Explicit publishing uses the user's GitHub. |
| **Transcripted** | MIT application source; dependency licenses remain separate. [License][tr-license] | Apple Silicon/macOS 26; signed/notarized DMG and Homebrew installation documented. [README][tr-readme] | Local speech/plain files; optional assistant reads via MCP; selected AI client controls off-device processing. |
| **Open Granola** | Apache-2.0. [License][og-license] | Tauri source; all three desktops advertised, but checked loopback/calendar code is incomplete. [Source][og-loopback] | Local-only/Airlock advertised. Do not accept network/physical-deletion guarantees from prose alone. |

### Boundaries and documentation conflicts

**Anarlog's desktop reach exceeds its bundled local speech reach.** Its Soniqo platform module compiles the real implementation only for macOS/aarch64; the other platform implementation returns `UnsupportedPlatform` for model preparation, transcription and diarization. This is a concrete S finding, not an inference from a marketing screenshot. Windows/Linux can use another configured provider, including a compatible self-hosted server, but that does not turn Soniqo into bundled local Linux ASR. [Platform selection][an-platform], [unsupported implementation][an-unsupported], [providers][an-providers]

**Vexa deployment docs have drift.** The inspected README says `make all` pulls release-validated images including the bot, while the inspected deployment guide still instructs `make bot` and warns about incompatible published bot images. This research did not execute either route and therefore does not resolve fresh-install success. An actual comparison must pin an image digest, commands and successful join/transcription evidence. [README][vx-readme], [deployment][vx-deploy]

**Open Granola's source contradicts parts of its completed checklist.** Linux and Windows loopback `start` functions create threads that sleep without delivering samples to their producer. Calendar EventKit and ICS helpers return empty vectors. These inspected paths cannot demonstrate the advertised capture or calendar behavior. This does not establish that every part of the repository is a stub; the Whisper and llama.cpp modules contain actual inference calls. [Loopback][og-loopback], [calendar][og-calendar], [Whisper module][og-transcribe], [LLM module][og-llm]

## Workflow capability matrix

The matrix describes documented/source coverage, not tested success. “Not found” means no active implementation was located during this inspection; it is not a proof that a private/commercial version lacks the feature. Sonefold entries refer to the checkout inspected during research, before any later implementation prompted by this report.

| Product | Capture, speech and speakers | Review and reuse | Integration / organization | Evidence limits |
| --- | --- | --- | --- | --- |
| **Sonefold** | Mic/system capture, imports/reanalysis, native Whisper/Parakeet, Qwen alignment, local speaker workers, experimental continuous NeMo labels; speaker rename/merge and prior-result restoration. | Grounded bounded summaries; six/custom templates; full-content Markdown/TXT/JSON/DOCX/PDF exports. Existing substring transcript search; saved-audio transcript seeking not connected in inspected UI. | Scoped CLI/MCP, authenticated loopback control, signed webhooks. No working calendar, archive chat, library tags or semantic retrieval located. | V: unit/build checks, synthetic CPU/CUDA speech and one short summary smoke; real multi-party accuracy/platform capture unmeasured. [Validation](2026-09-30-validation.md) |
| **Anarlog** | Bot-free device capture; local supported-Mac or selected provider ASR; local voiceprint and batch speaker-refinement paths documented. | Editable notes combined with memo/transcript; chat; local full-text search via Tantivy; Markdown export. | Calendar plugins, CLI/MCP, optional encrypted sync/sharing; mobile beta. | D/S; no independent Linux capture/model or sync-runtime check. [README][an-readme], [provider implementation guide][an-providers] |
| **Amical** | Local active-app-aware dictation, hotkeys, macros; live meeting audio capture P. | Voice-note summaries/tasks in progress. | MCP P; floating capture widget and local model setup D. | Roadmap explicitly distinguishes done/in-progress/planned. [README][am-readme] |
| **Vexa** | Google Meet, Teams, Zoom joining bots and real-time speaker-attributed stream; Jitsi validation still qualified. | Transcripts/recordings through API and web workbench; optional knowledge agents. | REST/WebSocket, webhooks/MCP; owned Markdown workspace and self-hosted agent plane. | D; different unattended/team use case from a personal desktop notebook. [README][vx-readme], [meeting API][vx-meetings] |
| **Screenpipe** | Broader continuous screen/audio/accessibility capture; meeting pipeline and speaker-aware history documented. | Search local history and run AI pipes over allowed slices; meeting summaries. | Local HTTP/SDK/MCP; permissioned pipes and optional paid cloud capabilities. | D; current licensing excludes it from an unrestricted OSS ranking. [README][sp-readme], [license][sp-license] |
| **Scriberr** | Imported audio/video and built-in recorder; WhisperX/Parakeet/Canary and pyannote/Sortformer adapter architecture. | Audio playback follows text; text seeks audio; notes/highlights; transcript chat and summaries. | API, CLI/folder watcher and installable PWA. | D/S for inspected adapters, watcher and chat/speaker handlers; not native system-audio capture parity. [README][sc-readme], [adapter design][sc-engines], [watcher source][sc-watch] |
| **Meet2Notes 0.7** | Separate live/final engines; Nemotron streaming, Parakeet; independent diarization/saved-voice matching; model lifecycle interface. | FTS5 archive search linked to segment/audio; hybrid RAG; grounded assistant scope/quick actions. | Local tags; read-only MCP, durable webhooks, plugins, backup-aware stable updater. | D/S retrieval orchestration; no executed fresh install or meeting benchmark. [Library][m2n-library], [RAG][m2n-rag], [README][m2n-readme] |
| **Parfait** | On-device Apple speech, FluidAudio; calendar/microphone activity detection and speaker naming. | Editable transcript/notes; user-curated follow-up queue; pre-meeting brief/weekly digest workflows. | Local MCP and Claude plugin; GitHub-backed explicit publishing, local HTML export. | D; platform-limited; Claude actions are optional third-party workflows, not all-local intelligence. [README][pa-readme] |
| **Transcripted** | Local meeting/file/dictation capture; reusable speaker profiles; review/confirmation loop. | Plain Markdown capture library; custom dictionary and external assistant evidence access. | One-click agent connection and read-only MCP; call/calendar recording prompts. | D/R; upstream speaker reports have version/protocol caveats. [README][tr-readme], [agent connection][tr-agent], [metrics][tr-metrics] |
| **Open Granola** | Local Whisper inference S; cross-platform system loopback P in checked source. | RAG/pre-meeting/commitment/importer workflows advertised; no end-to-end verification. | Calendar P in checked helpers; portable recipe/template ideas. | Prototype evidence; do not award shipping workflow points from its README alone. [Source][og-loopback], [calendar][og-calendar] |

## Speech libraries are not full competitors

| Engine library | License | What it contributes | What Sonefold still must supply |
| --- | --- | --- | --- |
| **WhisperX** | BSD-2-Clause code. Diarization weights/access agreements and alignment model terms are separate. [License][wx-license], [README][wx-readme] | Batch ASR, VAD, language-specific forced alignment and word-level speaker assignment; CPU/CUDA setup. Current source documents context-aware batching. | Physical capture, durable meetings, interruption recovery, note editor, searches, speaker correction, summaries, exports, permissions and installers. |
| **WhisperLiveKit** | Apache-2.0 code plus upstream components/model terms. [License][wlk-license] | Stateful simultaneous transcription, Sortformer/optional Diart, translation, API compatibility, model profiles, terminology context and browser example UI. [README][wlk-readme] | Meeting lifecycle/database, full note workflow, reversible edits, consentful connections, device integration and supported application releases. |

WhisperX's README throughput claim is not a desktop responsiveness or meeting-accuracy measurement on an RTX 5090 Laptop GPU. WhisperLiveKit publishes H100 English/French audiobook scatter results using six minutes per language; these are useful reproducibility references but do not measure multi-speaker meetings, sustained desktop capture or laptop hardware. Compare complete pipelines using the same local audio, language and protocol. [WhisperX][wx-readme], [WhisperLiveKit benchmark section][wlk-readme]

## Best gaps for Sonefold to close

These priorities are engineering judgments based on existing Sonefold capabilities and peer workflows. They are not a promise of a universal lead. Source inspection found substring SQL search in `database/repositories/transcript.rs` and the scoped automation bridge; no active FTS/semantic index or archive-chat product surface was located. `AudioPlayer.tsx` was empty, `useAudioPlayer.ts` existed, and the inspected meeting transcript panel exposed no playback/seek integration. Calendar icons and example content were not counted as integration. [Transcript repository](../../frontend/src-tauri/src/database/repositories/transcript.rs), [automation](../../frontend/src-tauri/src/extensions/automation.rs), transcript panel (`frontend/src/components/MeetingDetails/TranscriptPanel.tsx`, since removed), audio hook (`frontend/src/hooks/useAudioPlayer.ts`, since removed)

| Priority | User outcome / implementation direction | Peer evidence | Acceptance evidence needed |
| --- | --- | --- | --- |
| **1. Verify the words quickly** | Saved-audio player; click transcript time to seek; current segment highlight; summary source reference opens its originating turn; speaker review plays a short sample. Preserve no-audio/read-only behavior. | Meet2Notes search-to-segment-to-audio; Scriberr text seek/follow-along. [Library][m2n-library], [Scriberr][sc-readme] | On actual retained audio, click a cited line and land at its interval; test late paginated lines, missing audio, reanalysis, keyboard use and correct recording-relative time. Measure time to verify 20 seeded claims. |
| **2. Search a growing archive** | Local full-text index with source IDs, rank/excerpts, date/title filters, exact identifier behavior and segment deep links. Add lightweight tags; keep index changes transactional with edits, deletion and reanalysis. | Anarlog Tantivy; Meet2Notes FTS5/tag/active-version semantics. [Providers][an-providers], [library][m2n-library] | Labeled archive query set: recall@k/MRR for names, decisions, jargon and accented terms; p50/p95 search latency at 100/1,000 meetings; no stale/deleted excerpts. |
| **3. Ask grounded questions** | Meeting-scoped Q&A with bounded retrieved/raw context, validated references, visible scope and explicit insufficient-evidence answers. Then archive questions with time filters and active transcript versions. Reuse scoped local/BYOK providers; clear conversation on scope change. | Meet2Notes separates keyword search from hybrid RAG and budgets attachments first; Scriberr has transcript chat. [RAG][m2n-rag], [chat source][sc-chat] | Human-labeled factual/attribution accuracy, unsupported-answer rate, citation validity and abstention; adversarial instructions embedded in transcripts; latency/VRAM/context-overflow tests. |
| **4. Reduce first-use friction** | Guided native versus advanced-worker setup; verify local model revisions and license requirements; display sizes/device fit; dependable error/recovery paths; tested bundles. Preserve existing reviewed checkpoint selection. | Amical onboarding downloads local models; Meet2Notes exposes model install/load/unload and diagnostics; Anarlog documents supported-provider limits. [Amical][am-readme], [Meet2Notes][m2n-readme], [providers][an-providers] | Fresh-machine test with no source tree; offline second launch; missing/corrupt checkpoint recovery; CUDA/CPU fallback made explicit; time from install to saved first meeting on each supported OS. |
| **5. Prepare and follow through** | Read-only ICS/local-calendar import before OAuth; choose upcoming event to title recording; draft grounded briefs from relevant prior meetings. Persist editable action items with owner/date uncertainty and source turn links; completion state independent of regenerated summaries. | Anarlog calendar layer; Parfait curated follow-ups and brief/digest workflows. [Anarlog][an-readme], [Parfait][pa-readme] | Fixture ICS edge cases/timezones, duplicate events, retention/deletion; action precision, owner/date accuracy and duplicate-commitment rate across meetings. Keep external sending a separately authorized action. |
| **6. Recover and port the notebook** | Backup/restore and meeting import/export that include audio references, edits, speaker mappings, templates and provenance. Publish durable bundle migration behavior; do not confuse document export with full data recovery. | Plain-file ownership in Transcripted/Parfait; Meet2Notes backup-aware stable updater. [Transcripted][tr-readme], [Parfait][pa-readme], [updater][m2n-updating] | Stop/crash/power-loss scenarios, restore into fresh profile, schema migration rollback and canceled reanalysis; byte/record reconciliation and time to restore. |

Persistent voice recognition is a later option, not a required shortcut to good speaker labels. Transcripted exposes explicit confirmations, uncertain-match review and demotion after corrections; Sonefold currently keeps names per meeting. First improve turn correctness and review effort. A future voice-profile feature needs explicit local enrollment/removal, per-profile provenance, confirmation precision, false-merge and fragmentation measurements. A similarity score is not verified identity. [Recognition metrics][tr-metrics]

## What a defensible measured advantage would require

**Run equivalent workflows, not just equivalent model names.** Compare Sonefold, Meet2Notes and Scriberr on the same imported audio and Linux hardware first; compare Anarlog/Transcripted/Parfait on hardware they actually support. Keep bot-based Vexa as a separate unattended capture track. Record product revision, installer/image digest, model revision, quantization, acceleration backend, language, warmup, any GPU-sharing limit and errors.

Use a consented or appropriately licensed test set with quiet/noisy calls, remote compression, in-room meetings, two/many speakers, overlap, silence, German/English, code switching and hour-long sessions. Score **WER/CER and names/numbers separately; DER with declared collar/overlap policy; speaker-attributed errors; cold/warm latency, p95 live lag, revision churn, RTF, peak VRAM/RAM, missing audio/text, summary factual errors, owner/deadline accuracy, search recall, and human correction/verification effort**. Hardware responsiveness and uninterrupted retained audio are product measures alongside transcription quality.

Existing Sonefold validation establishes build/unit/integration compatibility and short synthetic CPU/CUDA inference. It does not establish representative WER/DER, long-meeting completeness or Windows/macOS capture reliability. Its documented empty short German live output must remain in the scorecard rather than being dropped as an inconvenient fixture. [Validation](2026-09-30-validation.md)

There are useful upstream evaluation precedents, with limits:

- **Meet2Notes** implements separate-process cold CPU/CUDA evaluators and append-only timing/error ledgers. Retaining skipped/unsupported runs and distinguishing load from inference are good practices; recorded runtime/transcript outputs alone do not establish WER/DER without aligned reference truth. [ASR evaluator source][m2n-asr-eval], [diarization evaluator source][m2n-diar-eval]
- **Transcripted** publishes an AMI four-meeting speaker baseline with DER collar/overlap settings and explicit cross-meeting fragmentation/false merges. Its prominent provenance note says the matcher later changed and the old numbers are not revalidated/reproducible as current results. Use this as a reporting standard, not as a present competitor score to beat. [Baseline report][tr-baseline]
- **WhisperLiveKit** makes small H100 audiobook speed/accuracy results reproducible and explicitly requests measurements on other hardware. That is engine evidence under a narrow protocol. [Benchmark method][wlk-readme]

A supportable claim has a bounded form: “On the published German/English imported-meeting set and this Linux hardware, Sonefold reduced median verification time by X while meeting Y accuracy and recovery criteria.” “Best open-source meeting app,” “more accurate than every competitor” and “enterprise ready” are unsupported without substantially broader evidence.

## Inspection record and source index

Read-only checks completed: local `AGENTS.md`, README, product/research/validation notes; batched primary-source web discovery; unauthenticated public revision queries; SHA-pinned README/license retrieval for 12 projects; full repository-tree discovery for eight projects; selected speech-platform, retrieval, capture/calendar, adapter, watcher/chat and benchmark-report files. No product code, `AGENTS.md`, license or Git history was modified by this research task. The only repository output is this report. Public source files and query metadata were staged in `/tmp` to make exact citations and conflicts reviewable.

Live sources checked September 30: the official Screenpipe June license-change notice. [Screenpipe announcement](https://screenpipe.com/blog/screenpipe-v2-19-new-open-source-license-changelog)

[rev-an]: https://github.com/fastrepl/anarlog/commit/fad3e6dcde39a6bfd81764099d47c9081ced07bb
[rev-am]: https://github.com/amicalhq/amical/commit/77424505c98eed38fd3e000e20a3858141fa4051
[rev-vx]: https://github.com/Vexa-ai/vexa/commit/dba990b413bd0f888b02d46a24f802db492addbb
[rev-sp]: https://github.com/screenpipe/screenpipe/commit/0ed8b473bfa512f868f87e3fe3f085d9c275dff3
[rev-sc]: https://github.com/rishikanthc/Scriberr/commit/a353078fd96b8aca4002681813524b7397c90df1
[rev-m2n]: https://github.com/estebanstifli/Meet2Notes/commit/aef6932c33db38caa3910420024137006308ec9d
[rev-og]: https://github.com/anshuman-pandey/open-granola/commit/9d62163dd00011bce843fa5b97fe5e4ce3e1943f
[rev-pa]: https://github.com/conrad-vanl/parfait/commit/0b4c1a4e9357442ad27cc28980a877220708d294
[rev-tr]: https://github.com/r3dbars/transcripted/commit/85bbcc09380f6de16e73119efb672e5e6a628f72
[rev-wx]: https://github.com/m-bain/whisperX/commit/771b4a14a9486f8fd5aef18ef49e35d639523dd3
[rev-wlk]: https://github.com/QuentinFuxa/WhisperLiveKit/commit/363e4f6d029694d9c81ae548beddd9d3c88a3637
[an-readme]: https://github.com/fastrepl/anarlog/blob/fad3e6dcde39a6bfd81764099d47c9081ced07bb/README.md
[an-licensing]: https://github.com/fastrepl/anarlog/blob/fad3e6dcde39a6bfd81764099d47c9081ced07bb/LICENSING.md
[an-providers]: https://github.com/fastrepl/anarlog/blob/fad3e6dcde39a6bfd81764099d47c9081ced07bb/docs/models-and-providers.mdx
[an-platform]: https://github.com/fastrepl/anarlog/blob/fad3e6dcde39a6bfd81764099d47c9081ced07bb/crates/transcribe-soniqo/src/platform/mod.rs
[an-unsupported]: https://github.com/fastrepl/anarlog/blob/fad3e6dcde39a6bfd81764099d47c9081ced07bb/crates/transcribe-soniqo/src/platform/unsupported.rs
[am-readme]: https://github.com/amicalhq/amical/blob/77424505c98eed38fd3e000e20a3858141fa4051/README.md
[am-license]: https://github.com/amicalhq/amical/blob/77424505c98eed38fd3e000e20a3858141fa4051/LICENSE
[vx-readme]: https://github.com/Vexa-ai/vexa/blob/dba990b413bd0f888b02d46a24f802db492addbb/README.md
[vx-license]: https://github.com/Vexa-ai/vexa/blob/dba990b413bd0f888b02d46a24f802db492addbb/LICENSE
[vx-deploy]: https://github.com/Vexa-ai/vexa/blob/dba990b413bd0f888b02d46a24f802db492addbb/docs/docs/deployment.mdx
[vx-meetings]: https://github.com/Vexa-ai/vexa/blob/dba990b413bd0f888b02d46a24f802db492addbb/docs/docs/api/meetings.mdx
[sp-readme]: https://github.com/screenpipe/screenpipe/blob/0ed8b473bfa512f868f87e3fe3f085d9c275dff3/README.md
[sp-license]: https://github.com/screenpipe/screenpipe/blob/0ed8b473bfa512f868f87e3fe3f085d9c275dff3/LICENSE.md
[sc-readme]: https://github.com/rishikanthc/Scriberr/blob/a353078fd96b8aca4002681813524b7397c90df1/README.md
[sc-license]: https://github.com/rishikanthc/Scriberr/blob/a353078fd96b8aca4002681813524b7397c90df1/LICENSE
[sc-engines]: https://github.com/rishikanthc/Scriberr/blob/a353078fd96b8aca4002681813524b7397c90df1/internal/transcription/README.md
[sc-watch]: https://github.com/rishikanthc/Scriberr/blob/a353078fd96b8aca4002681813524b7397c90df1/internal/cli/watch.go
[sc-chat]: https://github.com/rishikanthc/Scriberr/blob/a353078fd96b8aca4002681813524b7397c90df1/internal/api/chat_handlers.go
[m2n-readme]: https://github.com/estebanstifli/Meet2Notes/blob/aef6932c33db38caa3910420024137006308ec9d/README.md
[m2n-license]: https://github.com/estebanstifli/Meet2Notes/blob/aef6932c33db38caa3910420024137006308ec9d/LICENSE
[m2n-library]: https://github.com/estebanstifli/Meet2Notes/blob/aef6932c33db38caa3910420024137006308ec9d/docs/meeting-library.md
[m2n-rag]: https://github.com/estebanstifli/Meet2Notes/blob/aef6932c33db38caa3910420024137006308ec9d/docs/rag-and-mcp.md
[m2n-audio]: https://github.com/estebanstifli/Meet2Notes/blob/aef6932c33db38caa3910420024137006308ec9d/docs/audio-capture.md
[m2n-updating]: https://github.com/estebanstifli/Meet2Notes/blob/aef6932c33db38caa3910420024137006308ec9d/docs/updating.md
[m2n-asr-eval]: https://github.com/estebanstifli/Meet2Notes/blob/aef6932c33db38caa3910420024137006308ec9d/scripts/evaluate_asr.py
[m2n-diar-eval]: https://github.com/estebanstifli/Meet2Notes/blob/aef6932c33db38caa3910420024137006308ec9d/scripts/evaluate_diarization.py
[pa-readme]: https://github.com/conrad-vanl/parfait/blob/0b4c1a4e9357442ad27cc28980a877220708d294/README.md
[pa-license]: https://github.com/conrad-vanl/parfait/blob/0b4c1a4e9357442ad27cc28980a877220708d294/LICENSE
[tr-readme]: https://github.com/r3dbars/transcripted/blob/85bbcc09380f6de16e73119efb672e5e6a628f72/README.md
[tr-license]: https://github.com/r3dbars/transcripted/blob/85bbcc09380f6de16e73119efb672e5e6a628f72/LICENSE
[tr-agent]: https://github.com/r3dbars/transcripted/blob/85bbcc09380f6de16e73119efb672e5e6a628f72/docs/agent-connect.md
[tr-metrics]: https://github.com/r3dbars/transcripted/blob/85bbcc09380f6de16e73119efb672e5e6a628f72/docs/speaker-recognition-metrics.md
[tr-baseline]: https://github.com/r3dbars/transcripted/blob/85bbcc09380f6de16e73119efb672e5e6a628f72/Tools/SpeakerEvalHarness/BASELINE_REPORT.md
[og-license]: https://github.com/anshuman-pandey/open-granola/blob/9d62163dd00011bce843fa5b97fe5e4ce3e1943f/LICENSE
[og-loopback]: https://github.com/anshuman-pandey/open-granola/blob/9d62163dd00011bce843fa5b97fe5e4ce3e1943f/src-tauri/src/audio/loopback.rs
[og-calendar]: https://github.com/anshuman-pandey/open-granola/blob/9d62163dd00011bce843fa5b97fe5e4ce3e1943f/src-tauri/src/calendar.rs
[og-transcribe]: https://github.com/anshuman-pandey/open-granola/blob/9d62163dd00011bce843fa5b97fe5e4ce3e1943f/src-tauri/src/transcribe.rs
[og-llm]: https://github.com/anshuman-pandey/open-granola/blob/9d62163dd00011bce843fa5b97fe5e4ce3e1943f/src-tauri/src/llm.rs
[wx-readme]: https://github.com/m-bain/whisperX/blob/771b4a14a9486f8fd5aef18ef49e35d639523dd3/README.md
[wx-license]: https://github.com/m-bain/whisperX/blob/771b4a14a9486f8fd5aef18ef49e35d639523dd3/LICENSE
[wlk-readme]: https://github.com/QuentinFuxa/WhisperLiveKit/blob/363e4f6d029694d9c81ae548beddd9d3c88a3637/README.md
[wlk-license]: https://github.com/QuentinFuxa/WhisperLiveKit/blob/363e4f6d029694d9c81ae548beddd9d3c88a3637/LICENSE
