# Library, evidence and backups

Open **Library** in the sidebar to search transcripts, meeting titles, notes and generated summaries. Exact search uses SQLite FTS5. Date, meeting and tag filters apply before ranking. Enable semantic search in **Settings → Library & backups**, save a runtime/profile, then build the index. Changed or deleted sources cannot contribute stale vectors. Indexing is resumable; stop preserves completed batches.

The quality retrieval profile uses Nemotron 3 Embed 8B and Qwen3 Reranker 4B. Only one large local model runs at a time. It is slower to switch models but avoids keeping speech, generation and both retrieval models in VRAM together. Inference never downloads weights. Models require a recorded upstream revision; use the explicit download utility in `speech-worker/tools/download_models.py`. Nemotron's optional weights use OpenMDW 1.1; Qwen's use Apache 2.0. A compact Qwen profile is available for smaller devices.

**Ask** uses your selected summary provider. Each answer must supply source IDs and quotes that occur verbatim in retrieved evidence. Quotes and source revisions are checked before saving. Changed evidence marks saved answers stale. These checks establish source correspondence, not logical entailment: review the quoted passage before relying on the interpretation. Stop cancels retrieval/generation. Provider choice governs whether evidence leaves the device.

Open a source to load its transcript page directly and seek the saved recording. Playback streams through the native asset protocol and offers speed controls. Unknown timestamps remain unknown. Transcript text retains filler words and repetitions so displayed evidence matches stored text.

**Review as an action** opens an editable commitment, owner and due date. Saving is explicit. Completion/reopening does not alter the evidence snapshot. Manual actions also work without AI. Add meeting tags from the transcript view.

**Calendar** imports local ICS files. Supported recurring events, exclusions, detached overrides, cancellations and IANA time zones are expanded into a bounded upcoming window. Import warnings name unsupported constructs. Reimport replaces that source atomically. This does not connect to an online calendar or synchronize edits.

## Encrypted backup and recovery

In Library settings, enter a passphrase of at least 12 characters and matching confirmation, choose whether to include recordings, and save a backup. The app creates a consistent SQLite snapshot, removes credential/configuration tables, includes actions, notes and optional audio/templates, then encrypts and authenticates the archive with age. Model weights and provider/integration credentials are excluded. The passphrase is never persisted.

**Restore a copy** decrypts into a new directory, verifies every file and the database, rebases recording paths, and publishes the folder only after authentication succeeds. A wrong password, truncated archive or unsafe path cannot replace your workspace. This is a recoverable copy, not multi-device conflict synchronization.

To activate a restored copy, close Overtalk first. Preserve the entire existing application-data directory, including its database, WAL and SHM; never delete recovery files while the application runs. Copy the restored `meeting_minutes.sqlite` to the database filename expected by your installed build (`meeting_minutes.sqlite` in the current native manager), and keep the restored recordings folder at the paths embedded in that database. Reconfigure providers and local model/runtime paths. See the database manager when migrating older profiles. Keep the preserved profile until you have checked meetings, notes and playback.

The runtime diagnostics button checks SQLite, executable locations and storage permissions. Checkpoint discovery does not prove that inference will load; test a short recording and build a small semantic index.

## Assistant and CLI access

Enable the relevant client and meeting/transcript/summary read grants in Connections. The running app exposes the same filtered, indexed retrieval through the MCP `search_library` tool and:

```sh
python3 integrations/sonefold.py library 'What did we decide?' --tag product
python3 integrations/sonefold.py library 'launch' --keyword-only --after 2026-09-01
```

Tokens remain client-specific. Searching does not grant recording or editing permissions.

Short evidence IDs (`E1`, `E2`, …) exist only during a question request. The native validator resolves them back to the retrieved source revision and checks each quote against that source. Summary labels such as `[S1]` are not valid question citations. Unsupported IDs or altered quotes reject the answer; exact quotes establish source membership, not logical entailment.
