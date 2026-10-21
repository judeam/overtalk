# Linux packaging

`packaging/linux/build.py` builds the native desktop app (the Slint UI
`overtalk`, the engine `sonefold-service` and their helpers) and produces a
Debian package, an AppImage, a build record and `SHA256SUMS`. The Flatpak
manifest in `packaging/flatpak/` reuses the same staging step.

Nothing here publishes or uploads anything.

## Build the .deb and AppImage

Build inputs (Ubuntu 24.04 names): Rust 1.94 or newer, `cmake`, `clang`/`libclang-dev`,
`pkg-config`, `libgtk-3-dev`, `libwebkit2gtk-4.1-dev`, `libsoup-3.0-dev`,
`libasound2-dev`, `libpulse-dev`, `libpipewire-0.3-dev`, `dpkg-dev`,
`squashfs-tools`, `patchelf`, and `python3-gi` (or `rsvg-convert`) for icons.
`appstream` and `desktop-file-utils` are used for validation when present.

```sh
set -a; eval "$(python3 scripts/configure-ffmpeg-build.py)"; set +a   # a verified FFmpeg; see docs/BUILDING.md
nice -n 10 python3 packaging/linux/build.py
```

The results are written to `target/package-linux/dist/`:

| File | Contents |
| --- | --- |
| `overtalk_<version>_amd64.deb` | Installs to `/usr/lib/overtalk/{bin,lib/Sonefold}`, `/usr/bin/overtalk`, `/usr/bin/overtalk-cli`, desktop entry, metainfo, icons, `/usr/share/doc/overtalk/` |
| `Overtalk-<version>-x86_64.AppImage` | The same tree under `usr/`, plus the libraries it needs. `Overtalk-….AppImage cli …` runs `overtalk-cli` |
| `overtalk-build-info.json` | Commit, dirty flag, rustc, binary hashes, Depends, bundled libraries |
| `SHA256SUMS` (and `SHA256SUMS.asc` when signed) | Checksums of the three files above |

