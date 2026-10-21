#!/usr/bin/env python3
"""Build Overtalk's Linux packages: a .deb, an AppImage and SHA256SUMS.

Release binaries are built in a dedicated Cargo target directory, so packaging
never invalidates a developer build. One install tree is staged, then wrapped
as a Debian package (system libraries, Depends from dpkg-shlibdeps) and as an
AppImage (libraries bundled, except those that must match the host). Another
edition supplies its own engine with --engine-manifest; its source never
enters this repository.
"""
import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import urllib.request

ROOT = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(ROOT / "scripts"))
from native_licenses import stage_licenses  # noqa: E402
from worker_build import verify_binary  # noqa: E402

APP_ID = "io.github.judeam.Overtalk"
HOMEPAGE = "https://github.com/judeam/overtalk"
# AppImage type 2 runtime, release 20251108. The digest is the one GitHub
# publishes for this asset; the release is also signed by the AppImage key
# 570C77ACEA40C0F1B758902CBF96CCA56490F695 (see packaging/README.md).
RUNTIME_URL = "https://github.com/AppImage/type2-runtime/releases/download/20251108/runtime-x86_64"
RUNTIME_SHA256 = "2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d"
# The UI and its helpers. The engine (sonefold-service) and llama-helper are
# built separately; every binary sits in one directory, where each finds the
# others.
OSS_BINARIES = {
    "overtalk": "overtalk-native",
    "overtalk-worker": "overtalk-worker-sandbox",
    "overtalk-capture": "overtalk-capture",
    "overtalk-browser-bridge": "overtalk-browser-bridge",
    # Command line and MCP access for scripts and AI assistants.
    "overtalk-cli": "overtalk-cli",
}
ICON_SIZES = [16, 24, 32, 48, 64, 128, 256, 512]
# Libraries the AppImage takes from the host, following the AppImage project's
# excludelist: the C and C++ runtimes, graphics drivers, display and sound
# server clients, and font configuration must match the running system.
HOST_LIBRARIES = re.compile(r"""^(
    ld-linux-x86-64|libc|libm|libdl|libpthread|librt|libresolv|libutil|libanl|libmvec|libnss_\w+|libBrokenLocale|libthread_db
  | libstdc\+\+|libgcc_s
  | libGL|libEGL|libGLX|libGLdispatch|libOpenGL|libGLESv2|libglapi|libgbm|libdrm|libvulkan
  | libX11|libX11-xcb|libxcb(-[\w-]+)?|libXau|libXdmcp|libXext|libXrender|libXi|libXcursor|libXdamage|libXfixes
  | libXcomposite|libXrandr|libXinerama|libxshmfence|libxkbcommon(-x11)?|libwayland-(client|cursor|egl|server)
  | libasound|libpulse(-simple|-mainloop-glib)?|libpulsecommon-[\d.]+|libpipewire-[\d.]+|libjack
  | libfontconfig|libfreetype|libharfbuzz|libexpat|libz|libuuid|libcom_err|libgpg-error|libusb-1\.0|libICE|libSM
  | libdbus-1|libsystemd|libudev|libselinux|libapparmor|libcap
)\.so(\.[\d.]+)?$""", re.X)

DESCRIPTION = """Meeting notebook that records, transcribes and keeps sourced notes
 Overtalk records microphone and call audio, transcribes it on this computer
 and writes notes that link back to the passage they came from. Meetings and
 settings are stored locally; speech and summary models are downloaded on
 first use. Cloud AI providers are optional. Telemetry and automatic updates
 are disabled."""

