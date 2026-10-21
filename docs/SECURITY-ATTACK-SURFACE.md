# Codebase attack-surface audit

Snapshot from 17 October 2026; commit IDs and commit counts refer to the project's earlier, unpublished history.

This adversarial review followed a privacy hardening pass. It covered the
working tree at that time, including uncommitted files. It is not an independent
penetration test, a guarantee of no vulnerabilities, or a HIPAA/GDPR
certification. Controls and deployment limitations from an earlier, unpublished
audit are not repeated here. [Worker isolation](WORKER-ISOLATION.md) supersedes
the worker-containment limitation described at the end of this document.

## Result

No real embedded API key or private key was identified in the scanned source or
the 51 reachable Git commits. The running native app and its service had no TCP
listening sockets at inspection time. This does not mean optional listeners can
never start: automation, retained-web audio and sign-in callbacks are described
below.

The review found additional credential, audit, retention, model-provenance and
availability gaps. The fixes below are implemented in source. Release and
installation evidence is recorded in the verification section rather than
inferred from source tests.

## Findings and changes

| Finding | Impact and prerequisite | Change |
| --- | --- | --- |
| A01: credential-bearing discovery clients followed redirects | Medium. An unexpected redirect from the trusted API, or a compromised trusted network intermediary, could forward a custom `x-api-key` header. No such redirect or stolen key was observed. The Slint tool installer also used redirect-following urllib with its GitHub token. | Anthropic/OpenAI/Groq discovery uses the shared no-proxy, no-redirect client, checks routing policy and bounds responses to 2 MiB. OpenRouter discovery is bounded too. The installer rejects redirects on authenticated GitHub metadata requests. |
| A02: mutable, incompletely verified model downloads | Medium supply-chain risk. Four summary models, including Qwen 3.5 4B, used `main` without a digest; Whisper used mutable URLs with only size/header checks. A substituted artifact could reach a native parser. | All nine summary models and 12 downloadable Whisper variants now have exact upstream revisions and SHA-256 pins. Whisper validates bytes while streaming, caps size, rejects cleartext redirects, ignores system proxies and times out stalled chunks. No weights were downloaded for this audit. |
| A03: protected-mode audit bypass in alternate clients | Medium. An already-authorized automation/CLI/MCP client could read permitted content without the protected-mode audit boundary used by the main UI. This did not bypass the integration's content grants. | Every automation operation now has preflight and outcome auditing. Protected CLI/MCP reads use that gateway; audit failure denies the read. Ordinary non-protected offline reads still work. Invalid privacy metadata cannot silently enable offline access. |
| A04: webhook revocation and retained payloads | Medium. A queued retry did not re-check changed privacy policy or meeting deletion; failed deliveries retained summary bodies in files without an implemented crash-replay consumer. Proxy inheritance also differed from the primary provider client. | Retries re-check the saved routing policy and meeting existence. The client does not inherit system proxies. Retries keep their body only in memory, and obsolete UUID-named outbox files are cleared at startup without following directory links. An already transmitted request cannot be recalled. |
| A05: incomplete bounds on the optional recording listener | Low, local availability. A slow local client could hold incomplete headers or stop consuming a recording stream. Capability tokens and host/origin restrictions already protected the content. | Added a 10-second header deadline and a 15-second blocked-send deadline; existing eight-stream and 64 KiB chunk bounds remain. |
| A06: permissive helper discovery | Low, local execution hardening. Broad `llama-helper*` matches and relative overrides could select an unintended program when an installation or developer environment was misconfigured. | Select only exact installed filenames and regular files. Explicit overrides must be absolute. Release builds no longer use the developer workspace fallback. This does not protect against malware already running as the same OS user. |
| A07: weak prevention of future accidental commits | Preventive gap, not evidence of an existing leak. Only `.env` was ignored; environment variants and local database/key files were easier to stage accidentally. | Added narrow ignore patterns and a pinned, checksum-verified Gitleaks CI check covering the current source and full reachable history. Exceptions match only public checksum literals and SQLite C macros, not whole source directories. |

Also required explicit safetensors loading for the optional Cohere/MOSS workers
and factuality probe. The legacy synthetic-preview tool now creates its backup
inside a private unique temporary directory instead of a predictable `/tmp`
filename.

## Network and command surfaces

| Surface | Binding / authority | Review |
| --- | --- | --- |
| Native UI → Rust service | Private framed stdin/stdout child process | Typed allowlist, bounded frame sizes and queues; no server socket. UI operations pass the shared audit boundary. |
| Optional automation API | `127.0.0.1`, configurable unprivileged port, default 8421 | Disabled by default; separate revocable API/CLI/MCP bearer tokens; constant-time comparisons; rejects browser Origin and non-loopback Host; body/type/timeout/concurrency limits; reads and writes require explicit grants. |
| Retained-web recording playback | `127.0.0.1`, ephemeral port | Random per-recording capability, exact Host/origin checks, range bounds, stream limits and revocation. Added slow-client deadlines. It serves an already-open recording, not arbitrary paths. |
| OAuth callbacks | `127.0.0.1`, ephemeral port; Slack uses 43871 | Fixed trusted providers, state and PKCE, duplicate-parameter rejection, bounded callback reads and attempts. No general-purpose command endpoint. |
| Browser bridge | Native-messaging stdin/stdout | No listening socket. Meeting-origin allowlist and 8 KiB message bound; atomic local status file. |
| Privatemode helper | Docker publishes only `127.0.0.1:8788` by default | Container's internal `0.0.0.0:8080` is not a host-wide port publication. Pinned image, read-only key mount, dropped capabilities, no-new-privileges. No live provider calls were needed here. |
| Provider and connection clients | Outbound HTTPS to selected providers | No inbound cloud service is implemented by this repository. Reviewed URL validation, credential audience, redirects, project scoping and reviewed outgoing changes. The legacy account client targets localhost:5167. |

