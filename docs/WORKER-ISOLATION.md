# Worker isolation and follow-up audit

Snapshot from 17 October 2026; commit IDs and commit counts refer to the project's earlier, unpublished history.

Status at the time: implemented and verified on Linux. This is a source and
runtime hardening review, not an independent penetration test or legal certification.
The earlier [attack-surface audit](SECURITY-ATTACK-SURFACE.md) remains
applicable except for the worker-containment limitation superseded here.

## Scope and trust boundaries

The native Slint process communicates with a private Rust service over framed
stdio. The service owns the SQLite library, credential access and authorized
integration operations. Inference and media parsing receive narrow, explicit
inputs through `overtalk-worker`. They do not receive the library database,
provider credentials, host desktop sockets or ambient network access.

Covered launch paths: Python ASR/diarization/retrieval, llama.cpp notes, native
Whisper and Parakeet inference, voice embeddings, imported-audio decoding and
metadata, playback, waveform generation, audio encoding/checkpoint recovery,
redacted audio, and video encoding/remuxing. Embedded, compile-time VAD assets
remain inside the service; this is not permission to load arbitrary ONNX there.
Screen capture retains its explicit OS capture permission; its separate encoder
only receives raw frames and the selected output file.

| Platform | Mechanism | Local verification status |
| --- | --- | --- |
| Linux | Bubblewrap private filesystem/PID/IPC/network namespaces; no capabilities; no-new-privileges; nested user namespaces disabled; seccomp blocks ptrace, process memory access, BPF, keyrings, mount/module operations and other high-risk calls. No core dumps. | File/network/environment/descriptor denial, compiler, real CUDA, recording, ASR, diarization and notes workflows pass. |
| Windows | Unique AppContainer SID, explicit file ACLs, no network capabilities, inherited-stdio allowlist, kill-on-close Job Object. Output staged at low integrity, published after descendants terminate and reparse-point checks pass. | Cross-compile check; actual Windows test is required in CI. |
| macOS | Seatbelt deny-by-default profile; exact input/runtime reads, private work/output writes, restricted Metal service access; no network permission. | Cross-compile check; actual macOS test is required in CI. |

