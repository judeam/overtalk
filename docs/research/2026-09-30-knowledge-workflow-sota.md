# Local meeting knowledge workflows: research and implementation designs

Research cutoff and source access: **30 September 2026**. Scope: Sonefold's Rust/Tauri desktop application, a private meeting collection, and local inference on an RTX 5090 Laptop GPU. This is primary-source research and engineering design, not a measured ranking on real meetings. No recordings were uploaded, model weights downloaded or GPU inference run during this review.

The most useful next step is a durable evidence layer: SQLite full-text search, immutable transcript references, grounded questions across meetings, and reviewed action/decision records. Keep lexical search usable without Python, credentials, or downloaded AI models. Add small multilingual embeddings and reranking as an explicit optional capability. First preserve database/recording state and make verified backups; evidence features cannot compensate for lost source material.

## 1. Repository findings and scope boundary

These observations describe the initial source inspection on the research date, before concurrent implementation work. They are a baseline for the designs below, not a claim that later code still has every gap.

| Area | Initially observed behavior | Concrete extension |
| --- | --- | --- |
| Capture and recovery | Audio import, interrupted-recording recovery, 30-second audio checkpoints and transcript snapshots already exist. | Durable checkpoint manifests, non-destructive database recovery, verified reconstruction and explicit loss bounds. |
| Search | Native `search_transcripts` and companion search use escaped literal `LIKE`, ordered by recent meeting; no semantic index found. | Native FTS5 over transcripts, meeting titles, edited notes and summaries, with scoped ranked results and safe literal-query construction. |
| Summaries | Bounded hierarchical evidence extraction/reduction and source-reference validation already exist. | Persist intermediate structured evidence, cache unchanged leaves, link references to immutable transcript revisions and review action/decision records. |
| Citation identity | `processor.rs::with_source_ids` enumerates input lines into `[S1]`, `[S2]`, etc.; `validate_source_references` checks numeric range. | Store a real source manifest. A valid number alone does not identify the persisted turn or establish factual support. |
| Actions | Transcript model contains optional `action_items` text; no durable reviewed action lifecycle found. | Separate candidate extraction from accepted tasks, status updates, owner review, due-date review and supporting evidence. |
| Calendar | No current ICS/CalDAV workflow found. | File-based ICS import and task export first; explicit CalDAV configuration later. |
| Backup | Transcript revisions and temporary summary regeneration backup exist; no complete portable backup/restore workflow found. | Consistent database snapshot plus recordings/templates/revisions, integrity manifest, optional interoperable encryption, staged verified restore. |
| Diagnostics | Audio capability logging, onboarding/model checks and optional speech-worker health checks exist. | A compact fresh-install report separating presence, compatibility and actual operation, with private-data redaction. |
| Integrations and export | Scoped CLI/MCP, authenticated loopback API, signed webhooks, edited summary export and complete transcript export already exist. | Reuse these grants and native services; do not duplicate an automation bridge or document renderer. |

Local evidence: [README](../../README.md), [workflow guide](../ADVANCED_FEATURES.md), [integration contract](../../integrations/README.md), [native transcript repository](../../frontend/src-tauri/src/database/repositories/transcript.rs), [summary processor](../../frontend/src-tauri/src/summary/processor.rs), [summary cache/service](../../frontend/src-tauri/src/summary/service.rs), [audio checkpoints](../../frontend/src-tauri/src/audio/incremental_saver.rs), [database manager](../../frontend/src-tauri/src/database/manager.rs), [speech transcript backup migration](../../frontend/src-tauri/migrations/20260930010000_speech_backups.sql), [audio diagnostics](../../frontend/src-tauri/src/audio/diagnostics.rs). The archived Python backend is not the implementation target.

## 2. Retrieval: native lexical default, optional hybrid extension