The endpoint review includes Linear, Atlassian, Google, Microsoft, Slack, GitHub,
Notion, external model providers and webhooks. API keys belong in the OS keychain
or explicitly private local integration configuration. Model-generated text is
not allowed to select an arbitrary integration tool or automatically publish a
change.

## Imports, persistence and code execution

- Word imports are bounded and do not extract archive members to the filesystem;
  external XML entities are rejected. Backup restores authenticate the encrypted
  stream before publishing staged files and reject unsafe paths and duplicates.
- SQL search inputs are parameterized. Reviewed dynamic SQL uses fixed column
  and table fragments. The Python scanner's SQL-concatenation warning points to
  a constant speaker-column suffix, not user SQL.
- External meeting links use fixed executable argument arrays and a scheme
  allowlist. No transcript or model response is interpolated into a shell.
- The retained frontend has a restrictive CSP, safe Markdown rendering and no
  broad filesystem/asset grant. The current native UI has no embedded browser.
- Provider credentials are migrated to a namespaced OS keychain; the migration
  does not discard the original on a failed keychain write. Logs omit formatted
  content in production. Portable backups strip credentials.
- Reviewed speech loaders use local checkpoints, reject executable metadata and
  validate model shard paths. NeMo has archive/configuration guards and restricted
  PyTorch loading. Existing local checkpoint trust is still an assumption; the
  new download pins are not a claim that every old cached weight was rehashed.

## Verification

- Backend: **451 tests passed**, six pre-existing manual checks ignored. This
  includes actual loopback
  redirect tests proving a custom API-key header never reaches the second server,
  oversized response rejection, protected audit failures, webhook policy/deletion,
  old-outbox cleanup, helper selection, and model-catalog pins.
- Companion: **18 tests passed**, including protected gateway routing, permission
  revocation and invalid-policy handling. Speech suite: **97 tests run, two
  skipped**.
- Gitleaks 8.30.1: no findings after narrowly reviewed false-positive exceptions.
  It scanned 1,199 current text files and all 51 reachable commits. A synthetic
  AWS-shaped canary was detected and redacted with the same configuration.
  Four large/binary inputs
  are excluded from the working-tree text pass: the two vendored SQLite C files,
  a CJK font and a Visual Studio installer. Model/environment/build caches were
  not copied or scanned.
- Fresh Rust and frontend advisory checks passed the existing reachable-security
  gates. Eleven exact ML dependency pins had no listed advisories. Existing
  inactive optional Rust findings,
  maintenance/yanked notices and regression-verified frontend backports remain
  visible; they have not been relabelled as upstream fixed releases.
- Bandit reviewed **128 Python source/test/tool files**: 259 low and 34 medium
  heuristic findings, zero high. These are not 293 confirmed vulnerabilities.
  Reviewed categories include test assertions, fixed temporary paths, constant
  SQL fragments and offline model loads. Follow-up safetensors and temporary
  backup hardening are described above; source-specific review matters more than
  suppressing all scanner warnings.
- The native app/service listener inspection is a point-in-time observation with
  optional integrations idle, not proof that future configuration cannot expose
  another process or proxy.

Repeatable checks are in `scripts/audit-secrets.py` and
`scripts/verify-security-boundaries.py`. CI runs the former with full Git history
and the latter against the built service using a disposable profile.

### Installed verification

An installed package was checked after the fixes. All six packaged executable
hashes matched the manifest, and startup reported no errors. SQLite 3.54.1
integrity and foreign-key checks passed. All audited library tables (meetings,
passages, speaker records, timeline turns, summary records and benchmark
memberships, plus projects, notes, people and voice profiles) matched their
pre-install content hashes.

The built service also passed four real boundary workflows (token/origin/host
rejection, protected CLI auditing, failure-denies-content, obsolete webhook
cleanup) and eight privacy workflows, including encrypted restore and erasure.
The same four boundary checks also passed against the packaged installed
service. Test libraries were disposable; no live provider calls were made.

## Remaining boundaries

At the time of this audit, the inference and media helpers ran as the signed-in
user, not inside a full cross-platform OS sandbox. [Worker isolation](WORKER-ISOLATION.md)
later added that containment. A memory-safety bug in an updated native parser is
still a relevant threat. Do not treat arbitrary downloaded checkpoints as safe.

An attacker already controlling the OS account can read its unlocked keychain,
alter binaries/configuration or inspect memory. This is a single-user desktop
application, not a shared-user authorization system. Device encryption, protected
audit retention, verified release distribution and real Windows/macOS validation
remain operational requirements. Audit HMACs detect changed retained events but
do not prove that an entire tail was not removed. External provider security and
organization-managed deployment were outside this local source audit.