APPRUN = """#!/bin/sh
# AppImage entry point. The engine, sandbox helper and model helpers sit beside
# the UI in usr/bin and find each other there. Bundled libraries are reached
# through RUNPATH, so nothing here leaks into host programs such as ffmpeg or
# bwrap, which the engine runs from /usr/bin.
set -eu
here=$(dirname "$(readlink -f "$0")")
# `Overtalk.AppImage cli …` is overtalk-cli, for scripts and AI assistants.
if [ "${1:-}" = cli ]; then shift; exec "$here/usr/bin/overtalk-cli" "$@"; fi
[ -x /usr/bin/bwrap ] || echo "Overtalk: install bubblewrap; local models run in its sandbox." >&2
[ -x /usr/bin/ffmpeg ] || echo "Overtalk: install ffmpeg; recordings are written with it." >&2
# The engine links GTK through Tauri but never draws with it. Keep host GTK and
# GIO plugins, built against the host's libraries, away from the bundled ones.
unset GTK_MODULES GTK3_MODULES
export GIO_MODULE_DIR="$here/usr/lib/gio/modules"
exec "$here/usr/bin/overtalk" "$@"
"""


def run(command, **kwargs):
    print("+", " ".join(str(part) for part in command), flush=True)
    return subprocess.run([str(part) for part in command], check=True, **kwargs)


def output(command, **kwargs):
    return subprocess.run([str(part) for part in command], check=True, capture_output=True,
                          text=True, **kwargs).stdout.strip()


def digest(path):
    with path.open("rb") as handle:
        return hashlib.file_digest(handle, "sha256").hexdigest()


def git(repository, *args):
    return output(["git", "-C", repository, *args])


def cargo_env(target_dir, rustflags=None):
    env = dict(os.environ, CARGO_TARGET_DIR=str(target_dir))
    # tauri-build would copy the developer's sidecar seeds (for example a CUDA
    # llama-helper) into the target directory. This build stages its own.
    env["TAURI_CONFIG"] = json.dumps({"bundle": {"externalBin": []}}, separators=(",", ":"))
    # ggml otherwise compiles whisper.cpp with -march=native.
    env["CMAKE_PROJECT_INCLUDE_BEFORE"] = str(HERE / "portable-cpu.cmake")
    # Link ONNX Runtime statically, from ort's checksummed download or from a
    # supplied static library (offline builds), so no shared runtime has to be
    # shipped or found through LD_LIBRARY_PATH.
    location = Path(env.get("ORT_LIB_LOCATION", "/nonexistent"))
    unset = ["ORT_PREFER_DYNAMIC_LINK", "RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS"]
    if not (location / "libonnxruntime.a").is_file():
        unset += ["ORT_LIB_LOCATION", "ORT_SKIP_DOWNLOAD"]
    for name in unset:
        env.pop(name, None)
    # Keep the builder's home directory out of the binaries: Rust panic
    # locations and C/C++ assertions would otherwise embed it, and with it the
    # layout of the source trees.
    home = str(Path.home())
    env["RUSTFLAGS"] = " ".join(filter(None, [rustflags, f"--remap-path-prefix={home}=/build"]))
    for name in ["CFLAGS", "CXXFLAGS"]:
        env[name] = " ".join(filter(None, [env.get(name), f"-ffile-prefix-map={home}=/build"]))
    return env


def build(args):
    for name in ["OVERTALK_FFMPEG_PATH", "OVERTALK_FFMPEG_SHA256"]:
        if not os.environ.get(name):
            raise SystemExit(f"Set {name} first; see docs/BUILDING.md#verified-ffmpeg-input")
    if subprocess.run(["pkg-config", "--exists", "libpipewire-0.3"]).returncode != 0:
        raise SystemExit("overtalk-capture needs PipeWire headers: install libpipewire-0.3-dev "
                         "or point PKG_CONFIG_PATH at them")
    packages = [*OSS_BINARIES.values()]
    binaries = [*OSS_BINARIES]
    if not args.engine_manifest:
        packages.append("sonefold")
        binaries.append("sonefold-service")
    command = ["cargo", "build", "--release", "--locked"]
    for package in packages:
        command += ["-p", package]
    for binary in binaries:
        command += ["--bin", binary]
    run(command, cwd=ROOT, env=cargo_env(args.target_dir))
    if args.engine_manifest:
        run(["cargo", "build", "--release", "--locked", "--manifest-path", args.engine_manifest,
             "-p", args.engine_package, "--bin", "sonefold-service"],
            cwd=args.engine_manifest.parent, env=cargo_env(engine_target(args)))
    # llama.cpp picks its instruction set from Rust's target features. Without
    # this it is built for SSE2 only, which makes local summaries very slow.
    run(["cargo", "build", "--release", "--locked", "-p", "llama-helper"], cwd=ROOT,
        env=cargo_env(args.target_dir / "llama-helper", "-C target-cpu=x86-64-v3"))


