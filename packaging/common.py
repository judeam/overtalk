"""Shared by the Windows and macOS packagers: build, stage, record, checksum.

It follows packaging/linux/build.py: release binaries come from a dedicated
Cargo target directory, one tree is staged, and every run writes a build record
(`<package>-build-info.json`) and `SHA256SUMS`. Nothing here uploads or
publishes anything.
"""
import datetime
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import zipfile

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
from native_licenses import stage_crate_notices, stage_licenses  # noqa: E402

APP_ID = "io.github.judeam.Overtalk"
HOMEPAGE = "https://github.com/judeam/overtalk"
# The UI and its helpers. The engine (sonefold-service) and llama-helper are
# built separately; every binary sits in one directory, where each finds the others.
OSS_BINARIES = {
    "overtalk": "overtalk-native",
    "overtalk-worker": "overtalk-worker-sandbox",
    "overtalk-capture": "overtalk-capture",
    "overtalk-browser-bridge": "overtalk-browser-bridge",
}
# The command-line client ships beside the app whenever the workspace builds it.
CLI_BINARY = "overtalk-cli"


# Options whose next argument is a secret. Packaging never passes one, but a
# command echoed to a CI log must not reveal it if a caller ever does.
SECRET_OPTIONS = {"/p", "-password", "--password"}


def printable(command):
    """The command as echoed to the log, with secret option values replaced."""
    shown, hide = [], False
    for part in map(str, command):
        shown.append("***" if hide else part)
        hide = part.lower() in SECRET_OPTIONS
    return " ".join(shown)


def run(command, **kwargs):
    print("+", printable(command), flush=True)
    return subprocess.run([str(part) for part in command], check=True, **kwargs)


def output(command, **kwargs):
    return subprocess.run([str(part) for part in command], check=True, capture_output=True,
                          text=True, **kwargs).stdout.strip()


def digest(path):
    with Path(path).open("rb") as handle:
        return hashlib.file_digest(handle, "sha256").hexdigest()


def git(repository, *args):
    return output(["git", "-C", repository, *args])


def executable_name(name, windows):
    return f"{name}.exe" if windows else name


def cli_package(metadata):
    """The workspace package with a binary named overtalk-cli, or None."""
    for package in metadata.get("packages", []):
        for target in package.get("targets", []):
            if target.get("name") == CLI_BINARY and "bin" in target.get("kind", []):
                return package["name"]
    return None


def workspace_metadata(root=ROOT):
    return json.loads(output(["cargo", "metadata", "--locked", "--no-deps", "--format-version=1"], cwd=root))


def cargo_env(target_dir, rustflags=(), c_prefix_map=True, base=None):
    """The environment of every packaging build.

    `rustflags` is a sequence of separate flags. They are passed through
    CARGO_ENCODED_RUSTFLAGS so a home directory containing spaces stays one flag.
    """
    env = dict(os.environ if base is None else base, CARGO_TARGET_DIR=str(target_dir))
    # tauri-build would copy the developer's sidecar seeds (for example a CUDA
    # llama-helper) into the target directory. Packaging stages its own.
    env["TAURI_CONFIG"] = json.dumps({"bundle": {"externalBin": []}}, separators=(",", ":"))
    # ggml otherwise compiles whisper.cpp for the build machine's CPU only.
    env["CMAKE_PROJECT_INCLUDE_BEFORE"] = (ROOT / "packaging/linux/portable-cpu.cmake").as_posix()
    # ONNX Runtime is linked statically (Linux, macOS) or loaded from the DLL
    # shipped beside the engine (Windows); a developer's dynamic runtime is ignored.
    location = Path(env.get("ORT_LIB_LOCATION", "/nonexistent"))
    unset = ["ORT_PREFER_DYNAMIC_LINK", "RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS"]
    if not (location / "libonnxruntime.a").is_file():
        unset += ["ORT_LIB_LOCATION", "ORT_SKIP_DOWNLOAD"]
    for name in unset:
        env.pop(name, None)
    # Keep the builder's home directory out of panic locations and assertions.
    home = str(Path.home())
    env["CARGO_ENCODED_RUSTFLAGS"] = "\x1f".join([*rustflags, f"--remap-path-prefix={home}=/build"])
    if c_prefix_map:
        # clang and GCC only; MSVC's cl.exe has no equivalent option.
        for name in ["CFLAGS", "CXXFLAGS"]:
            env[name] = " ".join(filter(None, [env.get(name), f"-ffile-prefix-map={home}=/build"]))
    return env


