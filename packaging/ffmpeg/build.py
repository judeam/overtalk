#!/usr/bin/env python3
"""Build the LGPL FFmpeg that the Windows and macOS packages bundle.

FFmpeg and libopus are compiled from source tarballs pinned by SHA-256, as one
static, standalone ffmpeg(.exe) that links only to the operating system's own
libraries. No GPL or non-free component is enabled, so the binary is licensed
under the LGPL 2.1 or later and can ship with the exact corresponding source,
which this script also leaves beside the binary:

    <out>/ffmpeg(.exe)             the program
    <out>/ffmpeg-build.json        versions, digests, configure line, binary digest
    <out>/licenses/                FFmpeg's COPYING.LGPLv2.1 and libopus's COPYING
    <out>/source/                  the two verified tarballs

The packagers read that folder through OVERTALK_FFMPEG_PATH (see
docs/BUILDING.md#verified-ffmpeg-input) and ship the licences and tarballs.
configure and make need a POSIX shell: on Windows run this from an MSYS2 MINGW64
shell (mingw-w64-x86_64-gcc, make, diffutils, pkgconf and nasm, and a Python
for that shell); on macOS the Xcode command-line tools and pkg-config.
`--platform linux` exists only to cross-check the configure line on a
developer machine; Linux packages use the distribution's FFmpeg.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform as host
import shutil
import sys
import tarfile
import tempfile
import urllib.request

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))
import common  # noqa: E402
from common import MINIMUM_MACOS, ROOT, digest, run  # noqa: E402

# Upstream release tarballs. The digests were computed once from a download
# whose detached signature (ffmpeg-9.0.2.tar.xz.asc) gpg verified against the
# release key from https://ffmpeg.org/ffmpeg-devel.asc, fingerprint
# FCF9 86EA 15E6 E293 A564 4F10 B432 2F04 D676 58D8 (checked 2026-10-23),
# and libopus's digest is the one in xiph's published SHA256SUMS.txt
# (https://downloads.xiph.org/releases/opus/SHA256SUMS.txt).
# Changing a version or digest changes this file, and so the CI cache key.
FFMPEG_VERSION = "9.0.2"
FFMPEG_URL = f"https://ffmpeg.org/releases/ffmpeg-{FFMPEG_VERSION}.tar.xz"
FFMPEG_SHA256 = "8c3850283eb25fa026482078a04051e0be17347b09ef81a0849bec15a96e002e"
OPUS_VERSION = "1.6.1"
OPUS_URL = f"https://downloads.xiph.org/releases/opus/opus-{OPUS_VERSION}.tar.gz"
OPUS_SHA256 = "6ffcb593207be92584df15b32466ed64bbec99109f007c82205f0194572411a1"
DOWNLOAD_LIMIT = 64 * 1024 * 1024
RECORD = "ffmpeg-build.json"

# Only what Overtalk needs. --disable-autodetect keeps anything installed on the
# build machine (iconv, zlib, a TLS library, Homebrew's codecs) out of the
# result; FFmpeg's native codecs, demuxers, muxers and filters stay at their
# defaults. zlib is left out: the formats Overtalk imports (mp4/m4a, wav, mp3,
# flac, ogg, aac, mkv, webm, wma) decode without it. There are no network
# protocols; Overtalk runs FFmpeg with -protocol_whitelist file,pipe.
COMMON_CONFIGURE = [
    "--disable-autodetect", "--disable-network", "--disable-debug", "--disable-doc",
    "--disable-ffplay", "--disable-ffprobe",
    "--enable-static", "--disable-shared", "--pkg-config-flags=--static",
    "--enable-libopus",
]
SYSTEM_PREFIXES = ("/usr/lib/", "/System/")


def configure_arguments(target, assembler=True):
    """The arguments passed to FFmpeg's configure (no paths: libopus is found
    through PKG_CONFIG_PATH)."""
    arguments = list(COMMON_CONFIGURE)
    if target == "windows":
        # vfwcap (Video for Windows capture) would add a link to avicap32.dll, which
        # Windows Server lacks. h264_mf needs Direct3D 11 (more system DLLs): not built.
        arguments += ["--target-os=mingw32", "--arch=x86_64", "--extra-ldflags=-static",
                      "--disable-indev=vfwcap"]
    elif target == "macos":
        flag = f"-mmacosx-version-min={MINIMUM_MACOS}"
        arguments += ["--target-os=darwin", "--arch=arm64", f"--extra-cflags={flag}", f"--extra-ldflags={flag}",
                      "--enable-videotoolbox"]
    if not assembler:
        arguments.append("--disable-x86asm")
    return arguments


def opus_arguments(target, prefix):
    """The arguments passed to libopus's configure; `prefix` is where it is installed."""
    flags = "-O2"
    if target == "macos":
        flags += f" -mmacosx-version-min={MINIMUM_MACOS}"
    return [f"--prefix={prefix}", "--disable-shared", "--enable-static", "--disable-doc", "--disable-extra-programs",
            f"CFLAGS={flags}"]