def engine_target(args):
    return args.engine_manifest.parent / "target/package-linux"


def built_binaries(args):
    release = args.target_dir / "release"
    binaries = {name: release / name for name in OSS_BINARIES}
    engine = engine_target(args) if args.engine_manifest else args.target_dir
    binaries["sonefold-service"] = engine / "release/sonefold-service"
    binaries["llama-helper"] = args.target_dir / "llama-helper/release/llama-helper"
    for name, path in binaries.items():
        if not path.is_file():
            raise SystemExit(f"Missing {name}: {path}. Build without --skip-build first.")
    return binaries


def render_icons(svg, directory):
    try:
        import gi
        gi.require_version("GdkPixbuf", "2.0")
        from gi.repository import GdkPixbuf
    except (ImportError, ValueError):
        GdkPixbuf = None
    for size in ICON_SIZES:
        target = directory / f"{size}x{size}/apps/{APP_ID}.png"
        target.parent.mkdir(parents=True, exist_ok=True)
        if GdkPixbuf:
            GdkPixbuf.Pixbuf.new_from_file_at_size(str(svg), size, size).savev(str(target), "png", [], [])
        elif shutil.which("rsvg-convert"):
            run(["rsvg-convert", "-w", size, "-h", size, "-o", target, svg])
        else:
            raise SystemExit("Rendering icons needs python3-gi with GdkPixbuf, or rsvg-convert")
    scalable = directory / f"scalable/apps/{APP_ID}.svg"
    scalable.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(svg, scalable)


def stage(args, binaries, tree):
    """The install tree every format shares: bin/, lib/Sonefold/, share/.

    It is laid out as a prefix, so a Flatpak build stages it straight into /app.
    """
    (tree / "bin").mkdir(parents=True, exist_ok=True)
    for name, source in binaries.items():
        target = tree / "bin" / name
        shutil.copyfile(source, target)
        target.chmod(0o755)
        run(["strip", "--strip-unneeded", target])
    verify_binary(tree / "bin/overtalk-worker")
    # Tauri resolves the engine's resources as <exe>/../lib/Sonefold.
    run([sys.executable, ROOT / "scripts/stage-native-resources.py", tree])
    share = tree / "share"
    licenses = share / "licenses" / APP_ID
    stage_licenses(licenses)
    for source in sorted((HERE / "licenses").iterdir()):
        shutil.copyfile(source, licenses / source.name)
    # whisper.cpp and llama.cpp are compiled into the engine and the helper.
    metadata = json.loads(output(["cargo", "metadata", "--locked", "--format-version=1"], cwd=ROOT))
    for package in metadata["packages"]:
        vendored = {"whisper-rs-sys": "whisper.cpp", "llama-cpp-sys-2": "llama.cpp"}.get(package["name"])
        if vendored:
            notice = Path(package["manifest_path"]).parent / vendored / "LICENSE"
            shutil.copyfile(notice, licenses / f"{vendored}-LICENSE.txt")
    render_icons(ROOT / "frontend/public/brand/mark.svg", share / "icons/hicolor")
    (share / "applications").mkdir(exist_ok=True)
    desktop = share / f"applications/{APP_ID}.desktop"
    entry = (HERE / f"{APP_ID}.desktop").read_text()
    desktop.write_text(entry.replace("\nName=Overtalk\n", f"\nName={args.display_name}\n"))
    if shutil.which("desktop-file-validate"):
        run(["desktop-file-validate", desktop])
    (share / "metainfo").mkdir(exist_ok=True)
    metainfo = share / f"metainfo/{APP_ID}.metainfo.xml"
    shutil.copyfile(HERE / f"{APP_ID}.metainfo.xml", metainfo)
    if shutil.which("appstreamcli"):
        run(["appstreamcli", "validate", "--no-net", metainfo])


