# Build Overtalk

The default desktop interface is native Slint (`native/`, binary `overtalk`). It runs the Rust service `sonefold-service` (crate `sonefold` in `frontend/src-tauri/`) as a private child process. The Next.js/Tauri interface in `frontend/src/` is a supported alternative that uses the same service code. The optional Python speech worker runs separately. See [native UI builds and validation](NATIVE-UI.md) for the Slint development commands.

## Prerequisites

Install Rust 1.94 or newer (SQLx 0.9 requires it; CI uses 1.94) and Python 3.11 or newer for the build scripts. Ubuntu's apt `rustc` is older than the 1.94 minimum, so install Rust with rustup:

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

The web interface also needs Node 20 or newer, pnpm 9.15.9 and Bun for its tests. Use Python 3.12 for optional speech environments.

On Ubuntu 24.04, install the native development libraries and the runtime tools the engine uses:

```sh
sudo apt install build-essential pkg-config clang libclang-dev cmake ninja-build \
  libwebkit2gtk-4.1-dev libgtk-3-dev libxdo-dev libssl-dev librsvg2-dev \
  libasound2-dev libpulse-dev libpipewire-0.3-dev libspa-0.2-dev \
  libfontconfig1-dev libxkbcommon-dev libwayland-dev libinput10 libgbm1 libudev1 \
  bubblewrap ffmpeg
```

This list mirrors the Linux CI job's packages plus the compilers its hosted runner already has. It has not been checked on a clean install. CI also installs `xvfb dbus-x11 espeak-ng pulseaudio strace` for its integration checks. The web interface's AppImage also needs `gstreamer1.0-plugins-base` and `gstreamer1.0-plugins-good`; the `openblas` feature needs `libopenblas-dev`. Packaging inputs are listed in [packaging/README.md](../packaging/README.md).

macOS requires Xcode command-line tools. Windows requires Visual Studio C++ build tools and WebView2. CPU builds are the portable default; CUDA/Vulkan/Metal availability depends on the platform and toolchain. [Platforms](PLATFORMS.md) records what is verified on Linux, Windows and macOS, how capture and worker isolation work on each, and how to build and sign their packages.

## Build and run from source (Linux)