def build(args, windows, rustflags=(), llama_rustflags=(), c_prefix_map=True, extra_env=None):
    """Build the app, engine, helpers, llama-helper and (when present) overtalk-cli."""
    for name in ["OVERTALK_FFMPEG_PATH", "OVERTALK_FFMPEG_SHA256"]:
        if not os.environ.get(name):
            raise SystemExit(f"Set {name} first; see docs/BUILDING.md#verified-ffmpeg-input")
    base = dict(os.environ, **(extra_env or {}))
    target = ["--target", args.target] if args.target else []
    packages = [*OSS_BINARIES.values()]
    binaries = [*OSS_BINARIES]
    cli = cli_package(workspace_metadata())
    if cli:
        packages.append(cli)
        binaries.append(CLI_BINARY)
    if not args.engine_manifest:
        packages.append("sonefold")
        binaries.append("sonefold-service")
    command = ["cargo", "build", "--release", "--locked", *target]
    for package in dict.fromkeys(packages):
        command += ["-p", package]
    for binary in binaries:
        command += ["--bin", binary]
    run(command, cwd=ROOT, env=cargo_env(args.target_dir, rustflags, c_prefix_map, base))
    if args.engine_manifest:
        run(["cargo", "build", "--release", "--locked", *target, "--manifest-path", args.engine_manifest,
             "-p", args.engine_package, "--bin", "sonefold-service"],
            cwd=args.engine_manifest.parent,
            env=cargo_env(engine_target(args), rustflags, c_prefix_map, base))
    run(["cargo", "build", "--release", "--locked", *target, "-p", "llama-helper"], cwd=ROOT,
        env=cargo_env(args.target_dir / "llama-helper", llama_rustflags, c_prefix_map, base))


def engine_target(args):
    return args.engine_manifest.parent / "target" / args.target_dir.name


def release_dir(target_dir, target):
    return target_dir / target / "release" if target else target_dir / "release"


def built_binaries(args, windows):
    """Every binary to ship, by installed name (without .exe). overtalk-cli is optional."""
    release = release_dir(args.target_dir, args.target)
    binaries = {name: release / executable_name(name, windows) for name in OSS_BINARIES}
    engine = engine_target(args) if args.engine_manifest else args.target_dir
    binaries["sonefold-service"] = release_dir(engine, args.target) / executable_name("sonefold-service", windows)
    binaries["llama-helper"] = (release_dir(args.target_dir / "llama-helper", args.target)
                                / executable_name("llama-helper", windows))
    for name, path in binaries.items():
        if not path.is_file():
            raise SystemExit(f"Missing {name}: {path}. Build without --skip-build first.")
    # The command-line client ships beside the UI whenever the workspace has it.
    cli = release / executable_name(CLI_BINARY, windows)
    if cli_package(workspace_metadata()):
        if not cli.is_file():
            raise SystemExit(f"Missing {CLI_BINARY}: {cli}. Build without --skip-build first.")
        binaries[CLI_BINARY] = cli
    return binaries


def source_epoch():
    return int(os.environ.get("SOURCE_DATE_EPOCH") or git(ROOT, "log", "-1", "--format=%ct"))


def default_version(epoch, engine_manifest=None):
    """<crate version>-git<commit date>.<commit>, naming the checkout being packaged."""
    metadata = workspace_metadata()
    base = next(p["version"] for p in metadata["packages"] if p["name"] == "overtalk-native")
    date = datetime.datetime.fromtimestamp(epoch, datetime.timezone.utc).strftime("%Y%m%d")
    version = f"{base}-git{date}.{git(ROOT, 'rev-parse', '--short=10', 'HEAD')}"
    if engine_manifest:
        version += f"+engine.{git(engine_manifest.parent, 'rev-parse', '--short=10', 'HEAD')}"
    return version


def numeric_version(version, parts=3):
    """The leading numeric fields of a version, padded: '0.1.0-git...' -> '0.1.0'."""
    numbers = re.findall(r"\d+", version.split("-")[0].split("~")[0].split("+")[0])[:parts]
    numbers += ["0"] * (parts - len(numbers))
    return ".".join(str(int(number)) for number in numbers)


def stage_all_licenses(licenses):
    """The notices every package ships, plus the native libraries compiled in."""
    stage_licenses(licenses)
    stage_crate_notices(licenses)
    # ONNX Runtime 1.22.0: linked statically on Linux and macOS, a DLL on Windows.
    for source in sorted((ROOT / "packaging/linux/licenses").iterdir()):
        shutil.copyfile(source, licenses / source.name)
    metadata = json.loads(output(["cargo", "metadata", "--locked", "--format-version=1"], cwd=ROOT))
    for package in metadata["packages"]:
        vendored = {"whisper-rs-sys": "whisper.cpp", "llama-cpp-sys-2": "llama.cpp"}.get(package["name"])
        if vendored:
            notice = Path(package["manifest_path"]).parent / vendored / "LICENSE"
            if notice.is_file():
                shutil.copyfile(notice, licenses / f"{vendored}-LICENSE.txt")


