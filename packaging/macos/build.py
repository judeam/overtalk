#!/usr/bin/env python3
"""Build Overtalk for macOS: Overtalk.app inside a .dmg, a build record and SHA256SUMS.

The bundle keeps the UI, engine and helpers side by side in Contents/MacOS, where
each finds the others, and the engine's resources in Contents/Resources, where
Tauri resolves them. Every executable and the bundle are signed: ad hoc by
default, or with the owner's Developer ID (hardened runtime and a secure
timestamp), and the .dmg is then notarized when a notarytool profile is given.
See docs/PLATFORMS.md#macos and #macos-developer-id-and-notarization.
"""
import argparse
import json
import os
from pathlib import Path
import platform
import plistlib
import shutil
import subprocess
import sys
import tempfile

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))
import common  # noqa: E402
from common import MINIMUM_MACOS, ROOT, digest, run  # noqa: E402
from worker_build import verify_binary  # noqa: E402

ICON = ROOT / "frontend/src-tauri/icons/icon.icns"
ENTITLEMENTS = HERE / "entitlements.plist"
# macOS shows these when Overtalk first asks; the engine and helpers run inside
# this bundle, so the permissions belong to Overtalk.app.
USAGE = {
    "NSMicrophoneUsageDescription":
        "Overtalk records your microphone during the meetings you choose to record. Recordings stay on this Mac.",
    "NSAudioCaptureUsageDescription":
        "Overtalk records the sound of your call from other apps during the meetings you choose to record. "
        "Recordings stay on this Mac.",
    "NSScreenCaptureUsageDescription":
        "Overtalk records a screen or window only when you choose to capture a meeting's video. "
        "Recordings stay on this Mac.",
}
# Executables that open the microphone get its entitlement under the hardened runtime.
AUDIO_INPUT = {"overtalk", "sonefold-service"}


def architecture(target):
    machine = (target or platform.machine()).split("-")[0]
    return "arm64" if machine in {"aarch64", "arm64"} else "x86_64"


def info_plist(display_name, version, executable="overtalk", bundle_id=common.APP_ID):
    short = common.numeric_version(version, 3)
    return {
        "CFBundleDevelopmentRegion": "en",
        "CFBundleDisplayName": display_name,
        "CFBundleExecutable": executable,
        "CFBundleIconFile": "Overtalk.icns",
        "CFBundleIdentifier": bundle_id,
        "CFBundleInfoDictionaryVersion": "6.0",
        "CFBundleName": display_name,
        "CFBundlePackageType": "APPL",
        "CFBundleShortVersionString": short,
        "CFBundleVersion": short,
        "OvertalkBuildVersion": version,
        "LSApplicationCategoryType": "public.app-category.productivity",
        "LSMinimumSystemVersion": MINIMUM_MACOS,
        "NSHighResolutionCapable": True,
        "NSSupportsAutomaticGraphicsSwitching": True,
        "NSHumanReadableCopyright": "Jude Mingay. MIT licence.",
        **USAGE,
    }


def system_libraries_only(executable):
    """True when an executable links only to macOS's own libraries, so it can be
    copied into the bundle. Homebrew's FFmpeg, for example, needs its own dylibs."""
    try:
        linked = common.output(["otool", "-L", executable]).splitlines()[1:]
    except (OSError, subprocess.CalledProcessError):
        return False
    return all(line.strip().startswith(("/usr/lib/", "/System/")) for line in linked if line.strip())


def bundled_ffmpeg(args):
    if args.ffmpeg == "none":
        return None, "not bundled (--ffmpeg none)"
    source = common.verified_ffmpeg(args.ffmpeg) if args.ffmpeg else (
        common.verified_ffmpeg(None) if os.environ.get("OVERTALK_FFMPEG_PATH") else None)
    if source is None:
        return None, "not bundled (none given)"
    if not system_libraries_only(source):
        if args.ffmpeg:
            raise SystemExit(f"{source} links to libraries outside macOS; give a standalone FFmpeg")
        return None, ("not bundled: the build's FFmpeg needs non-system libraries; "
                      "Overtalk uses /opt/homebrew/bin/ffmpeg or /usr/local/bin/ffmpeg")
    return source, "bundled"


def stage_bundle(args, binaries, app):
    contents = app / "Contents"
    executables = contents / "MacOS"
    resources = contents / "Resources"
    executables.mkdir(parents=True)
    resources.mkdir()
    for name, source in binaries.items():
        target = executables / name
        shutil.copyfile(source, target)
        target.chmod(0o755)
    if sys.platform == "darwin" and architecture(args.target) == architecture(None):
        # The sandbox helper must come from these sources; a stale one is refused.
        verify_binary(executables / "overtalk-worker")
    ffmpeg, ffmpeg_state = bundled_ffmpeg(args)
    if ffmpeg:
        shutil.copyfile(ffmpeg, executables / "ffmpeg")
        (executables / "ffmpeg").chmod(0o755)
    common.stage_resources(resources)
    licenses = resources / "licenses"
    common.stage_all_licenses(licenses)
    sources = []
    if ffmpeg:
        notice, sources = common.stage_ffmpeg_notices(ffmpeg, licenses, args.out, "ffmpeg")
    else:
        notice = ("FFmpeg is not bundled. Overtalk uses /opt/homebrew/bin/ffmpeg or /usr/local/bin/ffmpeg\n"
                  "(brew install ffmpeg) to write and convert recordings.\n")
    (licenses / "FFMPEG.txt").write_text(notice)
    shutil.copyfile(ICON, resources / "Overtalk.icns")
    with (contents / "Info.plist").open("wb") as handle:
        plistlib.dump(info_plist(args.display_name, args.version), handle, sort_keys=True)
    (contents / "PkgInfo").write_text("APPL????")
    return {"ffmpeg": {"state": ffmpeg_state, **common.ffmpeg_record(ffmpeg, sources)}}