def normalise(root, epoch):
    """Fixed modes and times, so the same inputs give the same archive."""
    for path in sorted(root.rglob("*"), reverse=True):
        if not path.is_symlink():
            executable = path.is_dir() or os.access(path, os.X_OK)
            path.chmod(0o755 if executable else 0o644)
        os.utime(path, (epoch, epoch), follow_symlinks=False)
    os.utime(root, (epoch, epoch))


def shlib_depends(binaries):
    with tempfile.TemporaryDirectory() as directory:
        (Path(directory) / "debian").mkdir()
        (Path(directory) / "debian/control").write_text("Source: overtalk\n\nPackage: overtalk\nArchitecture: any\n")
        substvars = output(["dpkg-shlibdeps", "-O", *binaries], cwd=directory)
    return substvars.split("=", 1)[1]


def build_deb(args, tree, version, epoch, work):
    root = work / "deb"
    prefix = root / "usr/lib" / args.package
    shutil.copytree(tree / "bin", prefix / "bin")
    shutil.copytree(tree / "lib", prefix / "lib")
    (root / "usr/bin").mkdir(parents=True)
    (root / "usr/bin/overtalk").symlink_to(f"../lib/{args.package}/bin/overtalk")
    (root / "usr/bin/overtalk-cli").symlink_to(f"../lib/{args.package}/bin/overtalk-cli")
    share = root / "usr/share"
    for part in ["applications", "metainfo", "icons"]:
        shutil.copytree(tree / "share" / part, share / part)
    doc = share / "doc" / args.package
    shutil.copytree(tree / "share/licenses" / APP_ID, doc / "licenses")
    (doc / "copyright").write_text(
        "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\n"
        f"Upstream-Name: Overtalk\nSource: {HOMEPAGE}\n\n"
        "Files: *\nCopyright: 2024 Zackriya Solutions\n 2026 Jude Mingay\nLicense: MIT and Apache-2.0\n"
        f" See /usr/share/doc/{args.package}/licenses/LICENSE.md and NOTICE.md (voice memory\n"
        " is Apache-2.0). Third-party notices,\n"
        " including statically linked ONNX Runtime, whisper.cpp and llama.cpp,\n"
        f" are in /usr/share/doc/{args.package}/licenses/.\n")
    executables = sorted((prefix / "bin").iterdir())
    # Runtime programs the engine executes: the worker sandbox and the encoder.
    depends = f"{shlib_depends(executables)}, bubblewrap, ffmpeg"
    size = sum(path.stat().st_size for path in root.rglob("*") if path.is_file() and not path.is_symlink())
    fields = {
        "Package": args.package,
        "Version": version,
        "Architecture": "amd64",
        "Maintainer": args.maintainer,
        "Installed-Size": str((size + 1023) // 1024),
        "Depends": depends,
        # System audio is captured from PulseAudio monitor sources.
        "Recommends": "pipewire-pulse | pulseaudio",
        "Section": "sound",
        "Priority": "optional",
        "Homepage": HOMEPAGE,
    }
    if args.conflicts:
        fields.update(Conflicts=", ".join(args.conflicts), Replaces=", ".join(args.conflicts),
                      Provides=", ".join(args.conflicts))
    fields["Description"] = DESCRIPTION
    (root / "DEBIAN").mkdir()
    (root / "DEBIAN/control").write_text("".join(f"{key}: {value}\n" for key, value in fields.items()))
    normalise(root, epoch)
    deb = args.out / f"{args.package}_{version}_amd64.deb"
    run(["dpkg-deb", "--root-owner-group", "-Zxz", "--build", root, deb],
        env=dict(os.environ, SOURCE_DATE_EPOCH=str(epoch)))
    return deb, depends


def bundled_libraries(executables):
    libraries = {}
    for executable in executables:
        for line in output(["ldd", executable]).splitlines():
            if "not found" in line:
                raise SystemExit(f"{executable}: {line.strip()}")
            match = re.match(r"\s*(\S+) => (/\S+) \(0x", line)
            if match and not HOST_LIBRARIES.match(match[1]):
                libraries[match[1]] = Path(match[2]).resolve()
    return libraries


def appimage_runtime(args):
    path = args.appimage_runtime
    if path is None:
        cache = Path(os.environ.get("XDG_CACHE_HOME", Path.home() / ".cache")) / "overtalk-packaging"
        path = cache / f"appimage-runtime-{RUNTIME_SHA256[:16]}"
        if not path.is_file():
            cache.mkdir(parents=True, exist_ok=True)
            with tempfile.NamedTemporaryFile(dir=cache, delete=False) as handle, \
                    urllib.request.urlopen(RUNTIME_URL, timeout=60) as response:
                shutil.copyfileobj(response, handle)
            if digest(Path(handle.name)) != RUNTIME_SHA256:
                Path(handle.name).unlink()
                raise SystemExit("AppImage runtime checksum mismatch; refusing to use it")
            os.replace(handle.name, path)
    if digest(path) != RUNTIME_SHA256:
        raise SystemExit(f"{path} is not AppImage runtime 20251108 (checksum mismatch)")
    return path


def build_appimage(args, tree, version, epoch, work):
    appdir = work / "AppDir"
    usr = appdir / "usr"
    shutil.copytree(tree / "bin", usr / "bin")
    shutil.copytree(tree / "lib", usr / "lib")
    shutil.copytree(tree / "share", usr / "share")
    executables = sorted((usr / "bin").iterdir())
    libraries = bundled_libraries(executables)
    for name, source in sorted(libraries.items()):
        target = usr / "lib" / name
        shutil.copyfile(source, target)
        run(["patchelf", "--set-rpath", "$ORIGIN", target])
    for executable in executables:
        run(["patchelf", "--set-rpath", "$ORIGIN/../lib", executable])
    (usr / "lib/gio/modules").mkdir(parents=True)
    (appdir / "AppRun").write_text(APPRUN)
    (appdir / "AppRun").chmod(0o755)
    shutil.copyfile(usr / f"share/applications/{APP_ID}.desktop", appdir / f"{APP_ID}.desktop")
    icon = usr / f"share/icons/hicolor/256x256/apps/{APP_ID}.png"
    shutil.copyfile(icon, appdir / f"{APP_ID}.png")
    shutil.copyfile(icon, appdir / ".DirIcon")
    normalise(appdir, epoch)
    image = work / "squashfs"
    run(["mksquashfs", appdir, image, "-root-owned", "-noappend", "-no-xattrs", "-comp", "zstd",
         "-mkfs-time", epoch, "-all-time", epoch, "-quiet", "-no-progress"],
        # mksquashfs refuses these options when SOURCE_DATE_EPOCH is also set.
        env={name: value for name, value in os.environ.items() if name != "SOURCE_DATE_EPOCH"})
    name = args.display_name.replace(" ", "-")
    target = args.out / f"{name}-{version.replace('~', '-')}-x86_64.AppImage"
    with target.open("wb") as handle:
        for part in [appimage_runtime(args), image]:
            with part.open("rb") as source:
                shutil.copyfileobj(source, handle)
    target.chmod(0o755)
    return target, sorted(libraries)


def sign(args, artifacts):
    sums = args.out / "SHA256SUMS"
    sums.write_text("".join(f"{digest(path)}  {path.name}\n" for path in artifacts))
    key = args.sign_key or os.environ.get("OVERTALK_SIGNING_KEY")
    if key:
        run(["gpg", "--batch", "--yes", "--local-user", key, "--armor", "--detach-sign", sums])
    else:
        print("No signing key given: SHA256SUMS is unsigned. See packaging/README.md#signing.")
    return sums


def main():
    os.umask(0o022)
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--formats", default="deb,appimage", help="Comma-separated: deb, appimage")
    parser.add_argument("--out", type=Path, default=ROOT / "target/package-linux/dist")
    parser.add_argument("--target-dir", type=Path, default=ROOT / "target/package-linux")
    parser.add_argument("--skip-build", action="store_true", help="Package the binaries already built")
    parser.add_argument("--engine-manifest", type=Path, help="Cargo.toml of a workspace providing another sonefold-service")
    parser.add_argument("--engine-package", default="sonefold", help="Package in --engine-manifest that builds sonefold-service")
    parser.add_argument("--package", default="overtalk", help="Debian package name")
    parser.add_argument("--display-name", default="Overtalk")
    parser.add_argument("--conflicts", action="append", default=[], help="Package this one conflicts with and replaces")
    parser.add_argument("--version", help="Package version; default <crate version>~git<date>.<commit>")
    parser.add_argument("--maintainer", default=os.environ.get("OVERTALK_MAINTAINER", "Jude Mingay <judeam@users.noreply.github.com>"))
    parser.add_argument("--appimage-runtime", type=Path, help="Verified type2 runtime; downloaded and checked if omitted")
    parser.add_argument("--sign-key", help="GPG key that signs SHA256SUMS (or set OVERTALK_SIGNING_KEY)")
    parser.add_argument("--stage", type=Path, help="Only stage the install tree into this prefix (the Flatpak build uses /app)")
    args = parser.parse_args()
    args.engine_manifest = args.engine_manifest.resolve() if args.engine_manifest else None
    args.target_dir = args.target_dir.resolve()
    formats = set(args.formats.split(","))
    if not formats <= {"deb", "appimage"}:
        raise SystemExit(f"Unknown format in {args.formats}")
    if not args.skip_build:
        build(args)
    binaries = built_binaries(args)
    if args.stage:
        stage(args, binaries, args.stage.resolve())
        return
    epoch = int(os.environ.get("SOURCE_DATE_EPOCH") or git(ROOT, "log", "-1", "--format=%ct"))
    commit = git(ROOT, "rev-parse", "--short=10", "HEAD")
    if not args.version:
        crate = json.loads(output(["cargo", "metadata", "--locked", "--no-deps", "--format-version=1"], cwd=ROOT))
        base = next(p["version"] for p in crate["packages"] if p["name"] == "overtalk-native")
        date = datetime.datetime.fromtimestamp(epoch, datetime.timezone.utc).strftime("%Y%m%d")
        args.version = f"{base}~git{date}.{commit}"
        if args.engine_manifest:
            args.version += f"+engine.{git(args.engine_manifest.parent, 'rev-parse', '--short=10', 'HEAD')}"
    args.out = args.out.resolve()
    args.out.mkdir(parents=True, exist_ok=True)
    artifacts = []
    info = {"version": args.version, "package": args.package, "source_date_epoch": epoch,
            "commit": git(ROOT, "rev-parse", "HEAD"),
            "dirty": bool(git(ROOT, "status", "--porcelain", "--untracked-files=no")),
            "rustc": output(["rustc", "--version"]),
            "ffmpeg": "host package (Depends: ffmpeg); not bundled",
            "binaries": {name: digest(path) for name, path in binaries.items()}}
    if args.engine_manifest:
        repository = args.engine_manifest.parent
        info["engine"] = {"commit": git(repository, "rev-parse", "HEAD"),
                          "dirty": bool(git(repository, "status", "--porcelain", "--untracked-files=no"))}
    with tempfile.TemporaryDirectory(prefix="overtalk-package-", dir=args.out.parent) as temporary:
        work = Path(temporary)
        tree = work / "tree"
        stage(args, binaries, tree)
        if "deb" in formats:
            deb, depends = build_deb(args, tree, args.version, epoch, work)
            artifacts.append(deb)
            info["deb"] = {"file": deb.name, "depends": depends}
        if "appimage" in formats:
            appimage, libraries = build_appimage(args, tree, args.version, epoch, work)
            artifacts.append(appimage)
            info["appimage"] = {"file": appimage.name, "runtime_sha256": RUNTIME_SHA256,
                                "bundled_libraries": libraries}
    build_info = args.out / f"{args.package}-build-info.json"
    build_info.write_text(json.dumps(info, indent=2) + "\n")
    sums = sign(args, [*artifacts, build_info])
    for path in [*artifacts, build_info, sums]:
        print(f"{path}  {path.stat().st_size / 1e6:.1f} MB")


if __name__ == "__main__":
    main()