def fetch(url, sha256, destination):
    """Download `url` to `destination` unless a file with the digest is already there.

    The tarball is untrusted until its digest matches the pin."""
    if destination.is_file() and digest(destination) == sha256:
        return destination
    destination.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(dir=destination.parent, delete=False, suffix=".part") as partial:
        try:
            with urllib.request.urlopen(url, timeout=120) as response:
                data = response.read(DOWNLOAD_LIMIT + 1)
            if len(data) > DOWNLOAD_LIMIT:
                raise SystemExit(f"{url} is larger than {DOWNLOAD_LIMIT} bytes")
            if hashlib.sha256(data).hexdigest() != sha256:
                raise SystemExit(f"{url} does not match its pinned SHA-256; refusing to build from it")
            partial.write(data)
        except BaseException:
            Path(partial.name).unlink(missing_ok=True)
            raise
    os.replace(partial.name, destination)
    return destination


def extract(tarball, parent):
    """Unpack a verified tarball into `parent` and return its single top-level folder."""
    with tarfile.open(tarball) as archive:
        archive.extractall(parent, filter="data")
        roots = {Path(name).parts[0] for name in archive.getnames()}
    if len(roots) != 1:
        raise SystemExit(f"{tarball} does not have one top-level folder")
    return parent / roots.pop()


def posix(path):
    """A path in the form the POSIX shell expects (MSYS2 on Windows)."""
    path = Path(path)
    if os.name != "nt":
        return str(path)
    return common.output(["cygpath", "-u", str(path)])


def build_opus(source, build, arguments, env, jobs):
    build.mkdir(parents=True)
    run(["sh", posix(source / "configure"), *arguments], cwd=build, env=env)
    run(["make", f"-j{jobs}"], cwd=build, env=env)
    run(["make", "install"], cwd=build, env=env)


def build_ffmpeg(source, build, arguments, env, jobs):
    build.mkdir(parents=True)
    run(["sh", posix(source / "configure"), *arguments], cwd=build, env=env)
    run(["make", f"-j{jobs}"], cwd=build, env=env)


def linked_libraries(binary, target):
    """The libraries an executable loads: install names (macOS) or DLL names (Windows)."""
    if target == "macos":
        return [line.split()[0] for line in common.output(["otool", "-L", binary]).splitlines()[1:] if line.strip()]
    if target == "linux":
        return [line.split()[0] for line in common.output(["ldd", binary]).splitlines() if line.strip()]
    text = common.output(["objdump", "-p", binary])
    return [line.split("DLL Name:")[1].strip() for line in text.splitlines() if "DLL Name:" in line]


def check_standalone(binary, target):
    """Fail unless the binary needs only the operating system's libraries."""
    libraries = linked_libraries(binary, target)
    print(f"{binary.name} links: {', '.join(libraries)}")
    if target == "macos":
        bad = [name for name in libraries if not name.startswith(SYSTEM_PREFIXES)]
    elif target == "windows":
        system = Path(os.environ.get("SystemRoot", r"C:\Windows")) / "System32"
        # Windows API sets resolve to system DLLs; anything else must be in System32.
        bad = [name for name in libraries if not name.lower().startswith("api-ms-win-")
               and os.name == "nt" and not (system / name).is_file()]
        bad += [name for name in libraries if name.lower().startswith(("lib", "msys"))]
    else:
        bad = [name for name in libraries if "opus" in name or "libav" in name]
    if bad:
        raise SystemExit(f"{binary} is not standalone; it loads {', '.join(sorted(set(bad)))}")


def check_licence(binary, arguments):
    banner = common.output([binary, "-version"])
    # `-L` prints the licence of the configuration that was built.
    licence = " ".join(common.output([binary, "-L"]).split())
    if "GNU Lesser General Public License" not in licence or "GNU General Public License" in licence.replace("Lesser General", ""):
        raise SystemExit(f"{binary} does not report the LGPL 2.1 or later:\n{licence}")
    if any(flag in arguments for flag in ("--enable-gpl", "--enable-nonfree", "--enable-version3")):
        raise SystemExit("The configure line enables GPL, version 3 or non-free components")
    return banner.splitlines()[0]


def inputs(target, arguments):
    opus = opus_arguments(target, "<prefix>")
    return {"target": target,
            "ffmpeg": {"version": FFMPEG_VERSION, "url": FFMPEG_URL, "sha256": FFMPEG_SHA256},
            "libopus": {"version": OPUS_VERSION, "url": OPUS_URL, "sha256": OPUS_SHA256, "configure": opus},
            "configure": arguments}


