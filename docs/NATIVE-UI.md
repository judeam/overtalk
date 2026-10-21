# Slint interface migration

The native interface lives in `native/`. It replaces the web UI with compiled
Slint components and communicates with the existing Rust engine over bounded
stdin/stdout frames. The engine has no web-view window in this mode. It still
uses Tauri's platform handles and plugins; removing Tauri from the engine is a
separate migration. The Linux installer retains the existing web bundle as a fallback and takes a
SQLite backup before selecting the native launcher.

## Development

First set up the platform dependencies and the build environment
(`OVERTALK_FFMPEG_PATH`, `OVERTALK_FFMPEG_SHA256` and `TAURI_CONFIG`) from
[Build and run from source](BUILDING.md#build-and-run-from-source-linux), then:

```sh
python3 scripts/setup-native-tools.py   # optional: pinned Slint LSP for editors
python3 scripts/check-slint-lsp.py       # optional: checks every .slint file with it
cargo test -p overtalk-native -p overtalk-protocol -p overtalk-documents
cargo build --release -p sonefold --bin sonefold-service
cargo build --release -p overtalk-worker-sandbox --bin overtalk-worker
cargo build --release -p llama-helper
cargo run -p overtalk-native
```

Choose the same acceleration feature for the service and `llama-helper`
([GPU notes](GPU_ACCELERATION.md#choosing-a-backend)). Place `sonefold-service`
beside `overtalk` for distribution: a release build of the UI looks for it only
there. A debug build also falls back to `target/release/sonefold-service`;
`OVERTALK_SERVICE` can explicitly select a service binary. The application reuses
the existing database and recording paths. Use separate XDG directories for
integration tests.

The setup script pins Slint LSP 1.18.1, verifies the official release checksum,
and generates a local VS Code configuration. `.vscode/extensions.json` recommends
Slint and rust-analyzer. The generated absolute LSP path is intentionally not
committed. The checker speaks the LSP protocol to the actual server; it checks
every `.slint` source rather than relying on file extension associations.

## Verification

```sh
python3 scripts/check-native-service.py
cargo build -p overtalk-native --example visual_review
node scripts/capture-ui-reference.cjs /tmp/overtalk-parity
python3 scripts/capture-native-parity.py --output /tmp/overtalk-parity --fixture /tmp/overtalk-parity/fixture.json
OVERTALK_TRACE_RENDER=1 cargo run --release -p overtalk-native
```

The parity scripts capture the same synthetic meetings in the web UI and native
UI at 1280×720, 1920×1080, 3840×2160 and 7680×4320. Start `pnpm dev` in `frontend/`
before capturing the web reference. The native example uses the production
controller with a fixture service and the software renderer, without a display
server or access to the real database. It prints the captured binary's checksum
so rebuilding during a capture cannot mix revisions. View 0 is recording, 1 is
transcript, 2 is library, 3 is settings, and 8 is notes. This checks layout, not
presented frame rate. Rendering traces
report CPU submission time and opening-to-first-frame time; they do not measure
GPU completion or bypass display refresh limits. Normal runs do not log these
timings or run a continuous animation loop.

The same example can record a bounded, 48-frame native interaction demo with
`OVERTALK_VISUAL_DEMO=1`; its output argument becomes a frame directory. It plays
the synthetic meeting and switches to Notes through the production controller.
See [README media](media/README.md) for isolated capture and encoding commands.

## Motion and speaker identity

Speaker badges reuse the timeline's existing color mapping and include a person
icon. Decisions and actions have their own glyphs. A brief outline identifies the
current speaker during playback; there is no repeating pulse. Buttons use 120 ms
color feedback, toggles use 160 ms movement, recording state uses a 180 ms outline,
and confirmations fade over 160 ms without displacing the workspace.

General settings provides Follow system, Reduced motion and Full motion. The
desktop preference is read once off the event-loop thread. It checks GNOME/GTK
and KDE settings on Linux, Reduce Motion on macOS, and client-area animation on
Windows. System setting changes take effect on the next launch. The app setting
applies immediately. `OVERTALK_REDUCED_MOTION=1` is useful for deterministic checks
with Follow system selected.

`native/tests/motion_idle.rs` exercises actual UI properties with a controlled
clock: state feedback animates, finishes, stops requesting frames, and becomes
immediate under reduced motion. It does not establish GPU frame rate.

## Security and compatibility

- The private transport has bounded messages and queues and an explicit command
  allowlist. It does not expose a new TCP listener or execute command strings.
- The existing engine continues to validate file access, providers, imports,
  backups and database changes. Optional API/MCP/webhook settings remain opt-in.
- Password fields are masked. Leaving the provider key field blank preserves the
  saved key. This migration does not change the existing credential storage.
- Note saves retain newer edits until acknowledged. Switching meetings and
  closing the window flush pending notes. Closing during a recording is guarded.
- The native service collects recording updates independently of UI delivery,
  replaces partial segments by sequence, and journals completed recordings before
  saving them to SQLite. Stable meeting IDs make journal retries idempotent.
  Failed saves can be retried while keeping the completed recording in memory;
  pending journals are recovered on the next launch. The integration checker can
  exercise generated speech through isolated Pulse/PipeWire devices, including
  pause/resume and stop-tail persistence (`--record`).
- Export files are written to a native user-chosen path through an atomic rename.
  PDF, DOCX, Markdown, text and JSON rendering stays local.
- The CI workflow is configured to check the interface and protocol on Linux, Windows and macOS on pushes and pull requests; once it runs, the workflow result, rather than local compilation, is the compatibility gate.
  Audio capture, platform permissions and packaged installation still need
  platform-specific integration validation before a release; [Platforms](PLATFORMS.md)
  tracks what is verified on each system.

## Release checks

The production controller is exercised by `verify_workflows`, alongside a real
engine with disposable data. The checker covers saving and reopening meetings,
manual notes, transcript corrections, date/tag search, actions, archive/trash,
bookmarks, templates, fragmented MP4 playback and shutdown. `--local-ai-data`
uses existing model files read-only for transcription and generated notes;
`--batch` additionally imports and retranscribes generated speech in that
disposable profile (requires `--local-ai-data`);
`--record` adds recording through private virtual audio devices without changing
the default microphone or speaker.

PDF exports contain searchable Unicode text, wrapped table cells, repeated
headers and page numbers. Word exports use actual tables with fixed column
widths and repeated headers. The export fixture includes 80 rows to check page
breaks. Existing BlockNote content is converted to Markdown before editing.

The UI retains six recently opened meetings and paginates transcript reads.
Waveforms are cached as images and recording/playback timers run only when
needed. There is no idle animation loop. `benchmark_ui` uses synthetic data and
the real GPU window; it never sends mouse or keyboard input. Rendering timings
measure CPU submission, not GPU completion or monitor presentation rate.

A local 4K run on 15 October 2026, after the playback, timeline and compact-layout
fixes, measured 1.24 ms median / 1.73 ms p95 CPU render submission over 243 frames.
The initial meeting open reached its first frame in 63 ms and repeated opens in
19–21 ms. Two queued frames occurred after entering the idle phase, then drawing
stopped. Peak UI-process RSS was 214 MiB; this excludes the engine and inference
models. Re-run these measurements after layout or rendering changes rather than
treating them as a hardware-independent guarantee or a presented-FPS measurement.

The CI workflow is configured to compile and test the Slint interface and Rust engine on Linux, Windows and
macOS, perform Linux service/controller workflows, capture 720p through 8K
layouts, and audit dependencies weekly and on changes. Platform microphone,
screen-capture permissions, signing and GPU drivers still need testing on actual
Windows/macOS hardware. See [dependency security](DEPENDENCY-SECURITY.md).

Install an already verified Linux release with
`python3 scripts/install-native-linux.py`. The script uses the current local
helper and FFmpeg, stages source resources, verifies copied hashes, backs up the
database/settings/launchers, and atomically changes the version symlink. It does
not change monitor settings. The existing web bundle remains in place and its
launcher is retained as `~/.local/bin/sonefold-web`.

After rebuilding the local inference helper, select that exact binary with
`python3 scripts/install-native-linux.py --helper target/release/llama-helper`.
The install manifest records the copied helper hash and whether the source
tree had uncommitted changes; it must not silently substitute an older helper.
`--gpu-launcher PATH` makes the installed launcher run the app and its GPU
workers under that launcher ([GPU notes](GPU_ACCELERATION.md#sharing-the-gpu-with-the-desktop)).

## Playback and timeline regressions

The native Guide contains all 13 guide chapters in an offline, searchable
chapter view. It has numbered instructions, screen shortcuts, keyboard help,
and locally saved reading progress. F1 opens it; Escape returns to the prior
workspace. Compact windows use a chapter selector rather than squeezing a
second sidebar beside the content. Source: `native/data/guide.json` and
`native/ui/guide.slint`.

Connections use bundled official Linear, Jira, Confluence and Google logos,
with provenance in `native/assets/LICENSE.connection-logos.txt`. Provider forms
show only their required fields, mask secrets, and keep actions and validation
visible while the body scrolls. They do not offer token regeneration for
third-party credentials. The primary flow is now browser account linking; manual
tokens are under Advanced. See [account linking](CONNECTION-ACCOUNT-LINKING.md) for
OAuth/MCP architecture, the Google app-registration gate, and verification limits.
Fixture tests do not connect real accounts.

The timeline reserves at least half the available review area for the reading
pane. Speaker controls switch to a single row when the lane cannot accommodate
two rows; selecting the lane reveals Only/Skip. Notes retains its separate,
smaller timeline preference. Capture with `--scale 1.5 --sizes 3840x2160` to
exercise actual OS scaling independently of the app's reading zoom.

Follow playback includes a short word-focus transition shaped by the native
text renderer, including wrapped and non-Latin text. **Current saved transcripts
have segment timestamps, not forced-aligned word timestamps:** this is an
estimated reading aid, not evidence of exact word timing. Its UTF-8 ranges are
cached per passage and searched without scanning the meeting; pauses stop the
clock and reduced motion removes movement. No transcript or source evidence is
rewritten to create this effect.

```sh
python3 scripts/capture-native-parity.py --views guide,guide-review,guide-search,word-tracking,meeting,notes --sizes 1280x720,3840x2160
python3 scripts/capture-native-parity.py --views guide,word-tracking,meeting,notes --sizes 3840x2160 --scale 1.5
```

`cargo build -p overtalk-native --example verify_playback` followed by
`python3 scripts/verify-native-playback.py` exercises the production controller
with 230 variable-height passages, paginated reads, forward/backward seeks,
follow on/off, font resizing, and crowded timeline notes. It verifies the active
passage's painted highlight in the viewport, not just its index. Manual scrolling disables
Follow playback; rechecking it returns to the current passage. Search results
remain stable while searching.

Timeline notes are clustered in screen space when labels would overlap. Opening
a cluster lists every original note with its time. Selecting a note inspects it;
only Play from or a time link seeks the recording. The playhead interpolates
between audio updates, and note selection/hover uses brief opacity feedback.
Notes generation has a custom SVG indicator; its timer runs only during the job
and is disabled for reduced motion. General meeting notes are the default for
new profiles. The preview’s silent Daily Standup default is upgraded once; other
templates and subsequent explicit choices are retained. The selected template is
shown in the Notes header. Regeneration also refreshes the decision/action lanes.

Notes has its own compact timeline, initially capped at a quarter of the window
height and at the space its lanes need. Drag its bottom edge, or focus the divider
and use the arrow keys, to resize it. This preference does not change the
Transcript timeline. Speaker playback controls appear when a Notes lane is
selected; active Only/Skip filters remain visible. Generation language, model,
template and meeting context live under Options. On wide windows, a removable
decisions/actions panel uses the spare width beside the readable document.
Narrow windows expose the same source-linked moments through its toolbar button.

When the optional local voice model is installed, completed speaker analysis
also checks for acoustically matching diarization slots. Several clean samples,
separation from competing voices and nonoverlapping turns are required; matching
display names alone never merge people. The Speakers page can run the same
check on a saved meeting without repeating ASR. It keeps a transcript recovery
copy before modifying existing labels. Only unresolved passages are reassessed
after a merge; their text and timestamps are preserved, and ambiguous overlap
stays unassigned. Voice embeddings used for this check are temporary and do not
enroll people in voice memory.

## Project workspace and meeting continuity

Use **Projects** in the sidebar to create a project and assign existing meetings.
New recordings inherit the selected project; a sound check does not. The project
contains people, vocabulary and context. Give a person a name and role, then use
**Remember voice** on a confirmed meeting speaker to associate clean voice samples
with that person. Recognition searches that project's enrolled people. A name on
its own never establishes an acoustic match; ambiguous voices remain unassigned.

Remembering a voice first asks how the person agreed: in this meeting, in
writing or another way (the last two say where it is recorded), and a
confirmation that they explicitly agreed. That consent is saved as a one-year
voice authorization before any samples are taken, unless one already exists in
Privacy settings. Recognition after each transcription is off until **Recognise
remembered voices after each transcription** is turned on in the privacy policy,
because it also analyses the other speakers' voices; **Recognize voices in this
meeting** works either way. **Manage remembered voices** lists the profiles kept
on this computer; choose one to review or export that person's data (the export
lists the profile, when it was made and how often it matched, without its numeric
template) or to forget it.

The Context tab separates explicitly supplied background from proposed facts.
Facts learned from meetings include a source quote. Accept a proposal after
reviewing its source; changing the transcript or moving the source meeting out of
the project prevents stale evidence from being reused. The next recording's home
screen shows accepted current facts and open actions as a preparation brief.
Vocabulary is passed as an ASR hint. It never silently replaces saved words.

The Review tab lists unassigned/overlapping/repetitive passages and notes whose
sources need attention. Open the source to listen or correct the words/speaker.
A dismissed issue reappears when that source revision changes. Notes history is
under **Notes → Options → Meeting tools**. Restoring a version preserves the
previous one and rejects a restore if the current version changed meanwhile.

**Activity** contains queued transcription, notes, translation and context work.
Cancel or retry work there. Low memory mode limits the speech worker to two CPU
threads and releases its model after transcription; it takes effect on the next
worker launch. Audio capture takes priority over new queued work. Reminders are
local and require Overtalk to be running.

For translated reading, open the transcript reading controls and request a
language. The original remains authoritative. A wide screen displays both
columns; narrower windows stack them. Translations are cached against the exact
source revision. Redacted exports create a copy: literal phrases/contact patterns
for documents, or explicit muted time ranges for audio.

## Backups and connections

In **Projects → Backups**, first save a recovery key outside the backup folder,
then enable scheduled encrypted backups. The app stores the public recipient,
not the recovery secret. A restored archive is verified and staged first. After
you close and reopen Overtalk, the database is activated and the previous database
and WAL files are retained in a `before-restore-*` recovery directory. Keep the
recovery key: losing it means encrypted archives cannot be decrypted.

In **Projects → Connections**, configure Linear, Jira Cloud, Confluence Cloud or
Google. Tokens live in the operating system keychain. Jira/Confluence use an API
token and account email; Google requires Desktop OAuth client credentials and
opens the system browser with PKCE and a one-use loopback callback. Browse/read
uses the configured account's permissions. Remote create/update is staged as a
local draft; the review screen's **Publish change** is the write boundary. An
uncertain network result is never retried automatically, avoiding duplicate work.
Google Calendar sync is read-only. Calendar entries show supplied attendees and
open meeting URLs with the operating system's default handler.

## Local meeting widget and optional video

Meeting detection is off by default. Enable it on the recording screen to show a
small local widget for an upcoming calendar meeting, an open Zoom/Teams process,
or a supported meeting page. Process presence means only that the app is open.
The optional [browser companion](../integrations/browser/README.md) reports a
visible meeting page and whether its video is active, through native messaging.
It does not read chats, record the browser itself or join calls. Browser extension
installation and enablement are explicit actions.

Video is a separate opt-in screen/window capture. The native OS permission/source
picker remains visible. `overtalk-capture` records a bounded stream at 15 FPS with
a 720p target, encodes on two CPU threads, and follows audio pause/resume. Its
fragmented MP4 is recoverable after interruption. Completed video is attached to
the saved meeting and can be opened from Meeting tools. A failed audio remux is
reported; the original audio remains available separately. Actual capture support
and permission behavior depend on the installed OS and desktop portal.

A packaged build includes `overtalk`, `sonefold-service`, `overtalk-capture`,
`overtalk-browser-bridge`, the speech worker resources, FFmpeg and llama-helper.
The CI workflow is configured to compile the native targets on Linux, Windows and macOS. Disposable Linux
workflow tests exercise project isolation, search, corrections, reversible notes,
queued work and encrypted restore. This does not substitute for permission and
provider-account testing on each target OS.

The checked setup, connected-calendar Home, durable automatic notes, personal
notepad and meeting companion are documented in [Meeting workflows](MEETING-WORKFLOWS.md).

## Sharing a copy of a meeting

**Share** (meeting tab row, the narrow-window **Tools** menu and **Meeting tools**)
opens one dialog: a web page or an Overtalk file, the audio to include, and what to
hide (names or phrases, contact details, the notes, time ranges to silence). The
dialog states the estimated size before the save dialog opens. Nothing is uploaded.
This is separate from **Send** in the Notes tab (`hub-share-notes`), which drafts
notes for a connected tool. **Open a shared meeting** sits beside **Import audio**
on Home, on its own row under **Record a meeting**, and as a folder button in the
sidebar; it opens an `.overtalk` file, which becomes a new
meeting with its transcript, notes, sources and audio. The engine owns building,
redaction and every import check; see [Sharing](SHARING.md). Visual review covers
`share-dialog` and `share-file` (a redacted Overtalk file):
`python3 scripts/capture-native-parity.py --views share-dialog,share-file --sizes 1280x720,760x540`.
At 760x540 the options scroll while the size and **Save copy…** stay visible.

## Build disk usage

Development and test profiles disable incremental compilation and debug symbols
to avoid accumulating large native build caches. For a debugging session, opt in
with `CARGO_PROFILE_DEV_DEBUG=1` or `CARGO_PROFILE_TEST_DEBUG=1`, then remove that
override afterwards. Use one target directory and build only needed targets.

After builds finish, inspect obsolete executables with
`python3 scripts/prune-build-artifacts.py`. Add `--apply` to remove older hashed
copies while retaining the newest per target/profile. This script does not touch
models, installed app versions, recordings, backups or dependency libraries.