Useful options: `--formats deb` or `--formats appimage`, `--skip-build` to repackage the
binaries already built, `--version`, `--out`, and `--sign-key` (see [Signing](#signing)).
The version defaults to `<crate version>~git<commit date>.<commit>`, naming the
checkout at packaging time, so build releases from a clean checkout without `--skip-build`.
`SOURCE_DATE_EPOCH` defaults to the commit time; file modes and times are normalised.

### How the binaries are built

- Cargo uses its own target directory, `target/package-linux`, so packaging never
  rebuilds a developer's `target/release`.
- `TAURI_CONFIG` empties the Tauri `externalBin` list, so the developer's sidecar seeds
  (for example a CUDA `llama-helper`) are not copied in.
- `portable-cpu.cmake` turns off ggml's `-march=native`, so whisper.cpp targets
  AVX, AVX2, FMA and F16C. `llama-helper` is built with `-C target-cpu=x86-64-v3` for the
  same baseline; without it llama.cpp is built for SSE2 only. **The packages need an
  x86-64-v3 CPU (Intel Haswell or AMD Excavator/Zen, 2013 onwards).**
- ONNX Runtime 1.22.0 is linked statically from the archive ort-sys downloads and checks,
  so no `libonnxruntime.so` or `LD_LIBRARY_PATH` is needed. A supplied
  `ORT_LIB_LOCATION` is kept only if it contains `libonnxruntime.a`.
- The builder's home directory is mapped to `/build` (`--remap-path-prefix` for Rust,
  `-ffile-prefix-map` for C/C++), so panic and assertion locations do not reveal
  local paths or how the source trees are laid out.
- The packages are CPU-only. GPU builds (CUDA/Vulkan) are not packaged yet.

### Runtime requirements

- **.deb:** Ubuntu 24.04 or newer, or Debian 13 or newer. Depends comes from
  `dpkg-shlibdeps` over the shipped binaries (it uses the `t64` library names), plus
  the display, input and GL libraries the window loads at run time (X11/xcb, Wayland,
  xkbcommon, EGL/GL), and `bubblewrap` and `ffmpeg`, which the engine runs. It also recommends `pipewire-pulse | pulseaudio`.
- **AppImage:** glibc 2.39 or newer and libstdc++ from GCC 12 or newer, `/usr/bin/bwrap`, `/usr/bin/ffmpeg`,
  and FUSE (or run it with `--appimage-extract-and-run`). Bundled libraries are found
  through RUNPATH, so nothing leaks into host programs. These come from the host: the C/C++ runtimes,
  GL/EGL/DRM/Vulkan, X11/xcb/Wayland/xkbcommon, ALSA/PulseAudio/PipeWire,
  fontconfig/freetype/harfbuzz, D-Bus and systemd (see `HOST_LIBRARIES`).
- Local models run in the bubblewrap worker sandbox, so unprivileged user namespaces
  must be available ([BUILDING.md](../docs/BUILDING.md#isolated-inference-and-media-workers)).
  Ubuntu 23.10 and later, including 24.04, restrict them through AppArmor
  (`kernel.apparmor_restrict_unprivileged_userns=1`), which can stop bubblewrap;
  workers then fail closed. The fixes are an AppArmor profile for `/usr/bin/bwrap`
  that allows `userns`, or setting that sysctl to `0`, which weakens a system-wide
  hardening. The restriction is confirmed on GitHub's stock Ubuntu 24.04 runner
  ([Platforms](../docs/PLATFORMS.md#ubuntu-and-apparmor)); packages for Ubuntu need
  to handle it before release.

### FFmpeg

The engine runs `ffmpeg` beside itself first, then `/usr/bin`, then `/usr/local/bin`
(`frontend/src-tauri/src/audio/ffmpeg.rs`). It uses it to join recording checkpoints,
decode imports and export video. The packages do **not** bundle it.
The .deb depends on the distribution's `ffmpeg`. The AppImage uses the host's
(its `AppRun` warns if it is missing). The Flatpak runtime ships `/usr/bin/ffmpeg`.
Bundling Ubuntu's build would mean about 100 shared libraries, GPL source-offer duties
for its x264/x265 parts, and patent-encumbered codecs, without distribution security
updates. `OVERTALK_FFMPEG_PATH`/`OVERTALK_FFMPEG_SHA256` are still needed at build time,
because the engine's build script verifies the FFmpeg it stages for the Tauri bundle.

### What the first run downloads

Launching downloads nothing, and nothing is contacted on start-up. The engine
has no update check and no analytics. Recording, transcription and
notes need two models. The user downloads each from Setup or Settings, and
every download is pinned to a revision and checksummed:

| Purpose | Choice | Size |
| --- | --- | --- |
| Speech to text | Parakeet TDT 0.6B v3 INT8 (ONNX), the engine's default when none is chosen. From `huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx` | 670 MB |
| Notes | A built-in GGUF model. **Download models** in the setup checklist starts with Qwen 3.5 2B (1,221 MB) unless a notes provider or downloaded model is already chosen; Gemma 3 1B (1,019 MB) and Qwen 3.5 4B (2,614 MB) can be chosen in Settings | 1.0 to 2.6 GB |

**Download models** in the setup checklist starts both downloads at once and shows
their progress; **Finish setup** is available while they run. A meeting recorded
before the speech model arrives is saved as audio with a calm notice ("the speech
model is still downloading"), queued for transcription, and transcribed with the
configured local engine as soon as the model is ready; its notes follow when
automatic notes are on. The setup check reports a missing speech or notes model.
The usage report in Privacy settings records how long this took (first open to
first recording, download minutes, first recording to first transcript), on the
computer only.
Voice activity detection (Silero) is compiled in. Voice memory (about 28 MB),
Whisper, Gemma 4, the 27B profiles and the Python speech worker are used only
when chosen. Ollama and cloud providers need no download. A first run therefore
fetches about 1.7 to 3.3 GB.

## Signing

`build.py --sign-key <fingerprint>` (or `OVERTALK_SIGNING_KEY`) writes a detached,
ASCII-armoured `SHA256SUMS.asc`. Without a key, only `SHA256SUMS` is written. The
maintainer's steps:

1. Create an OpenPGP key reserved for releases, ideally an offline primary key with a signing subkey.
   Publish its fingerprint on the website and in this README.
2. Build with `--sign-key <fingerprint>`.
3. Users verify with `gpg --verify SHA256SUMS.asc SHA256SUMS && sha256sum -c --ignore-missing SHA256SUMS`.

An apt repository would sign its `Release` file instead. An embedded AppImage
signature (`appimagetool --sign`) is not produced.

The AppImage runtime is type2-runtime release `20251108`. `build.py` pins its
SHA-256 (the digest GitHub publishes for the asset) and downloads it to
`~/.cache/overtalk-packaging` when `--appimage-runtime` is not given. Its detached
signature, by the AppImage key `570C77ACEA40C0F1B758902CBF96CCA56490F695`, was checked
with `gpgv` when the pin was chosen.

## Flatpak

`packaging/flatpak/io.github.judeam.Overtalk.yml` targets `org.gnome.Platform//51`.
That runtime provides WebKitGTK 4.1 for the engine, and its freedesktop 26.08 base
provides `/usr/bin/ffmpeg`. The build runs `build.py --stage /app`.

Permissions: `--socket=wayland`, `--socket=fallback-x11`, `--share=ipc`, `--device=dri`
(display); `--socket=pulseaudio` (microphone and the monitor sources used for call audio);
`--share=network` (model downloads, optional providers, local Ollama);
`--filesystem=xdg-documents/sonefold-recordings:create` (the default recordings folder
only); `--talk-name=org.freedesktop.secrets` (provider credentials);
`--talk-name=org.freedesktop.Notifications` and `org.kde.StatusNotifierWatcher`
(notifications and tray). No home, host or device-wide access.

To build it:

```sh
flatpak install flathub org.flatpak.Builder org.gnome.Sdk//51 org.gnome.Platform//51 \
    org.freedesktop.Sdk.Extension.rust-stable//26.08 org.freedesktop.Sdk.Extension.llvm22//26.08
# flatpak-cargo-generator.py from flatpak/flatpak-builder-tools (pin a commit)
python3 flatpak-cargo-generator.py Cargo.lock -o packaging/flatpak/cargo-sources.json
flatpak run org.flatpak.Builder --user --install --force-clean build-dir packaging/flatpak/io.github.judeam.Overtalk.yml
```

Before a Flathub submission:

1. **The worker sandbox cannot start inside Flatpak.** There is no `bwrap` in the runtime,
   and Flatpak's seccomp filter denies new user namespaces. In freedesktop 25.08,
   `unshare --user` fails with "Operation not permitted". Every local model and FFmpeg run
   would fail closed. `crates/worker-sandbox` needs a Flatpak backend, for example
   `flatpak-spawn --sandbox` with no network and only the model and work directories exposed.
2. **File dialogs are not portals.** `tauri-plugin-dialog` uses its default `gtk3`
   backend (a GtkFileChooserDialog), which cannot reach files outside the sandbox. Use
   `default-features = false, features = ["xdg-portal"]`, then check that exports, which
   write a temporary file beside the target and rename it, work through the document portal.
3. Set the Wayland app id (`slint::set_xdg_app_id("io.github.judeam.Overtalk")`) so
   desktops match the window to its desktop entry.
4. The browser bridge registers native-messaging hosts in browser profiles, which the
   sandbox cannot reach. Hide it or document it as unavailable.
5. Push `main` so the metainfo screenshot URLs resolve, and replace the `dir` source
   with the release tag and commit.

Splitting `sonefold-service` from the Tauri crate would remove WebKitGTK and GTK
from every package. The .deb's Depends would shrink, the AppImage would shed about
150 MB of libraries, and the Flatpak could use the smaller freedesktop runtime.

## Windows and macOS

`packaging/windows/build.py` (a per-user NSIS installer and a portable zip) and
`packaging/macos/build.py` (Overtalk.app in a `.dmg`) follow the same pattern as
this script and share `packaging/common.py`: a dedicated target folder, one
staged tree, a build record and `SHA256SUMS`. Their layout, prerequisites,
bundled components and signing steps are in
[docs/PLATFORMS.md](../docs/PLATFORMS.md#building-the-packages).
`.github/workflows/package.yml` builds all three and smoke-tests the packaged
engine; `release.yml` attaches them to a draft release.

`overtalk-cli` ships in every package: `overtalk-cli.exe` in the Windows program
folder and `Overtalk.app/Contents/MacOS/overtalk-cli` on macOS.