def current(out, binary_name, wanted):
    """The build record when `out` already holds a build of exactly these inputs."""
    record = out / RECORD
    binary = out / binary_name
    if not record.is_file() or not binary.is_file():
        return None
    info = json.loads(record.read_text())
    same = all(info.get(key) == value for key, value in wanted.items())
    sources = all((out / "source" / Path(info[name]["url"]).name).is_file() for name in ("ffmpeg", "libopus"))
    return info if same and sources and digest(binary) == info["binary"]["sha256"] else None


def main():
    os.umask(0o022)
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    system = {"Windows": "windows", "Darwin": "macos"}.get(host.system(), "linux")
    parser.add_argument("--platform", choices=["windows", "macos", "linux"], default=system)
    parser.add_argument("--out", type=Path, help="default target/ffmpeg-<platform>")
    parser.add_argument("--work", type=Path, help="scratch folder; default a temporary one")
    parser.add_argument("--check", action="store_true",
                        help="build nothing; fail unless --out holds an intact build of the current inputs")
    parser.add_argument("--jobs", type=int, default=os.cpu_count() or 2)
    args = parser.parse_args()
    target = args.platform
    out = (args.out or ROOT / f"target/ffmpeg-{target}").resolve()
    name = "ffmpeg.exe" if target == "windows" else "ffmpeg"
    # Windows needs nasm (configure stops without it); a Linux cross-check may go without.
    assembler = target != "linux" or bool(shutil.which("nasm") or shutil.which("yasm"))
    arguments = configure_arguments(target, assembler=assembler)
    wanted = inputs(target, arguments)
    info = current(out, name, wanted)
    if info:
        print(f"{out / name} is current ({info['binary']['sha256']}); nothing to build")
    elif args.check:
        raise SystemExit(f"{out} does not hold a build of the pinned inputs")
    else:
        info = build(args, target, out, name, arguments, wanted)
    print(f"{out / name}  {(out / name).stat().st_size / 1e6:.1f} MB  sha256 {info['binary']['sha256']}")


def build(args, target, out, name, arguments, wanted):
    if out.exists():
        shutil.rmtree(out)
    source_dir = out / "source"
    ffmpeg_tar = fetch(FFMPEG_URL, FFMPEG_SHA256, source_dir / Path(FFMPEG_URL).name)
    opus_tar = fetch(OPUS_URL, OPUS_SHA256, source_dir / Path(OPUS_URL).name)
    with tempfile.TemporaryDirectory(prefix="overtalk-ffmpeg-", dir=args.work) as temporary:
        work = Path(temporary)
        ffmpeg_source = extract(ffmpeg_tar, work / "src")
        opus_source = extract(opus_tar, work / "src")
        prefix = work / "opus"
        env = dict(os.environ)
        if target == "macos":
            env["MACOSX_DEPLOYMENT_TARGET"] = MINIMUM_MACOS
        build_opus(opus_source, work / "build-opus", opus_arguments(target, posix(prefix)), env, args.jobs)
        # Find only the libopus built above, never one installed on the machine.
        pkgconfig = posix(prefix / "lib/pkgconfig")
        env["PKG_CONFIG_PATH"] = pkgconfig
        env["PKG_CONFIG_LIBDIR"] = pkgconfig
        build_ffmpeg(ffmpeg_source, work / "build-ffmpeg", arguments, env, args.jobs)
        binary = out / name
        shutil.copyfile(work / "build-ffmpeg" / name, binary)
        binary.chmod(0o755)
        licenses = out / "licenses"
        licenses.mkdir()
        shutil.copyfile(ffmpeg_source / "COPYING.LGPLv2.1", licenses / "ffmpeg-COPYING.LGPLv2.1")
        shutil.copyfile(opus_source / "COPYING", licenses / "libopus-COPYING")
    if target != "linux":
        run(["strip", "-x" if target == "macos" else "-s", binary])
    if target == "macos":
        # Stripping invalidates the linker's ad-hoc signature, and arm64 refuses unsigned code.
        run(["codesign", "--force", "--sign", "-", binary])
    check_standalone(binary, target)
    banner = check_licence(binary, arguments)
    info = {**wanted, "version_line": banner, "license": "LGPL-2.1-or-later",
            "binary": {"name": name, "sha256": digest(binary), "size": binary.stat().st_size}}
    if target == "macos":
        info["minimum_macos"] = MINIMUM_MACOS
    (out / RECORD).write_text(json.dumps(info, indent=2) + "\n", newline="\n")
    return info


if __name__ == "__main__":
    main()