def stage_resources(directory):
    """The engine's runtime resources (templates, speech worker, integrations)."""
    run([sys.executable, ROOT / "scripts/stage-native-resources.py", "--into", directory])


def verified_ffmpeg(choice):
    """The FFmpeg to bundle: an explicit path, the verified build input, or None for 'none'."""
    if choice == "none":
        return None
    path = Path(choice) if choice else Path(os.environ.get("OVERTALK_FFMPEG_PATH", ""))
    expected = os.environ.get("OVERTALK_FFMPEG_SHA256", "").lower()
    if not path.is_file():
        raise SystemExit("No FFmpeg to bundle: set OVERTALK_FFMPEG_PATH or pass --ffmpeg none")
    if not choice and digest(path) != expected:
        raise SystemExit("FFmpeg does not match OVERTALK_FFMPEG_SHA256; refusing to bundle it")
    return path


def build_record(args, binaries, epoch, extra=None):
    info = {"version": args.version, "package": args.package, "source_date_epoch": epoch,
            "commit": git(ROOT, "rev-parse", "HEAD"),
            "dirty": bool(git(ROOT, "status", "--porcelain", "--untracked-files=no")),
            "rustc": output(["rustc", "--version"]),
            "target": args.target or output(["rustc", "-vV"]).split("host: ", 1)[1].split()[0],
            "binaries": {name: digest(path) for name, path in sorted(binaries.items())}}
    if args.engine_manifest:
        repository = args.engine_manifest.parent
        info["engine"] = {"commit": git(repository, "rev-parse", "HEAD"),
                          "dirty": bool(git(repository, "status", "--porcelain", "--untracked-files=no"))}
    info.update(extra or {})
    return info


def write_checksums(out, artifacts, sign_key=None):
    """SHA256SUMS for the artifacts, and a detached signature when a GPG key is given."""
    sums = out / "SHA256SUMS"
    sums.write_text("".join(f"{digest(path)}  {path.name}\n" for path in artifacts), newline="\n")
    key = sign_key or os.environ.get("OVERTALK_SIGNING_KEY")
    if key:
        run(["gpg", "--batch", "--yes", "--local-user", key, "--armor", "--detach-sign", sums])
    return sums


def deterministic_zip(source, target, epoch, prefix):
    """Zip `source` as `prefix/...` with sorted entries and fixed times and modes."""
    stamp = datetime.datetime.fromtimestamp(max(epoch, 315532800), datetime.timezone.utc)
    stamp = stamp.timetuple()[:6]
    with zipfile.ZipFile(target, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=9) as archive:
        for path in sorted(source.rglob("*")):
            name = f"{prefix}/{path.relative_to(source).as_posix()}"
            if path.is_dir():
                entry = zipfile.ZipInfo(name + "/", stamp)
                entry.external_attr = (0o40755 << 16) | 0x10
                archive.writestr(entry, b"")
                continue
            entry = zipfile.ZipInfo(name, stamp)
            executable = os.access(path, os.X_OK) or path.suffix.lower() in {".exe", ".dll"}
            entry.external_attr = (0o100755 if executable else 0o100644) << 16
            entry.compress_type = zipfile.ZIP_DEFLATED
            with path.open("rb") as handle:
                archive.writestr(entry, handle.read())
    return target


def common_arguments(parser, platform):
    """Options shared with packaging/linux/build.py, where they mean the same thing."""
    parser.add_argument("--out", type=Path, default=ROOT / f"target/package-{platform}/dist")
    parser.add_argument("--target-dir", type=Path, default=ROOT / f"target/package-{platform}")
    parser.add_argument("--target", help="Rust target triple; default the host")
    parser.add_argument("--skip-build", action="store_true", help="Package the binaries already built")
    parser.add_argument("--engine-manifest", type=Path, help="Cargo.toml of a workspace providing another sonefold-service")
    parser.add_argument("--engine-package", default="sonefold", help="Package in --engine-manifest that builds sonefold-service")
    parser.add_argument("--package", default="overtalk", help="Base name of the artifacts")
    parser.add_argument("--display-name", default="Overtalk")
    parser.add_argument("--version", help="Version; default <crate version>-git<date>.<commit>")
    parser.add_argument("--ffmpeg", help="FFmpeg executable to bundle, or 'none'; default the verified OVERTALK_FFMPEG_PATH")
    parser.add_argument("--sign-key", help="GPG key that signs SHA256SUMS (or set OVERTALK_SIGNING_KEY)")


def resolve_arguments(args):
    args.engine_manifest = args.engine_manifest.resolve() if args.engine_manifest else None
    args.target_dir = args.target_dir.resolve()
    args.out = args.out.resolve()
    return args
