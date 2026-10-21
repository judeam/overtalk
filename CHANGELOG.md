# Changelog

## 0.1.0: first developer preview (20 October 2026)

Overtalk began on 30 September 2026 as a fork of the Meetily community edition
(v0.4.1, MIT). See [where Overtalk comes from](README.md#where-overtalk-comes-from).
This preview is built from source; Linux is the tested desktop path.

**Record and transcribe on your computer**
- Records the microphone and the call's audio without a meeting bot, and imports
  existing recordings. A recording made while the models download waits and is
  transcribed when the speech model arrives.
- Built-in speech recognition with Parakeet TDT 0.6B v3 (25 European languages); an
  optional speech worker runs Qwen3-ASR 1.7B (30 languages) in an OS-enforced sandbox
  with no credentials, app database or network (verified on Linux).
- A recording notice step: one sentence to paste or read aloud, saved with the
  meeting and in every export.

**Notes that cite their sources**
- Notes, decisions, actions and open questions, each linked to the passage it came
  from. Exports to PDF, DOCX, Markdown, text and JSON, with optional redaction.

**Projects and briefs, all open source**
- Meetings are filed into projects before they start (from the calendar invite) and
  after they end (from what was said), each choice with its reason.
- A brief before each meeting from past meetings, the calendar, connected tickets and
  the project, every line linked to its source.
- Voice memory that records the person's consent; recognition is off unless turned on.

**Pass it on**
- Share one meeting as a self-contained web page or an `.overtalk` file, with audio
  clips, the whole recording or none, and redaction. No upload or server.
- `overtalk-cli` and an MCP server for Claude Code, Claude Desktop, Codex, Cursor and
  VS Code, off until turned on, with a separate grant for each kind of access. The app
  proves its identity before a client sends its token.

**Privacy and packaging**
- A local usage report, kept on the computer; no telemetry.
- Optional private AI summaries through Privatemode, encrypted on the computer.
- Linux packages: `.deb` and AppImage (`packaging/`).

**Known limits**
- Windows and macOS builds are not yet validated on real hardware, and there are no
  signed installers.
- On real meetings the built-in speech engine misses many words: on the AMI test set
  (16 recorded meetings, 9 hours per condition) its word error rate is 34.8% on a
  headset mix and 38.3% on one table microphone, mostly deletions where people talk
  over each other. See `evaluations/ami/README.md`.

**Prepared for publication (21 October 2026)** ([release status](docs/RELEASE-STATUS.md))
- GPU workers run under a launcher only when `OVERTALK_GPU_LAUNCHER` names one; the
  service no longer looks for a wrapper in a fixed home-folder location.
- An installed macOS app ignores `OVERTALK_SERVICE`, so another program cannot choose
  the engine that inherits the app's microphone and screen-recording permissions.
  macOS helpers are started by absolute path.
- The worker sandbox also blocks io_uring. One error message now goes through the
  privacy-filtered logger instead of standard error.
- Removed commands left from Meetily's separate backend (profile and backend
  connection checks), which nothing called and which logged email addresses.
- The About dialog shows the attribution Slint's licence requires. `LICENSE.md` is
  the plain MIT text, NOTICE.md lists models and compiled-in components, and the
  Debian copyright file names Meetily's copyright.
- Windows packaging pins its FFmpeg and NSIS versions; cargo-deny now checks
  licenses; three advisories in rustls-webpki and webpki roots are fixed.
- Added SECURITY.md, issue-template guidance and corrected build, privacy and
  contributor documentation. Removed unused upstream images, fonts, an unreferenced
  Visual Studio installer and the obsolete updater-signing scripts.
- The repository holds current documentation only, indexed in `docs/README.md`;
  internal development logs, dated audits and design-tool files are not published.
  `docs/RELEASE-STATUS.md` lists what must happen before a binary release.
- The app names itself Overtalk wherever people see a name, including notifications,
  the tray, a restart prompt and the audio client the desktop's microphone indicator shows.
  Compatibility identifiers (`sonefold`, `com.judeam.sonefold`) are unchanged.
- Removed the psychiatric-session template inherited from Meetily; Overtalk makes
  no clinical claims.
- The worker sandbox refuses read grants that contain the home, data or
  configuration folders, and the speech worker's interpreter grant no longer
  widens to such a folder.
- CI pins the Rust toolchain action to a reachable commit, keeps pull requests from
  cancelling runs on `main`, and checks formatting of every package; Dependabot
  groups routine updates; a vulnerable `markdown-it` is overridden.