This sequence follows the Linux job in `.github/workflows/ci.yml`. It builds a CPU-only native app; for GPU builds see [GPU notes](GPU_ACCELERATION.md#choosing-a-backend).

```sh
git clone https://github.com/judeam/overtalk
cd overtalk

# Record the package-manager FFmpeg for the build (see "Verified FFmpeg input").
set -a; eval "$(python3 scripts/configure-ffmpeg-build.py)"; set +a

# Build the service without the web bundle's sidecars and frontend.
export TAURI_CONFIG='{"build":{"frontendDist":"service-assets"},"bundle":{"externalBin":[],"resources":{}}}'

cargo build --release --locked \
  -p overtalk-native -p sonefold -p overtalk-capture -p overtalk-browser-bridge -p overtalk-worker-sandbox \
  --bin overtalk --bin sonefold-service --bin overtalk-capture --bin overtalk-browser-bridge --bin overtalk-worker

# The local notes helper, for built-in notes models.
cargo build --release --locked -p llama-helper

# Optional: speech-worker and companion resources beside the release binaries.
python3 scripts/stage-native-resources.py --into target/release

cargo run --release --locked -p overtalk-native
```

A release build of `overtalk` starts the `sonefold-service` beside itself; a debug build also falls back to `target/release/sonefold-service`. The service finds `overtalk-worker` and `llama-helper` in its own folder. To build `.deb` and AppImage packages instead, run `python3 packaging/linux/build.py` with the same FFmpeg variables ([packaging/README.md](../packaging/README.md)).

### Environment variables

| Variable | Used by | Purpose |
| --- | --- | --- |
| `OVERTALK_FFMPEG_PATH`, `OVERTALK_FFMPEG_SHA256` | `sonefold` build | Verified FFmpeg the build stages for the Tauri bundle. Required. |
| `TAURI_CONFIG` | `sonefold` build | JSON merged into `tauri.conf.json`. The value above drops the web bundle's sidecars and frontend for service-only builds. |
| `TAURI_GPU_FEATURE` | `frontend/dev-gpu.sh`, `build-gpu.sh`, `pnpm tauri:dev`/`tauri:build` | Overrides GPU detection (`cuda`, `vulkan`, `hipblas`, `metal`, `coreml`, `openblas`, `openmp`; `none` for CPU). |
| `CMAKE_CUDA_ARCHITECTURES` | CUDA builds | Target compute capability; the scripts default to `native`. |
| `ORT_LIB_LOCATION` | `sonefold` build | An installed ONNX Runtime to link instead of the archive downloaded during the build. |
| `ORT_DYLIB_PATH` | Windows tests | `onnxruntime.dll` for the dynamically loaded ONNX Runtime; CI sets it for Windows test binaries. |
| `OVERTALK_SERVICE` | `overtalk` | Service binary the UI starts. Ignored inside a macOS `.app` bundle. |
| `SONEFOLD_LLAMA_HELPER` | `sonefold-service` | Absolute path to the `llama-helper` executable. |
| `OLLAMA_HOST` | `sonefold-service` | Ollama address used by **Settings → Experimental → GPU memory** to list and release Ollama models; defaults to `http://127.0.0.1:11434`. The notes provider's address is set in Settings. |
| `OVERTALK_GOOGLE_CLIENT_ID`, `OVERTALK_MICROSOFT_CLIENT_ID`, `OVERTALK_SLACK_CLIENT_ID`, `OVERTALK_GITHUB_CLIENT_ID` | build or launch environment | Public OAuth client IDs for account linking; see [account linking](CONNECTION-ACCOUNT-LINKING.md). |
| `OVERTALK_GPU_LAUNCHER` | `sonefold-service` | Optional launcher that GPU workers run under; see [GPU notes](GPU_ACCELERATION.md#sharing-the-gpu-with-the-desktop). |

## Web interface

```sh
cd frontend
pnpm install --frozen-lockfile
./dev-gpu.sh
```

Set `NEXT_TELEMETRY_DISABLED=1` in your shell to turn off the Next.js CLI's telemetry; CI sets it. `dev-gpu.sh` detects the GPU, builds the matching `llama-helper`, copies it to `frontend/src-tauri/binaries/` and runs `pnpm run tauri:dev`; `build-gpu.sh` does the same for a production build. The Tauri bundle lists `llama-helper`, `overtalk-worker` and `ffmpeg` as sidecars with Tauri's target-triple suffix in `frontend/src-tauri/binaries/`. Tauri's `beforeDevCommand`/`beforeBuildCommand` stage `overtalk-worker` (`scripts/worker_build.py`), and the `sonefold` build script stages FFmpeg from `OVERTALK_FFMPEG_PATH`, so set the FFmpeg variables first. The directory is Git-ignored; do not commit local executables.

`pnpm dev` starts a browser development server. Audio capture, SQLite, model management and native IPC require `pnpm tauri dev`; an ordinary browser does not provide those APIs.

## GPU builds

The launcher respects `CMAKE_CUDA_ARCHITECTURES`; the default `native` compiles for a detected NVIDIA device. Cross-building without a GPU requires an explicit architecture. The tested NVIDIA RTX 5090 Laptop GPU uses `120` and a compatible CUDA/toolchain installation:

```sh
CMAKE_POSITION_INDEPENDENT_CODE=ON CMAKE_CUDA_ARCHITECTURES=120 \
  pnpm tauri build --features cuda
```

For the native app, pass `--features cuda` to the `sonefold` and `llama-helper` builds instead ([GPU notes](GPU_ACCELERATION.md#choosing-a-backend)).

On a machine with mixed GCC versions, use a CUDA-compatible host compiler and make its C++ development libraries visible to the Rust linker. For example, when the default `cc` is GCC 14 without `libstdc++.so` and CUDA needs GCC 13, set `LIBRARY_PATH=/usr/lib/gcc/x86_64-linux-gnu/13`. Confirm the path with `g++-13 -print-file-name=libstdc++.so`; do not substitute a path from another platform.

Whisper's static CUDA objects must use position-independent code when linked into the Tauri shared library. Keep `CMAKE_POSITION_INDEPENDENT_CODE=ON` for this Linux CUDA build; changing this setting requires rebuilding `whisper-rs-sys`.

To keep the desktop responsive during GPU inference, workers can run under a GPU-sharing launcher named by `OVERTALK_GPU_LAUNCHER`. See [GPU notes](GPU_ACCELERATION.md) and [speech worker setup](../speech-worker/README.md). Native Parakeet INT8 uses ONNX Runtime on CPU; the advanced Python engines are independent of this build feature.

## Validate

Run `pnpm typecheck`, `pnpm test` and `pnpm build` in `frontend/`. From the root, with the build environment above, run worker and companion unit tests and `cargo test -p sonefold --lib`. ONNX Runtime may download during an initial build; an installed compatible runtime can be supplied with `ORT_LIB_LOCATION` and the platform dynamic-library path. The full list of local checks is in [AGENTS.md](../AGENTS.md#style-and-verification).

The native UI binary is `overtalk`; its service is `sonefold-service` from the
`sonefold` package. The web/Tauri binary is `sonefold`. Existing storage
uses application identifier `com.judeam.sonefold`; both interfaces reuse that data.

## Packaging

The native packages come from `packaging/linux/build.py` (.deb, AppImage), `packaging/windows/build.py` (per-user installer, portable zip) and `packaging/macos/build.py` (.app in a .dmg); see [packaging/README.md](../packaging/README.md) and [Platforms](PLATFORMS.md#building-the-packages). `pnpm tauri build` still creates the web interface's local packages after sidecars are prepared. Automatic updates are disabled. Pushing a version tag publishes the packages on GitHub Releases (`.github/workflows/release.yml`; `python scripts/release.py <version>` makes the tag). The CI workflows build the Slint interface and Rust engine on Linux, Windows and macOS, package and smoke-test all three, check the web interface and Python workers, and run dependency/security and performance regressions. Signing needs release certificates ([Platforms](PLATFORMS.md#signing)); hardware audio paths on Windows and macOS still require validation on real machines.

Record platform-specific failures and skipped hardware tests. [The dated validation record](research/2026-09-30-validation.md) documents what has actually passed on the development machine.

## Linux startup compatibility (web interface)

On the tested NVIDIA system, WebKitGTK 2.52.6 needed isolated caches and software UI painting. `scripts/run-installed-linux.sh` applies both. It runs an unpacked AppImage installed under `~/.local/opt/sonefold/app`, or the `AppRun` named by `SONEFOLD_APP_RUN`. This changes only the app's process environment; speech and language models keep their GPU acceleration, and the script runs the app under `OVERTALK_GPU_LAUNCHER` when that is set.

The launcher gives the app `~/.cache/sonefold/runtime` (or that suffix beneath a custom `XDG_CACHE_HOME`). This avoids the malformed shared font-cache patterns described in [WebKit bug 325185](https://bugs.webkit.org/show_bug.cgi?id=325185), which caused an infinite initialization loop on the tested machine. Meetings, settings, checkpoints and system font files remain untouched. The NVIDIA painting workaround uses `WEBKIT_DISABLE_DMABUF_RENDERER=1` and `WEBKIT_DISABLE_COMPOSITING_MODE=1`; both were needed for a rendered view there. On another graphics stack, set these variables to `0` when testing normal painting.

The external Python worker ignores AppImage-provided `PYTHONHOME`/`PYTHONPATH` and bundled library directories before invoking the selected virtualenv. Its startup errors distinguish missing runtime dependencies from oversized protocol responses. See [the current validation checkpoint](research/2026-09-30-validation.md#final-library-and-quality-profile-checkpoint) for packaged-worker evidence.

Linux AppImages include Tauri’s media framework (`bundle.linux.appimage.bundleMediaFramework=true`) for the recording player. Without its GStreamer plugins, entering a meeting with audio can fail in the webview. Verify playback against a generated audio fixture after packaging; frontend tests alone do not exercise these native plugins.


### Verified FFmpeg input

Cargo build scripts never download or unpack FFmpeg. On Linux install it through
your distribution; the Windows and macOS packages bundle the LGPL FFmpeg that
`packaging/ffmpeg/build.py` compiles from digest-pinned FFmpeg and libopus source
([packaging/README.md](../packaging/README.md#ffmpeg-on-windows-and-macos)).
Otherwise verify a publisher's immutable release and its independently obtained
SHA-256 checksum. Then set both
`OVERTALK_FFMPEG_PATH` (the executable) and `OVERTALK_FFMPEG_SHA256` before Cargo.
An absent or mismatched checksum fails the build before execution. The binary
is copied with a bounded stream and rechecked before bundling.

`scripts/configure-ffmpeg-build.py` prints both variables for the `ffmpeg` on
`PATH`, or for `--binary /absolute/path/to/ffmpeg`. Export them in sh, bash or zsh with:

```sh
set -a; eval "$(python3 scripts/configure-ffmpeg-build.py)"; set +a
```

Use it only for an **already verified** binary. Computing a checksum from an
untrusted download does not authenticate that download. For Windows and macOS, CI
builds FFmpeg in `.github/workflows/ffmpeg.yml` and passes it with
`--binary target/ffmpeg-<os>/ffmpeg`; Linux jobs use the distribution package.
The release packager must include required shared libraries; Homebrew and
gyan.dev binaries are not standalone, redistributable artifacts. The checksum
changes whenever the binary is rebuilt, so run the script again afterwards.

### Isolated inference and media workers

Native releases include `overtalk-worker` beside `sonefold-service`, the UI and
other helpers. On Linux install `bubblewrap` (Ubuntu/Debian: `sudo apt install
bubblewrap`) and keep unprivileged user namespaces available. Worker launch fails
closed if the OS cannot apply isolation. Do not bypass it to troubleshoot models.
Ubuntu 23.10 and later, including 24.04, restrict unprivileged user namespaces
through AppArmor (`kernel.apparmor_restrict_unprivileged_userns=1`), which can stop
bubblewrap; workers then fail closed. The `.deb` ships an AppArmor profile for
`overtalk-worker` that allows `userns`; for other installs, setting that sysctl to
`0` works but weakens a system-wide hardening. On GitHub's stock Ubuntu 24.04 runner
bubblewrap stops with `loopback: Failed RTM_NEWADDR: Operation not permitted`; the
`ci.yml` tests use the sysctl, the package workflow tests the profile. See
[Platforms](PLATFORMS.md#ubuntu-and-apparmor).
Windows uses AppContainer; macOS uses Seatbelt. The CI workflow is configured to
run the worker's file, environment and network denial probes on all three platforms. See
[worker isolation](WORKER-ISOLATION.md) for scope and limitations.

## Code quality

The workspace and native UI declare Rust 1.94 or newer; current CI uses Rust 1.94.
See [the codebase map](CODEBASE-MAP.md) and [quality tools](QUALITY-TOOLS.md) before changing shared modules. Run web tests with `pnpm test` to preserve process isolation.