Linux requires the distribution's `bubblewrap` package and working unprivileged
user namespaces; on Ubuntu 23.10 and later AppArmor can block them (see
[Platforms](PLATFORMS.md#ubuntu-and-apparmor)). A failed restriction fails the worker launch; the app does not
silently run the worker without containment. Public system libraries are read-only.
GPU workers retain only the device/runtime access needed for inference. An
optional GPU-sharing launcher named by `OVERTALK_GPU_LAUNCHER` runs inside the
same namespace as the worker; CUDA arithmetic passed under such a launcher.

Inputs are staged read-only and outputs are explicit. Temporary storage is
private and removed with the worker; stale, unlocked worker directories are
cleaned on later launches after a short creation grace period. Windows publication
rejects indirect outputs and kills descendants before copying. The signed-in
user can still read files they own, so this does not defend against a compromised
OS account, administrator, kernel or GPU driver.

## Additional findings fixed

- Summary health pings and generation previously had independent write/read
  locks. Concurrent exchanges could consume one another's responses. A single
  exchange lock now covers each request; busy pings are skipped. Stale background
  monitor generations exit without shutting down their replacements.
- Worker responses now have explicit size bounds, framing checks and deadlines.
  Native inference transports PCM in fixed blocks rather than JSON float arrays.
- Imported media, metadata and waveform generation previously invoked native
  parsing in the service. They now decode outside that trust boundary; the parent
  validates finite PCM and enforces duration/sample limits.
- FFmpeg's shared libraries include distro BLAS/LAPACK symlinks. Exact read-only
  grants are needed; granting the full `/etc` or home directory is unnecessary.
- Versioned runtime library symlinks need their exact targets, not broad grants
  over application data. The launcher now handles those targets explicitly.
- The first meeting screen used a Unicode caret absent from its bundled font.
  A trace observed 980 system font opens through Parley/Fontique fallback.
  Timeline/filter controls now use SVG chevrons, retaining the same appearance.

## Fresh audit checks

Reviewed optional local listeners, bearer/capability tokens, OAuth state/PKCE and
callback bounds, API/CLI/MCP grants, provider credential audiences, redirects,
model provenance, archive parsing, erasure/restore, retained-web rendering,
subprocess argument handling and native/Python worker trust. No remote public
listening endpoint was introduced. Automation is loopback-only and disabled by
default; browser calls are rejected. Playback uses short-lived capabilities.

The final Gitleaks scan covers 1,213 text files and all reachable history:
no secret findings. Both large SQLite C sources are included in the working-tree
scan, with no oversized text-file exclusions. Binary assets are not text-scanned.
Rust and
frontend advisory gates pass; all 11 exact ML dependency pins report no known
advisories. Two yanked transitive versions were then replaced with compatible
patch releases: `der` 0.8.2 and `spin` 0.9.9. Documented inactive dependencies,
maintenance notices and verified frontend backports remain visible, rather than
hidden by broad ignores.

Build, workflow, installation and CI evidence follows. Source inspection and a
cross-compile are not substitutes for OS runtime tests.

## Sources

- [Bubblewrap security model and limitations](https://github.com/containers/bubblewrap/blob/main/README.md)
- [Microsoft AppContainer isolation](https://learn.microsoft.com/en-us/windows/win32/secauthz/appcontainer-isolation)
- [CISA: eliminating buffer-overflow vulnerabilities](https://www.cisa.gov/sites/default/files/2025-02/secure-by-design-alert-eliminating-buffer-overflow-vulnerabilities-508c.pdf)

## Intermediate verification (superseded by final results below)

The 101 retained-web tests, 97 speech checks (two skipped), 21 Slint LSP files
and TypeScript pass. Four real authorization-boundary and eight privacy workflows
pass in disposable libraries. Native voice enrollment, later recognition,
withdrawal/expiry, project scoping and deletion pass through the isolated worker.
Initial UI comparison: first frame 251→22 ms at 1280×720 and 276→79 ms at
3840×2160 under concurrent compilation; cached opens remain 6–11 ms. Idle
redraw count was one settling frame, then zero over the measured idle interval.
These measurements are CPU/request-to-frame measurements, not monitor FPS.

## Release verification

- Comprehensive Linux/native/backend run: 505 checks passed, eight manual checks
  ignored. The additional compiler-alias regression brings the sandbox suite to
  three passing tests, including inherited-descriptor, namespace, ptrace and
  host-network denial.
- Real Qwen-3 ASR 1.7B transcription and word timestamps pass with the compiler
  symlinks preserved. Real Qwen 3.5 4B notes pass. A disposable-profile workflow
  also passes recording, pause/resume, stop, persisted audio/notes and database
  integrity.
- Voice enrollment/recognition, withdrawal/expiry, project boundaries and removal
  pass using the isolated CPU embedding worker. Four local API authorization
  workflows and eight privacy workflows pass.
- Retained web UI: 101 tests; speech: 97 checks, two skips; evaluation-tooling:
  151 checks, three skips; TypeScript and all 21 Slint LSP files pass.
- Fresh secret scan: 1,213 text files plus reachable history, no findings.
  Reachable Rust and web dependency checks pass; the 11 ML pins have no listed
  advisories. `der` 0.8.2 and `spin` 0.9.9 replace yanked versions.
- Both Windows and macOS sandbox crate cross-compiles pass. Hosted jobs currently
  cannot start because of an external repository-account restriction. They are
  **not** recorded as passing OS runtime tests.

The combined Qwen ASR + Nemotron diarization probe also passes inside isolation,
with word timestamps and assigned speaker labels.
The synthetic recording/notes workflow completes with the final policy. Upstream
vendored sources retain their original whitespace; application changes pass
`git diff --check` with those source snapshots excluded.

## Installed release

A package built from code commit `af73218` was installed and checked. All seven
executable hashes matched the manifest. The running native UI and service
resolved to this package, and startup reported no errors. The installed service
passed the local transcription/notes workflow. SQLite 3.54.1 integrity and
foreign-key checks passed. All 16 audited library tables kept identical
before/after hashes.

The main-branch hosted validation run was rejected before any test step because
of an external account restriction. Windows/macOS runtime assurance
is therefore still an explicit gate, despite passing local cross-compiles.
This release is not a claim of universal regulatory compliance or proof that
no vulnerability remains.

## Final history-scan review

After committing the vendor sources, full-history scanning identified 14 public
SQLite API-macro/FTS5 identifier matches, not credentials. Exceptions now require
the exact public expression, specific SQLite source/header paths and the
`generic-api-key` rule. They do not exclude entire files or directories. This
uses Gitleaks' documented [path-and-match allowlist conditions](https://github.com/gitleaks/gitleaks#configuration).

Every scan now runs synthetic canaries inside those same vendor paths and an
ordinary source path, verifies all are detected, and checks output redaction.
Negative-control runs confirm that both an overly broad vendor ignore and
missing public-constant exceptions fail the canary check. The working-tree text
limit now includes the 8.8/9.1 MiB SQLite amalgamations. The final source/history
scan passes with no findings or oversized text exclusions.

A fresh installed-package check confirmed all seven executable hashes and both
running processes, and all 16 library-table hashes still matched the
pre-install inventory. These final changes affected the repository scanner and
documentation only, so the installed application was not rebuilt.
