# Platforms: Linux, Windows and macOS

Overtalk is one Rust workspace on every system: the Slint desktop app `overtalk`,
its private engine `sonefold-service` (started with `--stdio`), the sandbox
launcher `overtalk-worker`, the screen-capture helper `overtalk-capture`, the
browser bridge, `llama-helper` for local notes and, when the workspace has it,
`overtalk-cli`. This page records what has been verified on each system, how
audio capture and worker isolation work there, how to build the packages, and
the signing steps only the maintainer can do.

## Status

Verified on 20 October 2026 from commit `940de46` plus the porting branch (commit
IDs refer to the project's earlier, unpublished history). Linux results were
tested on Zorin OS 18.1 (Ubuntu 24.04 base) on x86-64, with unprivileged user
namespaces allowed. "CI" means a job exists in
`.github/workflows` but GitHub Actions has not yet run it: hosted runs are blocked
by an account billing restriction, so **no Windows or macOS build has run on
those systems yet**.

| | Linux x86-64 | Windows 10/11 x64 | macOS 14.2+ (Apple silicon) |
| --- | --- | --- | --- |
| Compiles | Yes; in daily use on the test system | Cross-checked from Linux: `cargo clippy --all-targets -D warnings` for `x86_64-pc-windows-gnu` passes for every workspace crate except `llama-helper`, whose bundled cpp-httplib needs MSVC headers. Real MSVC build: CI | Cross-checked from Linux for `aarch64-apple-darwin`: clippy passes for `overtalk-native`, `-protocol`, `-documents`, `-voice-memory`, `-browser-bridge`, `-worker-sandbox`. The engine, capture helper and `llama-helper` need the macOS SDK: CI |
| Unit and UI tests | Pass locally | CI (`native` matrix) | CI (`native` matrix) |
| Installers | `.deb`, AppImage (packaging/linux), installed locally | NSIS installer compiled with makensis 3.09 on Linux from placeholder binaries; real installer, silent install, uninstall and portable zip: CI | `.app` in `.dmg`, ad hoc signed: CI |
| Packaged engine starts headless | Yes: the engine from the `.deb` passes `scripts/check-native-service.py` | CI, from the installed folder and from the zip | CI, from the mounted `.dmg` |
| Desktop UI screenshots (software renderer) | Yes, `home`, `meeting`, `settings` at 1280×720 | CI artifact `screens-windows` | CI artifact `screens-macos` |
| Worker sandbox | bubblewrap, runtime-tested | AppContainer; CI runs the denial probe test | Seatbelt; CI runs the denial probe test |
| Microphone and call audio on real hardware | Verified (PipeWire/PulseAudio) | Not verified | Not verified |

Hosted runners have no audio devices, so CI cannot prove recording works. Before
calling Windows or macOS supported, record a meeting on real hardware with both
microphone and call audio, and add the result here.

## Install layout and data

| | Program files | Engine data (meetings, settings, models) | UI preferences |
| --- | --- | --- | --- |
| Linux (.deb) | `/usr/lib/overtalk/bin`, resources in `/usr/lib/overtalk/lib/Sonefold` | `~/.local/share/com.judeam.sonefold` | same folder |
| Windows | `%LOCALAPPDATA%\Programs\Overtalk`: every executable, ONNX Runtime and Visual C++ runtime DLLs, `ffmpeg.exe`, `templates\`, `speech-worker\`, `integrations\`, `licenses\` | `%APPDATA%\com.judeam.sonefold` | `%LOCALAPPDATA%\com.judeam.sonefold` |
| macOS | `Overtalk.app/Contents/MacOS` (executables), `Contents/Resources` (resources, licences, icon) | `~/Library/Application Support/com.judeam.sonefold` | same folder |

Each executable finds the others beside itself; the engine finds its resources
through Tauri (the executable's folder on Windows, `Contents/Resources` on macOS).
Uninstalling never removes the data folders.

On Windows the engine folder is in the roaming profile, so downloaded models
(1.7 to 3.3 GB) would roam on a domain with roaming profiles. Moving models to
`%LOCALAPPDATA%` needs a migration and is not done yet.

## Audio capture

| | Microphone | Call (system) audio | Screen video |
| --- | --- | --- | --- |
| Linux | PulseAudio-compatible server (PipeWire-Pulse) | The chosen output's monitor source ([details](LINUX_AUDIO.md)) | xdg-desktop-portal picker (PipeWire) |
| Windows | WASAPI through cpal | WASAPI loopback of the chosen output device; no driver needed. An app holding a device in exclusive mode can block it | Windows Graphics Capture (`windows-capture`) in `overtalk-capture` |
| macOS | Core Audio through cpal; asks for Microphone permission | Core Audio process taps (the default backend); asks for "System Audio Recording Only" | ScreenCaptureKit in `overtalk-capture`; asks for Screen Recording |

The macOS minimum is 14.2. The engine calls Core Audio's process-tap API
through `cidre`, which links those functions directly, so on macOS 13 the engine
would not even start; `frontend/src-tauri/.cargo/config.toml` already targets
14.2. Supporting macOS 13 would need those symbols weak-linked and a fallback
to a loopback device such as BlackHole.

The macOS prompts show the usage strings in `Overtalk.app/Contents/Info.plist`
(`NSMicrophoneUsageDescription`, `NSAudioCaptureUsageDescription`,
`NSScreenCaptureUsageDescription`). The engine and helpers run from inside the
bundle as children of Overtalk, so macOS attributes their access to Overtalk.app.
The engine uses the Accessory activation policy, so it adds no second Dock icon.
macOS keys these permissions to the code signature: an ad hoc signed build asks
again after every update, a Developer ID build keeps them.

## Worker isolation

Local models, FFmpeg and media parsing run through `overtalk-worker`
(`crates/worker-sandbox`). It receives exact read grants (inputs, model and
runtime folders), exact write grants (outputs and a private work folder) and no
network. If the system cannot apply the isolation, the launch fails; there is no
unsandboxed fallback. Inherited file descriptors and handles other than standard
input and output are closed or not inherited, and the environment is reduced to
an allowlist.

- **Linux:** bubblewrap with new user, PID, IPC, network and mount namespaces, a
  read-only `/usr`, a seccomp filter (no ptrace, BPF, keyrings, mounts, module
  loading or nested namespaces), no capabilities and no core dumps. GPU workers
  see only the GPU device nodes. Requires the `bubblewrap` package and
  unprivileged user namespaces; the `.deb` handles Ubuntu's AppArmor restriction
  ([Ubuntu and AppArmor](#ubuntu-and-apparmor)).
- **Windows:** a fresh AppContainer profile per launch with no capabilities, so
  no network. The launcher adds an ACL entry for that container to each granted
  path, including the DLLs beside the program (ONNX Runtime and the Visual C++
  runtime load from there), and removes the entries afterwards. The worker runs in
  a kill-on-close Job Object limited to 128 processes, with no console window.
  Outputs outside the private folder are written there first and copied only
  after every process in the job has ended, refusing links and reparse points.
  The installer grants `ALL APPLICATION PACKAGES` read access to the program
  folder, as Windows does for Program Files.
- **macOS:** `sandbox-exec` with a deny-by-default Seatbelt profile: reads of the
  system (`/System`, `/usr`, `/Library/Apple`, `/opt/homebrew`) and the granted
  paths, writes only to granted paths, no network rule (so none), and for GPU
  workers IOKit and the Metal compiler service. File metadata stays readable
  everywhere. Apple deprecated `sandbox-exec` but still ships it; if it is
  removed, worker launches fail closed.

None of these protects against a compromised user account, an administrator, the
kernel or a GPU driver. A worker can use as much CPU and memory as the user's
other programs; only the process count is limited (Windows).

### Ubuntu and AppArmor

Ubuntu 23.10 and later, including 24.04, restrict unprivileged user namespaces
through AppArmor (`sysctl kernel.apparmor_restrict_unprivileged_userns` is `1`).
A program without a profile that says `userns,` still gets a namespace but no
capabilities in it, so bubblewrap stops with `loopback: Failed RTM_NEWADDR:
Operation not permitted`. Workers then fail closed: with the sandbox down,
recording (the encoder runs in it), local transcription, local notes, playback and
media imports do not work.

**The .deb fixes this.** It ships `/etc/apparmor.d/overtalk-worker`
([source](../packaging/linux/overtalk-worker.apparmor)), modelled on AppArmor's own
`bwrap-userns-restrict`, but attached to Overtalk's launcher instead of
`/usr/bin/bwrap`, so other programs that run bubblewrap are untouched. Three
profiles, all enforcing:

| Profile | Applies to | `userns` | Capabilities |
| --- | --- | --- | --- |
| `overtalk-worker` | `/usr/lib/overtalk/bin/overtalk-worker` | denied | none needed; may not use `ptrace` |
| `overtalk-worker-bwrap` | `/usr/bin/bwrap` when the launcher runs it (`Px`) | allowed | allowed |
| `overtalk-worker-unpriv` | stacked onto the sandboxed workload | denied | denied |

They allow almost everything else, as Ubuntu's do: AppArmor does not confine the
files here, bubblewrap does. Why not a simple `userns,` on the launcher with
`flags=(unconfined)`, which also works: a local user could then run the launcher
with `LD_PRELOAD` and have the injected code create a user namespace with
capabilities, which lifts Ubuntu's restriction for everyone. In the enforcing
profile that code runs in a profile that denies `userns`, and it cannot get
around that by running bubblewrap itself: the launcher starts bubblewrap with
`Px`, so glibc ignores `LD_PRELOAD`, `LD_LIBRARY_PATH` and `LD_AUDIT` there, the
launcher may not `ptrace` at all, and nothing may trace or read bubblewrap's
process (`deny ptrace (readby, tracedby)`). The launcher hands `LD_LIBRARY_PATH`
to the workload with `--setenv`, so it still arrives. Residual risk: any local user can run `overtalk-worker` with a
policy and program of their choosing and get a namespace in the launcher's fixed
bubblewrap sandbox, in which the program has no capabilities and cannot nest
namespaces, as with Ubuntu's `bwrap` profile; it cannot get capabilities that
way.

The file is a conffile (edit it or add `/etc/apparmor.d/local/overtalk-worker`).
The package loads it with `apparmor_parser` after installing or upgrading and
unloads it on removal, only when AppArmor is enabled; elsewhere (Debian without
AppArmor, containers, an RPM made with alien) it does nothing, and a failed load
never fails the installation. It `Breaks: apparmor (<< 4.0)`, because older
parsers do not know the syntax. Kernels without user namespace mediation need no
profile.

Not applicable to an AppImage, which cannot install a profile. On a restricted
host its launcher tests bubblewrap and prints why the sandbox is down. Because
the recording encoder and the Whisper and Parakeet workers all run in the
sandbox, the built-in engine does not work there either. Use the `.deb`, or
lift the restriction for every program:
`sudo sysctl kernel.apparmor_restrict_unprivileged_userns=0` (put it in a file
in `/etc/sysctl.d/` to keep it after a reboot). That turns off a system-wide
hardening, so prefer the `.deb`.

CI proves the profile on GitHub's stock Ubuntu 24.04 runner, with the restriction
left on (the `package.yml` Linux job): a sandboxed workload fails before the
install, runs after it from `/usr/lib/overtalk/bin` with `LD_LIBRARY_PATH` reaching
the workload, while a library injected with `LD_PRELOAD` is refused a user
namespace, is not loaded when it re-runs bubblewrap with itself preloaded, and
cannot `ptrace` or `process_vm_writev` a running bubblewrap, fails when the profile is unloaded, and
the profile is gone after removal. The same copy of the launcher at
another path stays blocked. Not tested: Ubuntu 24.10 and later (their
bubblewrap profile may also apply), and Debian 13 (AppArmor is on, but the
restriction is not).

## Building the packages

Each script builds release binaries in its own `target/package-<os>` folder from
a clean checkout, stages one tree, and writes the packages, a build record
(`*-build-info.json`: commit, dirty flag, rustc, target, binary hashes, what was
bundled, signature state) and `SHA256SUMS` to `target/package-<os>/dist`. FFmpeg
must be a verified build input first ([BUILDING.md](BUILDING.md#verified-ffmpeg-input)).
All packages are CPU-only; GPU builds are not packaged.

### Linux

See [packaging/README.md](../packaging/README.md).

### Windows

Needs Visual Studio 2022 Build Tools with the C++ workload and a Windows SDK,
Rust 1.94 or newer (MSVC), Python 3.12, CMake, NSIS 3 and FFmpeg:

```powershell
choco install ffmpeg --version=9.0.2 -y
choco install nsis --version=3.13.0 -y
python scripts/configure-ffmpeg-build.py   # prints OVERTALK_FFMPEG_PATH and _SHA256; set both
python packaging/windows/build.py          # --formats installer,zip  --version  --skip-build
```

| File | Contents |
| --- | --- |
| `Overtalk-<version>-windows-x64-setup.exe` | Per-user installer: no administrator rights, `%LOCALAPPDATA%\Programs\Overtalk`, Start menu shortcut, an entry in Installed apps, and an uninstaller. Silent: `/S /D=<folder>` |
| `Overtalk-<version>-windows-x64-portable.zip` | The same folder, to unpack anywhere |
| `overtalk-windows-build-info.json`, `SHA256SUMS` | Build record and checksums |

The installer refuses a non-empty folder that is not an earlier Overtalk. The
uninstaller deletes exactly the files the installer wrote, the shortcut, the
uninstall entry and the "Open Overtalk when you sign in" value, never the data.
Bundled: ONNX Runtime 1.22.0 (downloaded and checked by the engine's build
script), the Visual C++ runtime DLLs the binaries import (found with `vswhere`;
Microsoft permits this app-local redistribution of the files in `VC\Redist`),
and `ffmpeg.exe` from the verified build input. Chocolatey's FFmpeg is a gyan.dev
build under the GPL 3.0, so publishing the installer carries the GPL's
corresponding-source obligations for that exact build; the maintainer must decide
how to meet them (for example, attaching or linking the matching source) before a
public release. `--ffmpeg none` leaves it out (recordings then cannot be written
until FFmpeg is installed beside the engine).
`llama-helper` targets x86-64-v3 (AVX2), like on Linux.

### macOS

Needs Xcode (command-line tools), Rust 1.94 or newer, Python 3.12, CMake and
`brew install ffmpeg` (the verified build input):

```sh
set -a; eval "$(python3 scripts/configure-ffmpeg-build.py)"; set +a
python3 packaging/macos/build.py                 # ad hoc signed .dmg; --formats dmg,zip
```

The result is `Overtalk-<version>-macos-arm64.dmg` (Overtalk.app and an
Applications link), `overtalk-macos-arm64-build-info.json` and `SHA256SUMS`.
`LSMinimumSystemVersion` is 14.2 and every binary, CMake's included, is built
for that deployment target ([why](#audio-capture)). Whisper uses Metal and, when a Core ML encoder is
present, Core ML. Homebrew's FFmpeg is not bundled because it needs Homebrew's
libraries; the engine uses `/opt/homebrew/bin/ffmpeg` or `/usr/local/bin/ffmpeg`.
`--ffmpeg <path>` bundles a self-contained FFmpeg instead. Intel Macs:
`--target x86_64-apple-darwin` cross-compiles on Apple silicon; it is untested and
not built by CI.

### The command-line client

`overtalk-cli` (`crates/cli`) ships beside the app in every package:
`/usr/bin/overtalk-cli` from the `.deb`, `Overtalk-….AppImage cli …`, the program
folder on Windows (`overtalk-cli.exe`, not added to `PATH`), and
`Overtalk.app/Contents/MacOS/overtalk-cli` (link it into `/usr/local/bin` to use
it from a terminal). The Windows and macOS packagers fail if the workspace has
the client but it was not built. Each package's smoke test runs
`overtalk-cli --version`.

## CI

- `ci.yml`, job `native` (Linux, Windows, macOS): formatting, clippy, tests,
  release builds, and screenshots of the real UI through the software renderer
  (`scripts/capture-native-parity.py --views home,meeting,settings --sizes 1280x720`),
  uploaded as `screens-linux`, `screens-windows` and `screens-macos`. Windows
  checkouts keep LF line endings so generated-file checks compare bytes.
- `package.yml` (pushes to `main`, packaging pull requests, weekly, manual):
  builds every package from a clean target folder, then
  - Linux: unpacks the `.deb` and the AppImage and runs the engine smoke test in each;
    then installs the `.deb` with `apt-get` on the runner, whose AppArmor
    restriction stays on, and runs a sandboxed worker (`scripts/check-worker-sandbox.py`)
    with and without the profile, then removes and purges the package;
  - Windows: installs silently, checks the shortcut, the worker's build identity
    and the engine, uninstalls silently and checks nothing is left; then runs the
    engine from the portable zip;
  - macOS: mounts the `.dmg`, verifies the code signature and the `Info.plist`
    keys, checks the worker's build identity and runs the engine.
  Packages are uploaded as `overtalk-linux`, `overtalk-windows`, `overtalk-macos`.
- `release.yml` (manual, with a tag such as `v0.1.0`): runs `package.yml` with
  signing enabled, combines one `SHA256SUMS`, and creates or updates a **draft**
  release. It never publishes and refuses to touch a published release.

The engine smoke test keeps each run's profile disposable: Linux through
`XDG_*`, macOS through `HOME`. Windows resolves `%APPDATA%` through the Known
Folder API, which ignores environment variables, so there the check refuses to
run if an Overtalk profile already exists, and deletes the profile it created.

## Signing

Nothing here contains keys; every signature needs the maintainer's own credentials.
Unsigned builds work but warn: Windows SmartScreen shows "Windows protected your
PC" (More info, Run anyway), and macOS blocks the first open of an ad hoc signed
app (System Settings, Privacy & Security, Open Anyway).

### Windows (Authenticode)

1. Get a code-signing certificate: an OV or EV certificate from a certificate
   authority, or Azure Trusted Signing. EV and Trusted Signing get SmartScreen
   reputation at once; OV builds it up over downloads.
2. Locally, put the certificate in your user store (a token's driver does this;
   for a `.pfx`, run `Import-PfxCertificate -FilePath overtalk.pfx
   -CertStoreLocation Cert:\CurrentUser\My -Password (Read-Host -AsSecureString)`)
   and set `OVERTALK_WINDOWS_CERT_SHA1` to its thumbprint. Optional:
   `OVERTALK_TIMESTAMP_URL` (default DigiCert's) and `OVERTALK_SIGNTOOL`.
   `packaging/windows/build.py` then signs every Overtalk executable, the
   uninstaller (through a small wrapper script NSIS runs) and the installer with
   SHA-256 and a timestamp. Certificate files and passwords are never passed to
   signtool, so they cannot appear in a command line, a log or the NSIS script;
   the packager also masks any value after `/p` or `--password` in the commands
   it echoes.
3. For CI, add the repository secrets `WINDOWS_CERTIFICATE_PFX_BASE64` (the
   `.pfx`, base64-encoded) and `WINDOWS_CERTIFICATE_PASSWORD`. The workflow
   imports the certificate into the runner's user store with PowerShell (the
   password only in that step's environment), deletes the `.pfx`, signs by
   thumbprint, and removes the certificate and key at the end. A certificate on a
   hardware token cannot be used on hosted runners; sign locally instead. Azure
   Trusted Signing needs `signtool /dlib` arguments, which the script does not
   produce yet.

### macOS (Developer ID and notarization)

1. Join the Apple Developer Program, create a **Developer ID Application**
   certificate, and export it with its private key as a `.p12`.
2. Create an app-specific password for the Apple ID and note the Team ID.
3. Locally:
   ```sh
   xcrun notarytool store-credentials overtalk-notary --apple-id <id> --team-id <team> --password <app-specific>
   python3 packaging/macos/build.py --sign-identity "Developer ID Application: <Name> (<TEAM>)" --notary-profile overtalk-notary
   ```
   With an identity the script enables the hardened runtime, a secure timestamp
   and `packaging/macos/entitlements.plist` (microphone only), signs the `.dmg`,
   submits it for notarization and staples the ticket. Check with
   `spctl -a -vvv -t install <dmg>` and `xcrun stapler validate <dmg>`.
4. For CI, add the secrets `MACOS_CERTIFICATE_P12_BASE64`,
   `MACOS_CERTIFICATE_PASSWORD`, `MACOS_SIGN_IDENTITY`, `APPLE_ID`,
   `APPLE_TEAM_ID` and `APPLE_APP_PASSWORD`. The workflow imports the certificate
   into a temporary keychain and deletes it afterwards.

### Linux and the release

Sign `SHA256SUMS` with the release OpenPGP key as described in
[packaging/README.md](../packaging/README.md#signing). To release: run **Draft
release** with the tag, download the draft's files, check that each build record
shows the expected signature state and that `SHA256SUMS` verifies, sign it, then
publish the draft by hand.

## Still to verify on real hardware

- Recording on Windows (microphone plus WASAPI loopback) and macOS (microphone
  plus Core Audio taps), and their permission prompts.
- Screen capture through `overtalk-capture` on both.
- Opening at sign-in: the Windows `Run` value (including when turned off in
  Settings, Apps, Startup) and the macOS LaunchAgent (listed under Login Items).
  Unit tests cover only the entry formats.
- The tray icon, notifications and file dialogs, including whether the macOS
  engine's dialogs come to the front with the Accessory policy.
- The browser bridge registration script on Windows and macOS.
- The Visual C++ runtime copied beside the app on a Windows install without the
  redistributable (hosted runners already have it).