def sign_bundle(args, app):
    """Sign inside-out: every helper, then the main executable through the bundle."""
    identity = args.sign_identity
    developer = identity != "-"
    base = ["codesign", "--force", "--sign", identity]
    if developer:
        base += ["--options", "runtime", "--timestamp"]
    executables = app / "Contents/MacOS"
    for path in sorted(executables.iterdir()):
        run(["strip", "-S", "-x", path])
    for path in sorted(executables.iterdir()):
        if path.name != "overtalk":
            extra = ["--entitlements", ENTITLEMENTS] if developer and path.name in AUDIO_INPUT else []
            run([*base, *extra, "--identifier", f"{common.APP_ID}.{path.name}", path])
    extra = ["--entitlements", ENTITLEMENTS] if developer else []
    run([*base, *extra, app])
    run(["codesign", "--verify", "--deep", "--strict", "--verbose=2", app])
    return "developer-id" if developer else "ad-hoc"


def make_dmg(args, app, work, name):
    staging = work / "dmg"
    staging.mkdir()
    shutil.copytree(app, staging / app.name, symlinks=True)
    (staging / "Applications").symlink_to("/Applications")
    dmg = args.out / name
    dmg.unlink(missing_ok=True)
    run(["hdiutil", "create", "-volname", args.display_name, "-srcfolder", staging, "-fs", "HFS+",
         "-format", "UDZO", "-imagekey", "zlib-level=9", "-ov", dmg])
    if args.sign_identity != "-":
        run(["codesign", "--force", "--sign", args.sign_identity, "--timestamp", dmg])
    if args.notary_profile:
        if args.sign_identity == "-":
            raise SystemExit("Notarization needs a Developer ID signature: pass --sign-identity")
        run(["xcrun", "notarytool", "submit", dmg, "--keychain-profile", args.notary_profile, "--wait"])
        run(["xcrun", "stapler", "staple", dmg])
    return dmg


def main():
    os.umask(0o022)
    parser = argparse.ArgumentParser(description=__doc__)
    common.common_arguments(parser, "macos")
    parser.add_argument("--formats", default="dmg", help="Comma-separated: dmg, zip (a zipped .app)")
    parser.add_argument("--sign-identity", default=os.environ.get("OVERTALK_MACOS_SIGN_IDENTITY", "-"),
                        help="codesign identity; '-' (default) signs ad hoc")
    parser.add_argument("--notary-profile", default=os.environ.get("OVERTALK_NOTARY_PROFILE"),
                        help="notarytool keychain profile; notarizes and staples the .dmg")
    args = common.resolve_arguments(parser.parse_args())
    formats = set(args.formats.split(","))
    if not formats <= {"dmg", "zip"}:
        raise SystemExit(f"Unknown format in {args.formats}")
    if sys.platform != "darwin":
        raise SystemExit("Build macOS packages on macOS (codesign, hdiutil and the SDK are needed)")
    # Build every binary, CMake's included, for the oldest macOS the bundle declares.
    deployment = {"MACOSX_DEPLOYMENT_TARGET": MINIMUM_MACOS, "CMAKE_OSX_DEPLOYMENT_TARGET": MINIMUM_MACOS}
    if not args.skip_build:
        common.build(args, windows=False, extra_env=deployment)
    binaries = common.built_binaries(args, windows=False)
    epoch = common.source_epoch()
    args.version = args.version or common.default_version(epoch, args.engine_manifest)
    args.out.mkdir(parents=True, exist_ok=True)
    arch = architecture(args.target)
    stem = f"{args.display_name.replace(' ', '-')}-{args.version}-macos-{arch}"
    artifacts, sources = [], []
    with tempfile.TemporaryDirectory(prefix="overtalk-package-", dir=args.out.parent) as temporary:
        work = Path(temporary)
        app = work / f"{args.display_name}.app"
        bundled = stage_bundle(args, binaries, app)
        sources = [args.out / name for name in bundled["ffmpeg"]["source_files"]]
        signature = sign_bundle(args, app)
        if "dmg" in formats:
            artifacts.append(make_dmg(args, app, work, f"{stem}.dmg"))
        if "zip" in formats:
            archive = args.out / f"{stem}.zip"
            run(["ditto", "-c", "-k", "--keepParent", app, archive])
            artifacts.append(archive)
    info = common.build_record(args, binaries, epoch, {
        **bundled, "signature": signature, "notarized": bool(args.notary_profile),
        "minimum_macos": MINIMUM_MACOS, "architecture": arch,
        "artifacts": {path.name: digest(path) for path in artifacts}})
    record = args.out / f"{args.package}-macos-{arch}-build-info.json"
    record.write_text(json.dumps(info, indent=2) + "\n")
    sums = common.write_checksums(args.out, [*artifacts, *sources, record], args.sign_key)
    for path in [*artifacts, record, sums]:
        print(f"{path}  {path.stat().st_size / 1e6:.1f} MB")


if __name__ == "__main__":
    main()