SQLite FTS5 already supplies tokenization, ranked BM25 results, snippets and external-content indexing. Its BM25 convention is **lower score is better**. External-content triggers maintain subsequent mutations but do not index rows that existed before migration; explicitly rebuild the index. `unicode61` has defined Unicode tokenization/case handling, not language-aware morphological understanding. Trigram search is a separate useful substring mode. [SQLite FTS5 documentation](https://www.sqlite.org/fts5.html)

Recommended application design:

1. Normalize searchable content into a source table with an integer row identifier, meeting ID, source kind, revision, stable source ID, title and text. Keep actual source records authoritative. Index committed transcript turns, final summaries and user notes; avoid indexing every provisional live hypothesis.
2. Make indexing transactional with source changes. Cover insert, update and delete, including meeting rename, speaker edits, summary replacement and transcript restoration. Run an initial rebuild and expose a repair/rebuild operation that only touches derived indexes.
3. Keep a literal search box as the default. Construct quoted FTS terms from user text and bind the resulting expression. SQL parameter binding does not neutralize FTS `MATCH` grammar. Expose advanced operators only as a separate documented mode. Bound query length/token count and handle empty/punctuation-only queries.
4. Apply meeting/date/source filters inside candidate retrieval. Return persisted source IDs, excerpts, speaker/time metadata and score/rank. Escape snippet text before rendering; FTS highlight markers must not become arbitrary HTML.
5. Deduplicate repeated titles and overlapping passages. A title hit helps locate a meeting; a transcript turn supplies evidence. Show whether a result came from a user note, generated summary or transcript.

For hybrid search, retrieve independent ranked lexical and dense candidate lists, union stable passage IDs, and use reciprocal rank fusion: `sum(1 / (k + rank))`, with `k=60` as an initial tuning choice. This avoids mixing incomparable BM25 and cosine scales. RRF is a classic primary-source method, not a guarantee that every blend improves this corpus. [Cormack, Clarke and Büttcher, SIGIR 2009](https://cormack.uwaterloo.ca/cormacksigir09-rrf.pdf)

Start with approximately 100 lexical and 100 dense candidates, cap the fused set, and rerank only 20–50 passages. Those bounds are design defaults to benchmark. Build semantic passages from adjacent complete turns, initially around 250–500 model tokens, with modest overlap and preserved turn membership. Expand a selected passage by neighboring turns before generation so that short answers, pronouns and agreement/disagreement remain interpretable. Never cite the embedding vector or a lossy passage summary as though it were the original speech.

For a small personal collection, an exact CPU vector scan is a credible first implementation. At 50,000 passages, 512-dimensional float32 vectors occupy about 102 MB before metadata; 1024 dimensions double that. This is arithmetic, not a latency measurement. Cache and scan contiguous vectors rather than decoding SQLite blobs one at a time for every query. Consider ANN only after measured corpus/query latency justifies it.

`sqlite-vec` provides a single-file SQLite extension with `vec0` vector tables and metadata/partition filtering. At this cutoff the release listing identifies **0.1.9** as stable; DiskANN/other ANN additions are in **0.1.10 alpha**. Pin and test the extension, including delete and restore behavior. A new vector extension does not need to become a prerequisite for the default search. [Official sqlite-vec table documentation](https://alexgarcia.xyz/sqlite-vec/features/vec0.html), [official releases](https://github.com/asg017/sqlite-vec/releases)

## 3. Local embedding and reranking candidates

There is no primary-source evidence that one current model is best on Sonefold's German/English, noisy ASR, proper names and implicit references. Published MTEB/MMTEB or general retrieval results support a shortlist; they do not establish meeting accuracy or runtime compatibility.

| Candidate | Primary-source properties | Fit for this application |
| --- | --- | --- |
| **Qwen3-Embedding-0.6B** | Apache-2.0; 100+ languages; 32K input; 1024 dimensions with adjustable 32–1024 dimensions. | Recommended optional baseline. Start at 512 or 1024 dimensions and compare on real retrieval queries. |
| Qwen3-Embedding-4B/8B | Same family; larger embedding dimensions and published multilingual retrieval results. | Optional higher-quality evaluation, with materially larger runtime footprint. Keep 0.6B as the portable tier. |
| **Qwen3-Reranker-0.6B** | Apache-2.0; multilingual; 32K input; causal relevance judge. | Recommended small reranker after correct first-party scoring is verified. |
| BGE-M3 | MIT; multilingual; 8192-token input; dense, sparse and multivector representations. | Useful mature alternative. Dense-only integration is simpler than exposing all three modes. |
| BGE-reranker-v2-m3 | Apache-2.0; multilingual cross-encoder with a conventional sequence-classification interface. | Practical alternate reranker and a simpler comparison for native inference runtimes. |
| EmbeddingGemma-300M | 300M; 100+ languages; 2048-token input; 768 dimensions with 512/256/128 truncation and renormalization. **Gemma license**, gated first-party HF weight access. | Strong small-device candidate for users who accept its separate terms; does not satisfy ungated installation through that HF repository. |
| Jina embeddings v5 text small/nano | February 2026 family; small is 677M, 119+ languages, 32K, 1024 dimensions. **CC-BY-NC-4.0**, task adapters/custom code in standard loading example. | Research comparison, not an unrestricted default for an MIT desktop application. Code and model-weight licenses differ. |
| zerank-2-reranker | Apache-2.0; 4B Qwen3-based; 32K; published conversational/domain results. May 2026 score API changed to raw Yes logits. | Optional workstation comparison. Verify German performance and avoid sharing score thresholds with Qwen/BGE. |

Sources: [Qwen embedding card](https://huggingface.co/Qwen/Qwen3-Embedding-0.6B), [Qwen reranker card](https://huggingface.co/Qwen/Qwen3-Reranker-0.6B), [Qwen technical report](https://arxiv.org/abs/2506.05176), [BGE-M3 card](https://huggingface.co/BAAI/bge-m3), [BGE reranker card](https://huggingface.co/BAAI/bge-reranker-v2-m3), [EmbeddingGemma card and weight-access terms](https://huggingface.co/google/embeddinggemma-300m), [Jina v5 card/license](https://huggingface.co/jinaai/jina-embeddings-v5-text-small), [zerank-2 card/API change](https://huggingface.co/zeroentropy/zerank-2-reranker).

### Immutable Qwen revisions and offline loading

The first-party Hugging Face model API reports the following ungated revisions, both modified before the cutoff:

| Model | Revision | Last modification |
| --- | --- | --- |
| `Qwen/Qwen3-Embedding-0.6B` | `97b0c614be4d77ee51c0cef4e5f07c00f9eb65b3` | 2026-04-20 |
| `Qwen/Qwen3-Reranker-0.6B` | `e61197ed45024b0ed8a2d74b80b4d909f1255473` | 2026-04-16 |

[Embedding model API](https://huggingface.co/api/models/Qwen/Qwen3-Embedding-0.6B), [reranker model API](https://huggingface.co/api/models/Qwen/Qwen3-Reranker-0.6B). These are reviewed candidate pins, not a claim that weights were installed or inference passed. Record per-file checksums in the model-install manifest as well as the upstream revision.

For an optional Python worker, use native Qwen3 support in Transformers (`>=4.51.0` is the first-party minimum), an explicitly pinned compatible PyTorch build, local checkpoint paths, `local_files_only=True`, and `trust_remote_code=False`. Require already-prepared local files during application inference. `HF_HUB_OFFLINE=1` protects against accidental hub access; an explicit model-install action can download an ungated snapshot without hosted API credentials. No FlashAttention installation is required for a first portable baseline. [Transformers offline operation](https://huggingface.co/docs/transformers/installation), [Transformers Qwen3 API](https://huggingface.co/docs/transformers/model_doc/qwen3)

Keep inference in a bounded worker process. Do not make search import the ASR/NeMo stack. Share the process-launch/cancellation machinery where appropriate, but report independent model readiness. With 0.6B parameters, BF16 weights are approximately 1.2 GB per model before activations/runtime buffers; the actual GPU fit must be measured with concurrent ASR, local generation and display use. Load sequentially or unload idle models initially. When the GPU also drives the desktop, run heavy CUDA worker/evaluation commands under a GPU-sharing launcher ([GPU notes](../GPU_ACCELERATION.md#sharing-the-gpu-with-the-desktop)); inspect actual free VRAM before selecting batch size.

### Correct embedding and reranking semantics

Embedding requires `AutoModel`, inference/eval mode, attention masks, **last attended token pooling** and L2 normalization. Query text carries an English retrieval instruction; document text does not receive the query instruction. The pinned pooling configuration explicitly enables last-token pooling. If using left padding, the last sequence position is the final attended token; `attention_mask.sum()-1` alone is wrong for a left-padded sequence. An implementation can instead find the greatest attended position for each row, which works with either padding direction. When shortening a Matryoshka vector, truncate first and normalize again. [Pinned Qwen pooling configuration](https://huggingface.co/Qwen/Qwen3-Embedding-0.6B/raw/97b0c614be4d77ee51c0cef4e5f07c00f9eb65b3/1_Pooling/config.json), [first-party Qwen implementation examples](https://github.com/QwenLM/Qwen3-Embedding)

The reranker is **not** a generic text-classification head. The first-party Transformers route loads `AutoModelForCausalLM`, supplies the prescribed judge template, and reads the final-token logits for `yes` and `no`. Ranking can use their difference; two-token softmax produces a 0–1 score, which is not calibrated factual confidence. Preserve the assistant suffix and reserved token budget when truncating. Use the reviewed template file, whose input roles are `system`, `query` and `document`, rather than treating ordinary chat formatting as equivalent. [Pinned reranker template](https://huggingface.co/Qwen/Qwen3-Reranker-0.6B/raw/e61197ed45024b0ed8a2d74b80b4d909f1255473/chat_template.jinja)

Modern SentenceTransformers exposes Qwen reranking through `CrossEncoder`, but its compatible loader/module version must be pinned and verified; older versions cannot be assumed to interpret the current checkpoint. The first-party card distinguishes raw logit difference from sigmoid output. In a first implementation, direct Transformers inference makes the scoring path explicit. [Qwen reranking API](https://huggingface.co/Qwen/Qwen3-Reranker-0.6B)

The llama.cpp server documents `/v1/embeddings` and reranking endpoints, but endpoint availability alone is insufficient. Its source has architecture-specific Qwen conversion, and a July 2026 issue reports effectively zero reranking scores for a Qwen-derived GGUF; the issue is unconfirmed and closed as stale, not proof that every Qwen reranker fails. Test the pinned converter/model/runtime against an obviously relevant and irrelevant passage before integrating it. [Official llama.cpp API](https://github.com/ggml-org/llama.cpp/blob/master/tools/server/README.md), [Qwen conversion source](https://github.com/ggml-org/llama.cpp/blob/master/conversion/qwen.py), [runtime issue and exact environment](https://github.com/ggml-org/llama.cpp/issues/25447)

An embedding profile must include model ID/revision, tokenizer, pooling, instruction, dimension, normalization and quantization. Never compare vectors from changed profiles; rebuild derived indexes in the background. Tests should cover batch-versus-single consistency, padding, malformed/empty/nonfinite output, dimension mismatches, cancellation and stale results after transcript editing.

## 4. Grounded questions across meetings

The ALCE benchmark separates answer correctness from citation quality; retrieval plus inline citation markers does not guarantee support. The July 2026 evidence-generation survey reviews citation, attribution and quotation as related but distinct mechanisms. These findings support treating citations as a native evidence contract rather than decorating generated prose. [ALCE, EMNLP 2023](https://aclanthology.org/2023.emnlp-main.398/), [evidence-based generation survey, ACL 2026](https://aclanthology.org/2026.acl-long.1430/)

Proposed source identity:

```text
meeting_id + transcript_revision + segment_id + text_sha256
          + optional audio_start_ms/audio_end_ms
```

Create a server-side source manifest for each answer. Assign short request-local source handles, map them to immutable identities, and let the model emit only allowlisted handles. Persist the manifest with the answer. Server code supplies titles, dates, speaker names and playback locations; the model does not generate arbitrary URLs, paths or timestamps.

Use a staged answer contract: retrieve → select quoted evidence → generate small factual claims with evidence handles → validate IDs/revisions/quote spans → render links. A selected quote should be an exact source span, with only documented Unicode/whitespace normalization if necessary. Quote existence still does not prove that the claim follows from it. Reject or flag missing support, include uncertainty, and offer “insufficient evidence in the selected meetings.” Do not claim confidence from a reranker score. Fine-grained quoted evidence is an active research direction. [FRONT grounded-citation paper](https://arxiv.org/abs/2408.04568)

For contradictions, retrieve both statements and preserve dates: “The August review proposed X; the September review agreed Y.” For numerical, owner or due-date claims, use stricter review than for general themes. Ask “What changed?” through date-aware retrieval and explicit comparisons. Ask “Which accepted actions are overdue?” through deterministic queries over reviewed task records, then optionally summarize that result. A top-k retriever cannot establish an exhaustive count over the collection.

Transcript reanalysis, restoration and edits can invalidate saved answers. Show their original revision or mark evidence stale; never silently retarget an old `[S12]` to a different line. Include archived turns in portable backups. Audio timestamps prove where the quoted transcript is located, not whether ASR transcribed speech correctly. Playback and speaker review remain the final inspection path.

Keep user conversation history bounded and separate from retrieved evidence. Resolve follow-up references through the prior selected meeting/filter context, then retrieve anew. Treat transcript and calendar descriptions as untrusted evidence. A chat prompt containing a request to email notes cannot grant an integration scope. The C²-Cite research explicitly studies citation-marker/context alignment, but changing model internals and training a citation router is a later experiment, not necessary for a useful initial product. [C²-Cite, WSDM 2026](https://arxiv.org/abs/2602.00004)

## 5. Structured action extraction and decision tracking

QMSum motivates selecting relevant meeting spans before summarization; ExplainMeetSum adds human-annotated evidence sentences. Dialogue factuality research shows that fluent summaries can still make attribution/coreference errors. This argues for evidence-backed records and a correction workflow, not immediately promoting every generated bullet into a task. [QMSum](https://aclanthology.org/2021.naacl-main.472/), [ExplainMeetSum](https://aclanthology.org/2023.acl-long.731/), [fine-grained dialogue factuality](https://aclanthology.org/2023.acl-long.377/)

Extract bounded structured candidates from neighboring turns with a schema and deterministic validation. Existing local generation is adequate infrastructure: Ollama supports JSON schema output, and llama.cpp documents schema-constrained response formats. Capability-detect each provider rather than assuming every compatible endpoint implements these controls. [Ollama structured outputs](https://docs.ollama.com/capabilities/structured-outputs)

Application record design:

| Field group | Purpose |
| --- | --- |
| Identity | Candidate ID, accepted action/decision ID, meeting ID and extraction version. |
| Meaning | Action text; category `commitment`, `request`, `proposal`, `decision` or `open_question`; preserve original wording. |
| People | Explicit owner text and optional reviewed person link; anonymous speaker IDs are per meeting, not automatically cross-meeting identities. |
| Time | Original due phrase, optional interpreted date, timezone/anchor, ambiguity and review state. |
| Evidence | Immutable source handles and short verified quote spans; allow multiple turns for request/acceptance. |
| Review | Candidate/accepted/rejected state, reviewer changes and timestamps. |
| Lifecycle | Open/in progress/done/cancelled; updates separate from model re-extraction. |
| Decisions | Proposed/agreed/superseded, supporting sources and optional reviewed `supersedes` link. |

An explicit “I'll send the revised draft on Friday” can become a candidate. “Someone should send the draft” leaves the owner unstated. “Could we launch Friday?” is a proposal until agreement is evidenced. Missing owner/date remains null. A relative Friday requires a known meeting date and local timezone; ambiguous dates are reviewed before reminders or calendar output. Naming a speaker in one meeting does not establish that similarly named people or voices in another meeting are identical.

Re-extraction must preserve user-accepted tasks and their status. Suggest duplicates using evidence/text similarity but merge only with a traceable rule or review; never create a duplicate task whenever a summary language changes. Track “superseded by” as an explicit relationship, not “latest meeting wins.” User edits remain first-class data with their own revision. A local overview of reviewed actions and decision history is more reliable than inferring task completion from silence in later meetings.

## 6. Incremental and resumable summaries

The existing pipeline already chunks and recursively reduces evidence. The next design is **persistent leaf caching**, not another map-reduce rewrite. Hash the stable transcript range plus revision, model/profile, extraction prompt/schema and language. Persist the leaf ledger, its source manifest and completion status. Appends generate only new leaves; edits invalidate the affected leaf and its ancestor reductions; speaker-name changes invalidate display/attribution dependencies. Keep deterministic chunk boundaries where possible so a small edit does not churn the whole tree.

Separate extracted evidence from the chosen report template. Facts/actions/decisions can feed different templates and languages without inventing a second source of truth. Persist cancellation progress and allow resume after restart. Treat generated text as a replaceable projection; retain the evidence and original transcript. The existing English-summary cache fingerprints the full source/template/provider parameters but is narrower than a resumable hierarchy. [Existing cache implementation](../../frontend/src-tauri/src/summary/service.rs)

For live summaries, process only finalized transcript prefixes, with a revision cursor. Provisional speaker/text hypotheses may change; show the cursor/time range and a pending state. Periodically reconcile accepted evidence against completed transcription. A live summary must not imply completeness when capture continues or the speech worker fell behind. Online incremental meeting summarization remains a distinct evaluation problem. [Incremental temporal meeting summarization research](https://aclanthology.org/2021.newsum-1.11/)

An append-only evidence ledger avoids repeatedly asking a model to “update the previous summary,” a process that can carry forward early mistakes and erase dissent. Model outputs can propose amendments; the provenance layer decides which transcript revision they belong to. Measure cache hits, recomputation, time to first useful update and factual drift on long meetings.

## 7. Calendar: ICS first, CalDAV optional

ICS file import and VTODO/VEVENT export are fully offline and need no hosted account. Preserve the source calendar and canonical event identity. A read-only “Upcoming meetings” view can suggest titles and link a recording to a chosen occurrence; automatic recording is a separate product decision.

RFC 5545 defines recurrence identity and date/time semantics. `RECURRENCE-ID` identifies the original occurrence even if its actual start moved. Recurrence combines `DTSTART`, `RRULE`, `RDATE` and exclusions. DATE values, floating times and timezone-qualified times have distinct meanings; event `DTEND` is exclusive. UTF-8 line folding is an octet concern, and escaped TEXT/newline characters need a real parser. [iCalendar specification](https://www.rfc-editor.org/rfc/rfc5545)

Implement staged parsing followed by validation and import, with these application constraints:

- Use `(calendar_source, UID, RECURRENCE-ID)` for idempotent identity; store raw input as a recoverable source record and `SEQUENCE`/revision metadata for updates.
- Preserve DATE-only events as dates, not midnight UTC. Preserve floating local times until a user calendar timezone is chosen. Retain timezone identity alongside UTC instants for zoned events.
- Handle cancelled events and detached overridden instances; an override can move an occurrence or change its duration. Avoid counting the master and exception twice.
- Bound recurrence expansion by date horizon, occurrence count, input size and compute budget. Do not expand an infinite rule into a giant persistent list.
- Keep unsupported recurrence/VTIMEZONE features visible. Importing only `DTSTART` from a weekly event without a limitation indicator is incorrect behavior.
- For reviewed actions, export VTODO where supported. Unknown due dates remain absent; DATE deadlines remain DATE. Do not create an invented meeting or send invitations to guessed attendee addresses.

Rust `ical` 0.11.0 exposes `IcalParser`, property parsing and unfolding; it is not by itself a recurrence/calendar engine. Rust `rrule` 0.14.0 provides `RRuleSet`, range filtering and bounded `all(limit)`, uses `chrono-tz`, and is MIT/Apache-2.0. Its supported timezone set is IANA-based; custom VTIMEZONE interpretation and RECURRENCE-ID exception application remain integration responsibilities. Keep validation limits enabled; arbitrary recurrence rules can still consume CPU, so process off the UI thread with bounded execution. [ical API](https://docs.rs/ical/0.11.0/ical/), [rrule API](https://docs.rs/rrule/0.14.0/rrule/), [rrule license/limits](https://docs.rs/crate/rrule/0.14.0), [rrule security notes](https://github.com/fmeringdal/rust-rrule/blob/main/SECURITY.md)

`icalendar` is useful for Rust event/task generation and supports escaping/folding in the library. Verify output with a separate parser and representative calendar clients. [Official icalendar source](https://github.com/hoodie/icalendar)

CalDAV can target a self-hosted server as well as hosted providers. The core workflow is discovery, bounded `calendar-query`, ETag comparison, `calendar-multiget`, and conditional writes (`If-Match`/`If-None-Match`) to avoid overwriting remote edits. Use WebDAV `sync-collection` tokens when advertised; tokens may expire, requiring reconciliation. Do not assume every server supports sync tokens, VTODO or scheduling. Keep calendar credentials outside portable backups by default. [CalDAV RFC 4791](https://www.rfc-editor.org/rfc/rfc4791), [WebDAV collection synchronization RFC 6578](https://www.rfc-editor.org/rfc/rfc6578)

Start with file import and reviewed task export. A calendar description or imported URL is not authorization to contact an arbitrary endpoint or send meeting text. A configured URL subscription/CalDAV connection should have its own explicit enablement and network behavior.

## 8. Portable backup and encryption

### Consistent snapshot before encryption

Copying a live `.sqlite` file alone is insufficient. SQLite's WAL is part of persistent database state; separating it can lose committed transactions. SQLite provides the Online Backup API and `VACUUM INTO` for a consistent live snapshot. `VACUUM INTO` produces a compact snapshot but its destination must be new/empty, interrupted output is incomplete, and the output should be durably synced before publication. [SQLite WAL](https://www.sqlite.org/wal.html), [Online Backup API](https://www.sqlite.org/backup.html), [VACUUM INTO behavior](https://sqlite.org/lang_vacuum.html)

Use a new private staging directory. Acquire a short application backup lease that prevents file deletion/replacement while recording references are collected. Capture a database snapshot, then include only sealed recordings referenced by that snapshot. Checkpoint an active recording or explicitly omit its unsealed tail and record the cutoff. Hash every included file and reject missing references; a consistent SQLite snapshot alone does not make separate audio files consistent.

The versioned backup manifest should include app/format/schema versions, meeting IDs, transcript revisions, file sizes/hashes, recording content IDs, template definitions and exclusion choices. Include actual recordings when requested, because imported paths may be outside the app directory. Use archive-relative generated paths rather than restoring arbitrary absolute paths. Exclude credentials, integration tokens, webhook secrets, ephemeral locks, derived caches and model weights by default. A sanitized data snapshot needs an explicit allowlist/redaction of credential-bearing tables; hashing or encryption does not remove those secrets.

Restore to a private temporary location, validate archive size/count/path bounds, reject traversal and link entries, verify every hash, open the staged database, run integrity/foreign-key checks and the compatible migration path, and verify referenced recordings before replacing current data. Refuse unsupported newer formats. Close pools/workers around final replacement, create a pre-restore backup, and make activation atomic. Treat an import source as untrusted bytes, even after decryption: encryption authenticates an artifact, not its application-level schema.

### Recommended encrypted artifact: age

Use `age` rather than inventing a password-encryption container. The format has recipient/passphrase key wrapping, an authenticated header and streaming authenticated payload. A versioned archive encrypted as a whole hides meeting titles and manifest contents. The conventional X25519 recipient and scrypt passphrase modes are interoperable; size and transfer timing remain observable. [age format specification](https://age-encryption.org/v1), [reference age implementation](https://github.com/FiloSottile/age)

Rust `age` **0.12.1**, published July 2026, is MIT/Apache-2.0 and interoperates with the Go tool. It offers `Encryptor::with_user_passphrase(SecretString)`, `with_recipients`, `wrap_output`, `Decryptor::new_buffered` and `decrypt`. The encrypting writer **must finish** to emit the final authenticated chunk. For restore, consume the decrypted stream completely before publishing staged files; otherwise later authentication/truncation failure can arrive after early plaintext has been used. For automated jobs, use generated X25519 identity/recipient keys rather than a hard-coded programmatic passphrase. [Versioned age crate/license](https://docs.rs/crate/age/0.12.1), [Encryptor API](https://docs.rs/age/0.12.1/age/struct.Encryptor.html), [Decryptor API](https://docs.rs/age/0.12.1/age/struct.Decryptor.html)

Use a private file/dialog input rather than CLI password arguments, avoid logging keys, keep a separately recoverable key/passphrase, and produce a decrypted staging file only in a private location. For large recordings, stream the archive into encryption instead of building the whole plaintext in RAM. Publish the backup only after finish, durable sync and successful verification; write `.partial` while creating it. A public checksum alongside an encrypted backup detects accidental changes but is not a substitute for authenticated decryption.

Libsodium secretstream with XChaCha20-Poly1305 is an alternative for an application-specific framed stream; its final tag detects incomplete streams, and password-key derivation supports Argon2id. This requires a versioned container, KDF metadata and careful framing/interoperability work. It is appropriate if there is a strong protocol need, but age reduces the initial format-design burden. Do not describe age's passphrase KDF as Argon2id: it uses scrypt. [Libsodium secretstream](https://libsodium.gitbook.io/doc/secret-key_cryptography/secretstream), [password hashing/key derivation](https://doc.libsodium.org/password_hashing)

For repeated large offsite backups, restic/rustic are stronger candidates than rewriting a multi-gigabyte encrypted archive every time: they provide encrypted, deduplicated snapshot repositories and local filesystem backends. They still require a consistent Sonefold snapshot as input and operational restore testing. Keep this an optional user-managed path, not a required hosted account. [Restic repository format](https://github.com/restic/restic/blob/master/doc/design.rst), [rustic project](https://github.com/rustic-rs/rustic)

## 9. E2EE synchronization is a separate layer

An encrypted backup can be copied to another device and restored. That is useful portability, but it does not merge concurrent notes, task status or deletions. Do not synchronize the live SQLite database/WAL as independently changing files. For a first single-owner sync mode, immutable encrypted meeting bundles plus explicit conflict-preserving import are much easier to reason about.

For later true editing sync, use immutable recording blobs plus per-meeting metadata/notes/task documents. Give edits stable IDs, causal parent/version information and tombstones; build SQLite/FTS/vector indexes as local projections. Transcripts should retain revisions instead of merging two model reanalyses word-by-word. Speaker/person mappings, accepted decisions and due dates need semantic conflict review rather than silently selecting the newest wall-clock timestamp.

Automerge is a Rust/JavaScript local-first CRDT option; it converges concurrent list/text edits and exposes competing scalar values. Its sync protocol assumes an ordered reliable transport. CRDT convergence does not supply encryption, device authentication, revocation or conflict meaning. Encrypt/authenticate changes at endpoints and preserve competing task/deadline values in the UI. Benchmark metadata history size before adopting it for the whole notebook. [Automerge conflicts](https://automerge.org/docs/reference/documents/conflicts/), [Rust synchronization API](https://automerge.org/automerge/automerge/sync/index.html), [official license](https://github.com/automerge/automerge/blob/main/LICENSE)

Syncthing can transport completed encrypted bundles without a hosted account. Its optional untrusted-device mode encrypts file contents/names/structure but exposes folder identity/label and sizes; the current documentation still marks that feature beta/testing. It is not a notebook merge engine or a retention policy. Prefer application-encrypted immutable backups for the initial transport-independent workflow. [Current Syncthing encrypted-device documentation](https://docs.syncthing.net/users/untrusted.html)

A relay/storage server must remain unable to read plaintext; TLS alone does not provide that property. Losing every decryption key means losing access. Device revocation requires new keys for future changes, and cannot retract plaintext a previously authorized device already received. These are protocol properties to design before marketing “E2EE sync,” not defects solved by a checkbox.

## 10. Recording recovery and fresh-install diagnostics

### Recovery must preserve original evidence

The initially inspected database manager deletes `meeting_minutes.sqlite-wal` and `-shm` after a malformed/corrupt error and retries. Stop that behavior: preserve the original database/WAL/SHM together, establish no concurrent writer, quarantine or copy the whole set, and attempt diagnostic/recovery operations on a duplicate. A retained WAL after a crash is normal SQLite behavior, not proof of corruption. Salvage can lose rows and should report what was recovered. [SQLite WAL persistence rules](https://www.sqlite.org/wal.html), [SQLite recovery tools and limitations](https://www.sqlite.org/recovery.html)

The existing audio saver writes roughly 30-second encoded checkpoint files and merges them with FFmpeg. The recoverable tail is therefore bounded by the last durably completed checkpoint, plus any captured buffer and encode-in-progress exposure; do not promise zero-loss recovery. Add monotonically numbered checkpoint metadata with sample range/rate, hash, duration and seal state. Write to a temporary file, validate decodability/sample duration, sync, rename and durably publish the manifest. Recover sealed files in sample order, report gaps/corrupt fragments, and keep originals until reconstructed output is verified. [Existing saver/recovery](../../frontend/src-tauri/src/audio/incremental_saver.rs)

Finalization should generate a new output file, verify decode/duration and update metadata before removing checkpoints. Recovery must be repeatable without accumulating duplicate transcript rows or overwriting a valid final recording. Recording callbacks must not block on encoding or disk sync; bounded queues and a separate durable writer keep capture responsive. Disk exhaustion must become a visible saving failure while the app accurately reports what remains recoverable.

### Diagnostics should distinguish presence from operation

Provide a read-only report first, with an explicit short recording/model smoke test separately. Report status, reason and one concrete remedy per capability:

| Layer | Useful checks |
| --- | --- |
| Application/bundle | App version, OS/architecture, bundled-resource resolution, sidecar presence/executable/version, frontend/native mismatch. |
| Storage | App data directory, free space, database schema/migration state, outstanding recovery candidates, index readiness. Avoid emitting meeting titles/paths in a shareable report. |
| Capture | Available microphone/system sources, selected device still present, permission state and backend errors. Device enumeration does not prove captured samples arrive. |
| ASR | Local checkpoint presence/hash, native provider availability, optional Python version/import compatibility. Distinguish dependency check from actual model load/transcription. |
| Generation/retrieval | Helper/Ollama reachability, configured local model, supported request features, embedding profile/readiness. Model presence is not a successful inference result. |
| Acceleration | Built backend and driver/runtime compatibility, actual loaded device when tested, fallback reason. GPU name is not proof that this binary supports Blackwell. |
| Privacy/support | Redact secrets, transcript text, user names and absolute private paths; save report locally and let the user choose onward delivery. |

Tauri sidecar packaging requires target-specific executable names and correct bundle resolution; a development checkout with prepared binaries is not evidence that a fresh installed bundle contains them. Verify clean installation independently on each supported platform. [Tauri sidecar packaging documentation](https://v2.tauri.app/develop/sidecar/), [local build instructions](../BUILDING.md), [existing validation limits](2026-09-30-validation.md)

No diagnostic should change display, GPU-routing or audio-system settings.

## 11. Evaluation and release criteria

Use consented/sanitized German and English meetings with named entities, overlap, corrections, uncertain deadlines, declined requests, recurring-calendar exceptions and a few long sessions. Label queries and evidence manually. Public meeting benchmarks help compare methods, but verify each dataset's own license before redistribution; a paper's license does not license its underlying recordings.

| Capability | Meaningful measurements/failure cases |
| --- | --- |
| Retrieval | Recall@20/100, nDCG@10, exact name/code recall, paraphrase and cross-language queries, date/meeting-filter accuracy, stale/deleted source exclusion. |
| Hybrid/reranking | Lexical vs dense vs fused vs reranked ablation; cold/warm p50/p95 latency, CPU/RAM/VRAM, batch consistency and recording responsiveness. |
| Chat | Answer correctness, citation correctness and completeness, source/quote existence, abstention on unanswerable questions, contradictions and attribution errors. |
| Actions/decisions | Commitment precision, proposal confusion, explicit owner/date correctness, evidence coverage, duplicate detection and preservation of reviewed task state. |
| Incremental summaries | Cache hit rate, recomputation after localized edits, restart/resume, source retention, provisional/final reconciliation and factual drift. |
| Calendar | DST changes, floating/all-day dates, exclusive end, RRULE/RDATE/EXDATE, moved/cancelled exceptions, duplicate imports and unsupported custom timezones. |
| Backup/restore | Live WAL snapshot, external recordings, missing audio, newer format, disk-full, crash during publication, wrong key, altered/truncated ciphertext, hostile archive entries and full restore on a clean machine. |
| Recovery | Forced process exit between checkpoints/finalization/DB commit; verify duration/sample gaps and idempotent retry, preserve originals on failure. |

Do not declare semantic search superior until it adds relevant hits or improves measured rank beyond lexical results. Do not declare citations accurate solely because a reference passed allowlist validation. Do not declare E2EE sync implemented when only encrypted backup export exists. Keep these distinctions in user-facing release notes.

## 12. Implementation order and deferred complexity

1. **Preserve sources:** non-destructive WAL recovery, durable recording checkpoint/finalization semantics, consistent backup and a staged restore path.
2. **Native evidence navigation:** FTS5, filters, source excerpts, stable turn/revision identities, direct transcript/audio navigation and shared retrieval API.
3. **Useful knowledge workflows:** per-meeting and cross-meeting grounded questions, reviewed action tracking, decision history and offline calendar import/export.
4. **Optional quality tier:** pinned Qwen 0.6B embedding/reranking worker, profile-aware rebuilds, rank fusion and representative local evaluation.
5. **Efficiency and portability:** persisted extraction/reduction caches, encrypted age artifacts, diagnostic reports and clean-install verification.
6. **Later sync:** immutable encrypted object exchange first; CRDT metadata/edit sync only after conflict/authentication/key lifecycle contracts are specified.

Graph-based retrieval and model training are not prerequisites. A reviewed relational decision/task graph plus source references already answers many temporal/multi-meeting questions. Consider more elaborate graph retrieval only when a labeled benchmark demonstrates failures that scoped hybrid retrieval and deterministic structured queries cannot address. Keep every optional model/server replaceable, all derived indexes rebuildable, and the original meeting evidence portable.

## 13. Quality-profile follow-up: strongest practical local retrieval shortlist

**This September 30 follow-up supersedes the 0.6B quality-tier recommendation in sections 3 and 12.** The cached 0.6B models are an explicit efficiency option. For a quality priority on a 24 GiB RTX 5090 Laptop GPU, recommend **NVIDIA Nemotron-3-Embed-8B-BF16 plus Qwen3-Reranker-4B**, with serialized inference and source-aware hybrid retrieval. If weights must specifically use Apache 2.0, compare **Octen-Embedding-8B** against **Qwen3-Embedding-8B** on the meeting evaluation before choosing. No single public benchmark establishes universal superiority.

### Evidence from the current primary leaderboard

The MTEB API was read on September 30, 2026. Scores below use its named benchmark rows. The public RTEB column is a calculated unweighted mean over the **same 16 public tasks present for these candidates**; the benchmark metadata now lists a seventeenth public task, SWEbenchCodeRetrieval, without these corresponding results. It is not an average over different coverage sets.

| Embedder | Current RTEB(beta) rank | Mean on the common 16 public RTEB tasks | RTEB German | LMEB long-horizon memory |
| --- | --- | --- | --- | --- |
| Nemotron 3 Embed 8B | 2 overall; highest-ranked downloadable model | 78.46 | 77.18, rank 1 | 64.36, rank 1 |
| Octen Embed 8B | 3 | 79.53 | 74.05, rank 7 | No result found |
| Octen Embed 4B | 7 | 77.47 | Not established here | No result found |
| Qwen3 Embed 8B | 15 | 73.10 | 66.85, rank 27 | 55.94, rank 25 |
| Qwen3 Embed 4B | Incomplete comparable coverage | Not reported | Not established here | 53.30, rank 47 |

Sources: [current RTEB score payload](https://mteb-leaderboard-backend.hf.space/v1/benchmarks/RTEB%28beta%29/scores), [Nemotron model scores](https://mteb-leaderboard-backend.hf.space/v1/models/nvidia/Nemotron-3-Embed-8B-BF16/scores), [Octen model scores](https://mteb-leaderboard-backend.hf.space/v1/models/Octen/Octen-Embedding-8B/scores), [Qwen8B model scores](https://mteb-leaderboard-backend.hf.space/v1/models/Qwen/Qwen3-Embedding-8B/scores), [Qwen4B model scores](https://mteb-leaderboard-backend.hf.space/v1/models/Qwen/Qwen3-Embedding-4B/scores).

**Interpretation:** Nemotron's German and memory results are better aligned with German/English meetings than an all-task embedding average, making it the preferred quality-profile candidate. Octen leads the common public RTEB mean, so do not call Nemotron a winner on every retrieval task. Current ranks, partial result coverage, private results and a calculated public mean are different quantities. MTEB removed the private column on January 14 because one benchmark partner had structural access to private data; the private results remain available. Do not compare Octen's January overall/private headline directly with NVIDIA's July public headline or treat model cards' dated first-place claims as current rankings. [MTEB maintainers' decision](https://github.com/embeddings-benchmark/mteb/issues/3934), [Octen's dated report](https://octen-team.github.io/octen_blog/posts/octen-rteb-first-place/), [NVIDIA's July release](https://huggingface.co/blog/nvidia/nemotron-3-embed-wins-rteb).

Keep Qwen3-Reranker-4B: its own multilingual reranking score is 72.74 versus 72.94 for 8B, while 4B leads that sibling on the reported English and instruction-following tests. Jina's July v3.5 card, using its unified top-100 protocol, reports Qwen4B ahead of Jina3.5 on MIRACL and RTEB; Jina wins that comparison's BEIR column. This is further evidence for a strong multilingual 4B shortlist, not a universally strongest reranker. Jina3.5 and Contextual multilingual v2 have noncommercial weight licenses and therefore do not meet this commercial-use default. [Qwen reranker results](https://huggingface.co/Qwen/Qwen3-Reranker-4B), [Jina3.5 comparison and license](https://huggingface.co/jinaai/jina-reranker-v3.5), [Contextual6B license](https://huggingface.co/ContextualAI/ctxl-rerank-v2-instruct-multilingual-6b).

### Reproducible revisions and actual memory floor

These ungated repository revisions were verified from first-party metadata, and all precede the cutoff. BF16 memory is parameter count times two bytes in GiB, **weights only**; runtime allocation is larger.

| Model | Repository revision | Last modified | License | BF16 weights |
| --- | --- | --- | --- | --- |
| nvidia/Nemotron-3-Embed-8B-BF16 | `d1f2f25730bbd775b99b29185134bc86653bf2d1` | 2026-08-28 | OpenMDW 1.1 | 14.81 GiB |
| Octen/Octen-Embedding-8B | `5adcfa292e712091dfc30f0e97f0b2282e6cc66c` | 2026-02-09 | Apache 2.0 | 14.10 GiB |
| Octen/Octen-Embedding-4B | `fea468fae3f0caffbae8a12ba792d1c394b6277d` | 2026-02-09 | Apache 2.0 | 7.49 GiB |
| Qwen/Qwen3-Embedding-8B | `1d8ad4ca9b3dd8059ad90a75d4983776a23d44af` | 2025-07-07 | Apache 2.0 | 14.10 GiB |
| Qwen/Qwen3-Embedding-4B | `5cf2132abc99cad020ac570b19d031efec650f2b` | 2025-06-20 | Apache 2.0 | 7.49 GiB |
| Qwen/Qwen3-Reranker-4B | `22e683669bc0f0bd69640a1354a6d0aebcfeede5` | 2026-04-16 | Apache 2.0 | 7.49 GiB |
| Qwen/Qwen3-Reranker-8B | `77d193c791ed757ca307ee72715aa132723da912` | 2026-04-16 | Apache 2.0 | 15.25 GiB |

Metadata sources: [Nemotron8B](https://huggingface.co/api/models/nvidia/Nemotron-3-Embed-8B-BF16), [Octen8B](https://huggingface.co/api/models/Octen/Octen-Embedding-8B), [Octen4B](https://huggingface.co/api/models/Octen/Octen-Embedding-4B), [Qwen8B](https://huggingface.co/api/models/Qwen/Qwen3-Embedding-8B), [Qwen4B](https://huggingface.co/api/models/Qwen/Qwen3-Embedding-4B), [reranker4B](https://huggingface.co/api/models/Qwen/Qwen3-Reranker-4B), [reranker8B](https://huggingface.co/api/models/Qwen/Qwen3-Reranker-8B).

Nemotron's weights and configurations use **OpenMDW 1.1**, not Apache; an Apache backbone or additional-information link does not change that. OpenMDW permits commercial dealing without a field-of-use restriction, requires retained license/origin notices on distribution, and imposes no additional obligations on outputs. [Actual OpenMDW license](https://openmdw.ai/license/1-1/).

Nemotron8B plus reranker4B needs **22.30 GiB in weights alone**. Desktop allocations, activations, workspace and other models make simultaneous residency unreliable on 24 GiB. Serialize embedding, reranking, ASR and generation, start with batch 1 and bounded input lengths, and measure free memory and latency. A model's laptop throughput was not measured in this research. Quantized checkpoints need a distinct profile and retrieval evaluation; a published acceleration result on H100/GB200 is not a measured RTX 5090 Laptop result. Lower-dimensional stored vectors reduce index cost, not transformer parameter memory.

### Offline API contract for the preferred embedder

Nemotron uses native `Ministral3Model`, `is_causal: false`, masked **mean pooling**, and 4096-dimensional vectors. Qwen last-token pooling must not be reused. Its documentation specifies Transformers >=5.2.0 and SentenceTransformers >=5.4.1 and supports SDPA without installing FlashAttention. Use the exact checkpoint's `query: ` and **`passage: `** prefixes; the release blog's specification table says `document: `, while the actual model card, saved prompts and inference example say `passage: `. Prefer the pinned checkpoint contract. [Pinned model configuration](https://huggingface.co/nvidia/Nemotron-3-Embed-8B-BF16/blob/d1f2f25730bbd775b99b29185134bc86653bf2d1/config.json), [official inference examples](https://huggingface.co/nvidia/Nemotron-3-Embed-8B-BF16).

Illustrative implementation contract, to be verified with the packaged runtime rather than treated as an executed test:

```python
# After an explicit download of the pinned snapshot; no network during inference.
import torch
import torch.nn.functional as F
from transformers import AutoModel, AutoTokenizer

revision = "d1f2f25730bbd775b99b29185134bc86653bf2d1"
model_id = "nvidia/Nemotron-3-Embed-8B-BF16"
tokenizer = AutoTokenizer.from_pretrained(
    model_id, revision=revision, local_files_only=True, padding_side="left"
)
model = AutoModel.from_pretrained(
    model_id, revision=revision, local_files_only=True,
    dtype=torch.bfloat16, attn_implementation="sdpa"
).to("cuda").eval()

texts = ["query: " + query] + ["passage: " + text for text in passages]
batch = tokenizer(texts, padding=True, truncation=True,
                  max_length=2048, return_tensors="pt").to("cuda")
with torch.inference_mode():
    hidden = model(**batch).last_hidden_state
    mask = batch["attention_mask"].unsqueeze(-1).to(hidden.dtype)
    pooled = (hidden * mask).sum(dim=1) / mask.sum(dim=1).clamp_min(1)
    vectors = F.normalize(pooled.float(), p=2, dim=-1)
```

The 2048-token bound above is an application starting point, not the model's maximum. Reject empty inputs, process queries/documents in appropriately sized batches, and retain the exact text/source hashes. Masked pooling excludes padding; disabling causal attention comes from the native pinned model configuration. Persist model ID, revision, tokenizer, pooling, prefixes, precision, dimension and normalization in the derived-index profile. Rebuild on any profile change. Combine scoped BM25 and dense candidates with RRF before reranking; do not replace lexical exact-name/date/code matches with embeddings alone.

Octen remains an Apache retrieval challenger rather than a drop-in parameter change: its model card explicitly recommends a `"- "` document prefix workaround for inherited Qwen behavior, so give it a separate profile. Qwen3-VL's newer multimodal release does not establish stronger transcript retrieval. [Octen usage and known issue](https://huggingface.co/Octen/Octen-Embedding-8B), [Qwen3-VL embedding card](https://huggingface.co/Qwen/Qwen3-VL-Embedding-8B).

## 14. Quality-profile follow-up: local generation for summaries and cited answers

**Recommend Qwen3.8-27B in a pinned 4-bit local runtime as the primary quality candidate**, with Qwen3.5-4B/9B explicitly available as faster profiles. Qwen3.8 is an actual August 2026 open release, not a hypothetical successor; its official checkpoint is ungated and Apache 2.0, and a pinned GGUF uses the existing native `qwen35` architecture. The release's instruction-following improvements make it a reasonable candidate for careful extraction and synthesis, but no direct German/English meeting-summary or citation-factuality evaluation was found. This is a practical, source-supported shortlist recommendation, not a claim that it beats every local summarizer. [Official Qwen3.8 checkpoint](https://huggingface.co/Qwen/Qwen3.8-27B), [current local Ollama model](https://ollama.com/library/qwen3.8:27b).

### What supports the choice, and what remains unproven

| Candidate | Relevant primary evidence | Practical role on 24 GiB |
| --- | --- | --- |
| Qwen3.8-27B | Official IFBench 79.5 versus 69.1 for Qwen3.6-27B; flexible thinking control. These are general instruction/agent results, not German meeting citations. | Preferred quality candidate in 4-bit form, with bounded context and serialized inference. |
| Qwen3.6-27B / Qwen3.5-27B | Apache predecessors with the same broadly supported Qwen3.5 architecture; 3.6 focuses strongly on coding. | Comparators or fallback for runtime compatibility; later Qwen3.8 merits evaluation first. |
| Qwen3.5-9B / 4B | Existing smaller Apache profiles, with lower memory/latency expectations. | Explicit fast modes, not an automatically selected quality floor. |
| Gemma 4 31B | Apache and ungated; official MMMLU 88.4 and MRCR v2 eight-needle 128K 66.4; multilingual pretraining and long-context evidence. | Strong alternate quality candidate in 4-bit form; requires a direct bilingual meeting evaluation against Qwen3.8. |
| Gemma 4 26B A4B | Apache and ungated; official MMMLU 86.3 and MRCR 44.1. Active parameters improve compute cost but all experts still need storage/residency. | Alternate efficiency/quality candidate; do not budget it as a 4B-weight model. |

Sources: [Qwen3.8 evaluation and protocol notes](https://huggingface.co/Qwen/Qwen3.8-27B), [Qwen3.6](https://huggingface.co/Qwen/Qwen3.6-27B), [Qwen3.5-9B](https://huggingface.co/Qwen/Qwen3.5-9B), [Gemma4 evaluation](https://huggingface.co/google/gemma-4-31B-it), [Google's April 2 release and Apache license](https://blog.google/innovation-and-ai/technology/developers-tools/gemma-4/).

Qwen's scores involve vendor-run harnesses, including in-house tasks and revised benchmark annotations. Gemma's published multilingual and needle retrieval tests do not measure meeting action attribution or citation entailment either. Do not rank these candidates by mixing independently reported metrics, assuming 4-bit results equal original-weight results, or assuming a supported maximum context is useful within the laptop's memory budget. For German specifically, verify language consistency, names, dates, speaker attribution and corrections on actual transcripts; Qwen3.8's public release does not supply a German meeting result.

### Exact source and runtime artifacts

All following pretrained repository heads precede the cutoff and are ungated Apache 2.0, verified using their first-party HF metadata:

| Model | Repository revision | Last modified |
| --- | --- | --- |
| Qwen/Qwen3.8-27B | `1d4bf0f2ff6012fd82039f2fa52739d0dd7c60c0` | 2026-08-14 |
| Qwen/Qwen3.6-27B | `6a9e13bd6fc8f0983b9b99948120bc37f49c13e9` | 2026-04-24 |
| Qwen/Qwen3.5-27B | `fc05daec18b0a78c049392ed2e771dde82bdf654` | 2026-04-24 |
| Qwen/Qwen3.5-9B | `c202236235762e1c871ad0ccb60c8ee5ba337b9a` | 2026-03-02 |
| Qwen/Qwen3.5-4B | `851bf6e806efd8d0a36b00ddf55e13ccb7b8cd0a` | 2026-03-02 |
| google/gemma-4-31B-it | `842da3794eaa0b77d5f08bae87a17459d91ff475` | 2026-07-20 |
| google/gemma-4-26B-A4B-it | `4d7ae4984b7db7de8f8457170b3f1a419ee76d52` | 2026-07-20 |

Metadata sources: [Qwen3.8](https://huggingface.co/api/models/Qwen/Qwen3.8-27B), [Qwen3.6](https://huggingface.co/api/models/Qwen/Qwen3.6-27B), [Qwen3.5-27B](https://huggingface.co/api/models/Qwen/Qwen3.5-27B), [9B](https://huggingface.co/api/models/Qwen/Qwen3.5-9B), [4B](https://huggingface.co/api/models/Qwen/Qwen3.5-4B), [Gemma31B](https://huggingface.co/api/models/google/gemma-4-31B-it), [Gemma26B](https://huggingface.co/api/models/google/gemma-4-26B-A4B-it).

A repository revision alone does not identify the quantized runtime artifact. Two independently verified options are:

| Runtime artifact | Exact identifier | Size and implication |
| --- | --- | --- |
| ggml-org/Qwen3.8-27B-GGUF | Repository `71bc7b627595dc8a91039addd9c791ae548d6747` (2026-09-27); file `Qwen3.8-27B-Q4_K_M.gguf`, SHA-256 `c600de0300ae8a0eb3a6c0b8b5561b8b96f16bd2c863c2a66c42de29d391a747` | 18,973,870,528 bytes, about 17.67 GiB; additional projector/draft artifacts are separate. |
| Ollama qwen3.8:27b, llama.cpp GGUF manifest | Manifest SHA-256 `aaee06c39dcf2437cde036998d960e1fc1494b8191be7cc9657d01e509097813`; model layer `f5f1dd8920d417aac2718b0bda3403da274301efdd6760b4f0f4b864ff2ad57d` | Model layer 16,810,714,464 bytes plus projector 931,146,016 bytes; about 16.52 GiB combined, before runtime memory. The tag is mutable. |

Sources: [ggml-org checkpoint and conversion provenance](https://huggingface.co/ggml-org/Qwen3.8-27B-GGUF), [HF file-size and LFS digest metadata](https://huggingface.co/api/models/ggml-org/Qwen3.8-27B-GGUF?blobs=true), [Ollama model details](https://ollama.com/library/qwen3.8:27b), [public registry manifest read on September 30](https://registry.ollama.ai/v2/library/qwen3.8/manifests/27b).

These are different conversions: do not assume identical weights, sizes or measured quality from their shared Q4_K_M label. Pin the chosen conversion, source checkpoint, chat template, runtime version, quantization, context length and sampling settings in an evaluation manifest. Ordinary tags and mutable `latest` are convenience selectors, not sufficient reproducibility records. These checks fetched metadata and a bounded 64 KiB GGUF header range; no tensor weights were downloaded and no model was executed.

### Memory budget, API behavior and evaluation contract

The official Qwen3.8 checkpoint contains roughly 27.78B total BF16 parameters, about **51.75 GiB in weights alone**, so BF16 does not fit. Its 8-bit form also exceeds the 24 GiB budget before runtime allocation. A 4-bit artifact is a credible fit candidate at batch 1 with an initial **8K or 16K total context** and no concurrent embedder/reranker/ASR model; actual fit still needs runtime measurement. Retain the existing bounded hierarchical summary pipeline instead of configuring the advertised 256K/1M context. Weight files, VRAM residency, KV cache, recurrent state, workspace, desktop memory and optional draft/projector allocations are distinct quantities.

For Qwen's architecture, the 16 full-attention layers with four KV heads of width 256 imply approximately 64 KiB of FP16 KV data per token, or 1 GiB at 16K, **excluding recurrent states and runtime overhead**. This is a derived estimate from the official architecture, not an observed allocation. Configurations with multiple parallel requests increase the cache budget. Speculative drafts may improve speed but consume additional memory; exclude them from the initial validated profile. [Pinned architecture configuration](https://huggingface.co/Qwen/Qwen3.8-27B/blob/1d4bf0f2ff6012fd82039f2fa52739d0dd7c60c0/config.json).

For the built-in native helper, preserve its source-validated evidence pipeline and bounded output budget. Its inspected protocol does not accept a JSON schema or constrained grammar, so a JSON instruction alone cannot guarantee syntax. Validate and reject malformed/unsupported evidence after generation; if grammar support is added, factual source validation remains necessary. Save final answer content as application knowledge. Ollama is an optional alternative, not the native implementation contract: `/api/chat` supports a schema in `format`, thinking control, and `keep_alive: 0` at the serialized boundary. Those options must not be assumed to exist in `llama-helper`. [Current helper protocol](../../llama-helper/src/main.rs), [Ollama chat API](https://docs.ollama.com/api/chat), [structured-output API](https://docs.ollama.com/capabilities/structured-outputs).

Promote the quality candidate after a reproducible comparison against existing 4B/9B and Gemma4 on sanitized German, English and mixed-language meetings. Freeze the same retrieved evidence, include unanswerable queries, negated commitments, changed deadlines, contradictory meetings and speaker corrections. Score supported-claim precision, citation correctness/completeness, action-owner-date precision, German fluency and factual consistency, plus cold/warm latency, memory and desktop responsiveness. Repeat with a fixed runtime, artifact, schema and sampling profile; preserve failure cases rather than accepting fluent summaries as validation. No published generic score substitutes for this evidence contract.


### Native compatibility, exact prompt and runtime update

The exact quality artifact URL is [Qwen3.8-27B-Q4_K_M.gguf at the pinned GGUF revision](https://huggingface.co/ggml-org/Qwen3.8-27B-GGUF/resolve/71bc7b627595dc8a91039addd9c791ae548d6747/Qwen3.8-27B-Q4_K_M.gguf). A read-only HTTP range request returned bytes 0–65535 of 18,973,870,528; its GGUF version 3 header declares:

| Native field | Verified value |
| --- | --- |
| `general.architecture` | `qwen35` |
| `qwen35.block_count` | 64 |
| `qwen35.context_length` | 262144 |
| `qwen35.embedding_length` | 5120 |
| Full-attention Q/KV heads; head width | 24 / 4; 256 |
| Recurrent/full-attention pattern | 48 recurrent layers, 16 full-attention layers; full attention every fourth layer |
| FFN width; rotary dimension | 17408; 64 |

This is direct binary metadata, consistent with the [pinned pretrained configuration](https://huggingface.co/Qwen/Qwen3.8-27B/blob/1d4bf0f2ff6012fd82039f2fa52739d0dd7c60c0/config.json) and [pinned Hub GGUF metadata](https://huggingface.co/api/models/ggml-org/Qwen3.8-27B-GGUF/revision/71bc7b627595dc8a91039addd9c791ae548d6747?blobs=true). Register `layer_count: 64`; an initial application context of 8192 or 16384 is separate from the artifact's training context. The native file is 18094.893 MiB; use a ceiling if download metadata requires whole MiB.

The GGUF's saved template exactly matches the [pinned Qwen3.8 Jinja template](https://huggingface.co/Qwen/Qwen3.8-27B/blob/1d4bf0f2ff6012fd82039f2fa52739d0dd7c60c0/chat_template.jinja), SHA-256 `c3cf9e34abf4f9e36c2d72165aa9c132d3e2a725b6c2586aaa3a8af9d7a81041`. A CPU-only Jinja rendering of the Qwen3.8 template, saved GGUF template and Qwen3.5 template, using a nonempty system/user text pair with `enable_thinking=False`, matched the current native format exactly. The assistant generation prefix is:

```text
<|im_start|>assistant
<think>

</think>

```

Consequently `qwen3.5_nonthinking` is source-compatible for the current ordinary text summary prompt; it does not require an invented Qwen3.8 delimiter. Qwen3.8's additional reasoning-effort instructions apply when thinking is enabled, and its full multi-turn/tool template has additional behavior, so this equality is not a blanket guarantee for tools or reasoning history. [Current native template](../../frontend/src-tauri/src/summary/summary_engine/models.rs), [pinned Qwen3.5 template](https://huggingface.co/Qwen/Qwen3.5-27B/blob/fc05daec18b0a78c049392ed2e771dde82bdf654/chat_template.jinja).

The inspected [Cargo manifest](../../llama-helper/Cargo.toml) and [lockfile](../../Cargo.lock) pin `llama-cpp-2` and `llama-cpp-sys-2` **0.1.146**, whose published wrapper commit `4afdaf0782ef7f3254a186a7ff67a1c7491c6dce` bundles llama.cpp **`e21cdc11a0461d8b0cbd28cc356d993bf6be7282`**. It implements the `qwen35` loader and graph. However, upstream's September 6 [GDN normalization correctness fix](https://github.com/ggml-org/llama.cpp/commit/5fdfa6282936576d2f352d4b97f397a109f207a6) replaces `ggml_l2_norm` with `build_gdn_l2_norm` for Q/K, correcting the normalization formula. The old bundled source lacks that fix. This matters to existing Qwen3.5 models as well as the new candidate; architecture recognition alone is insufficient runtime validation. [Exact old upstream source](https://github.com/ggml-org/llama.cpp/blob/e21cdc11a0461d8b0cbd28cc356d993bf6be7282/src/models/qwen35.cpp), [wrapper submodule provenance](https://api.github.com/repos/utilityai/llama-cpp-rs/contents/llama-cpp-sys-2/llama.cpp?ref=4afdaf0782ef7f3254a186a7ff67a1c7491c6dce).

**Update the paired bindings to 0.1.157 before promoting the quality profile.** This is the latest non-yanked pair published before the cutoff, released September 22 at 21:58 UTC. Its published, checksum-verified source archives identify wrapper commit **`b7bab03da647f07ed373335872643ea7fd0d0af4`** and bundled llama.cpp **`26394b4e6749a41c3633db040e0987500a5f7013`**. That bundled Qwen35 implementation contains `build_gdn_l2_norm`. Pin the wrapper and lock the matching sys crate rather than assuming a floating transitive dependency remains unchanged. [Wrapper release metadata](https://crates.io/api/v1/crates/llama-cpp-2/0.1.157), [sys release metadata](https://crates.io/api/v1/crates/llama-cpp-sys-2/0.1.157), [submodule provenance](https://api.github.com/repos/utilityai/llama-cpp-rs/contents/llama-cpp-sys-2/llama.cpp?ref=b7bab03da647f07ed373335872643ea7fd0d0af4), [exact corrected runtime source](https://github.com/ggml-org/llama.cpp/blob/26394b4e6749a41c3633db040e0987500a5f7013/src/models/qwen35.cpp).

| Published 0.1.157 source archive | Verified SHA-256 |
| --- | --- |
| `llama-cpp-2` | `6a5e80d283163beac602b70e99ab2d596e1f1c35aa3ed3422faa0ad990869ac8` |
| `llama-cpp-sys-2` | `cdd0b39fc874267312983364d48379179a7945ad9d4c068c176304ae1cdca1ec` |

The helper's applicable API break is `LlamaSampler::penalties`: version 0.1.157 adds the model vocabulary size as its first argument. Both current penalty call sites need `model.n_vocab()` before the existing four arguments. Negative `penalty_last_n` no longer means whole context, but the current helper already clamps it to nonnegative values. Source inspection found the other used backend/model/tokenization/context/batch/basic sampling signatures unchanged. Broader tool-chat and LLGuidance APIs changed, but this helper uses neither. This is a source audit, not a successful build. [New sampler source](https://docs.rs/crate/llama-cpp-2/0.1.157/source/src/sampling.rs), [current helper calls](../../llama-helper/src/main.rs).

Before hardware validation, fix two inspected native budget assumptions: `get_default_gpu_layers` guesses 33 layers for every file above 2.5 GiB and never consumes `ModelDef.layer_count`, capping a 64-layer model at 33 GPU layers; and `n_batch` equals the entire context, increasing prefill allocation. Use actual layer metadata, free-memory-aware offload and bounded prefill microbatches, then measure actual GPU residency, full-context prefill, decode speed and desktop responsiveness. The inspected generator seeds stochastic sampling from wall-clock time; repeatable evaluation needs a recorded/configurable seed or repeated runs. These are implementation recommendations; this research changed no product code and ran no GPU workloads. [Native offload and generation implementation](../../llama-helper/src/main.rs).
