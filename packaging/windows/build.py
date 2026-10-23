#!/usr/bin/env python3
"""Build Overtalk for Windows: a per-user NSIS installer, a portable zip and SHA256SUMS.

Release binaries are built in target/package-windows, then one folder is staged:
the app, engine and helpers side by side, ONNX Runtime and the Visual C++
runtime DLLs they load, FFmpeg, the engine's resources and the licences. The
installer and the zip wrap that same folder. Signing is optional and uses the
owner's Authenticode certificate (see docs/PLATFORMS.md#windows-authenticode).
"""
import argparse
import json
import os
from pathlib import Path
import re
import shutil
import struct
import subprocess
import sys
import tempfile

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))
import common  # noqa: E402
from common import ROOT, digest, run  # noqa: E402
from worker_build import verify_binary  # noqa: E402

ONNXRUNTIME = ["onnxruntime.dll", "onnxruntime_providers_shared.dll"]
ICON = ROOT / "frontend/src-tauri/icons/icon.ico"
# Run values and the uninstall entry use this name.
REGISTRY_NAME = "Overtalk"
TIMESTAMP_URL = "http://timestamp.digicert.com"


def pe_imports(path):
    """The DLLs a PE image imports, including delay-loaded ones (lower case)."""
    data = Path(path).read_bytes()
    if data[:2] != b"MZ":
        raise ValueError(f"{path} is not a Windows executable")
    header = struct.unpack_from("<I", data, 0x3C)[0]
    if data[header:header + 4] != b"PE\0\0":
        raise ValueError(f"{path} has no PE header")
    sections, optional_size = struct.unpack_from("<H", data, header + 6)[0], struct.unpack_from("<H", data, header + 20)[0]
    optional = header + 24
    directories = optional + (112 if struct.unpack_from("<H", data, optional)[0] == 0x20B else 96)
    table = optional + optional_size

    def offset(rva):
        for index in range(sections):
            size, address, raw_size, raw = struct.unpack_from("<IIII", data, table + 40 * index + 8)
            if address <= rva < address + max(size, raw_size):
                return rva - address + raw
        raise ValueError(f"{path}: address {rva:#x} is outside every section")

    def name(rva):
        start = offset(rva)
        return data[start:data.index(b"\0", start)].decode("ascii").lower()

    names = set()
    # Data directory 1 is the import table (20-byte descriptors, name at +12);
    # 13 is the delay-load table (32-byte descriptors, name at +4).
    for entry, size, name_at in [(1, 20, 12), (13, 32, 4)]:
        rva = struct.unpack_from("<I", data, directories + 8 * entry)[0]
        if not rva:
            continue
        at = offset(rva)
        while any(data[at:at + size]):
            names.add(name(struct.unpack_from("<I", data, at + name_at)[0]))
            at += size
    return names


def vc_runtime_directory():
    """The newest Visual C++ x64 runtime redistributable folder of this Visual Studio."""
    folders = []
    if os.environ.get("VCToolsRedistDir"):
        folders += Path(os.environ["VCToolsRedistDir"]).glob("x64/Microsoft.VC*.CRT")
    vswhere = Path(os.environ.get("ProgramFiles(x86)", r"C:\Program Files (x86)")) / "Microsoft Visual Studio/Installer/vswhere.exe"
    if vswhere.is_file():
        install = common.output([vswhere, "-latest", "-products", "*", "-requires",
                                 "Microsoft.VisualStudio.Component.VC.Tools.x86.x64", "-property", "installationPath"])
        if install:
            folders += Path(install).glob("VC/Redist/MSVC/*/x64/Microsoft.VC*.CRT")

    def version(folder):
        return tuple(int(part) for part in folder.parent.parent.name.split(".") if part.isdigit())
    folders = [folder for folder in folders if folder.is_dir()]
    return max(folders, key=version) if folders else None


def runtime_dlls(images, redist):
    """Redistributable runtime DLLs the images need, followed through their own imports."""
    available = {path.name.lower(): path for path in redist.glob("*.dll")} if redist else {}
    needed, queue = {}, list(images)
    while queue:
        for name in sorted(pe_imports(queue.pop())):
            if name in available and name not in needed:
                needed[name] = available[name]
                queue.append(available[name])
    return [needed[name] for name in sorted(needed)]


def signing_command(display_name):
    """signtool arguments for the owner's certificate, or None when unsigned.

    The certificate is chosen by its SHA-1 thumbprint from the user's
    certificate store (an imported .pfx, or a hardware token), so no password
    ever reaches a command line, a log or the NSIS script.
    """
    thumbprint = os.environ.get("OVERTALK_WINDOWS_CERT_SHA1", "").strip()
    if not thumbprint:
        if os.environ.get("OVERTALK_WINDOWS_CERT_FILE"):
            raise SystemExit("Import the .pfx into Cert:\\CurrentUser\\My and set OVERTALK_WINDOWS_CERT_SHA1 "
                             "to its thumbprint; certificate files and passwords are not passed to signtool")
        return None
    if not re.fullmatch(r"[0-9A-Fa-f]{40}", thumbprint):
        raise SystemExit("OVERTALK_WINDOWS_CERT_SHA1 must be a certificate's 40-character SHA-1 thumbprint")
    signtool = os.environ.get("OVERTALK_SIGNTOOL") or shutil.which("signtool")
    if not signtool:
        kits = Path(os.environ.get("ProgramFiles(x86)", r"C:\Program Files (x86)")) / "Windows Kits/10/bin"
        candidates = sorted(kits.glob("10.*/x64/signtool.exe"))
        signtool = str(candidates[-1]) if candidates else None
    if not signtool:
        raise SystemExit("Signing was requested but signtool.exe was not found; set OVERTALK_SIGNTOOL")
    return [signtool, "sign", "/sha1", thumbprint, "/fd", "sha256", "/td", "sha256",
            "/tr", os.environ.get("OVERTALK_TIMESTAMP_URL", TIMESTAMP_URL),
            "/d", display_name, "/du", common.HOMEPAGE]


def uninstaller_signer(signer, work):
    """NSIS's !uninstfinalize command: a wrapper script that runs signtool on %1.

    The define names only the wrapper; the signtool arguments live in it.
    """
    wrapper = work / "sign-uninstaller.cmd"
    line = subprocess.list2cmdline(signer).replace("%", "%%")
    wrapper.write_text(f'@echo off\r\n{line} "%1"\r\n', encoding="utf-8")
    return f'"{wrapper}" "%1"'


def sign(command, paths):
    if command and paths:
        run([*command, *paths])


def stage(args, binaries, tree, signer):
    """The application folder every format shares. Tauri resolves the engine's
    resources in the executable's own folder on Windows."""
    tree.mkdir(parents=True)
    own = []
    for name, source in binaries.items():
        target = tree / f"{name}.exe"
        shutil.copyfile(source, target)
        own.append(target)
    if os.name == "nt":
        # The sandbox helper must come from these sources; a stale one is refused.
        verify_binary(tree / "overtalk-worker.exe")
    sign(signer, own)
    # The engine loads ONNX Runtime from its folder (ort's load-dynamic); build.rs
    # downloaded and verified this exact release.
    runtime = ROOT / "frontend/src-tauri/binaries/onnxruntime"
    for name in ONNXRUNTIME:
        if not (runtime / name).is_file():
            raise SystemExit(f"Missing {runtime / name}; build sonefold-service for Windows first")
        shutil.copyfile(runtime / name, tree / name)
    images = [*own, *(tree / name for name in ONNXRUNTIME)]
    redist = None if args.vc_runtime == "none" else (Path(args.vc_runtime) if args.vc_runtime else vc_runtime_directory())
    vc_runtime = runtime_dlls(images, redist)
    if redist is None and args.vc_runtime != "none" and any(
            name.startswith(("vcruntime", "msvcp")) for image in images for name in pe_imports(image)):
        raise SystemExit("The binaries need the Visual C++ runtime but no redistributable folder was "
                         "found; pass --vc-runtime <...\\x64\\Microsoft.VC143.CRT> or --vc-runtime none")
    for source in vc_runtime:
        shutil.copyfile(source, tree / source.name.lower())
    ffmpeg = common.verified_ffmpeg(args.ffmpeg)
    if ffmpeg:
        shutil.copyfile(ffmpeg, tree / "ffmpeg.exe")
    common.stage_resources(tree)
    licenses = tree / "licenses"
    common.stage_all_licenses(licenses)
    shutil.copyfile(runtime / "onnxruntime-LICENSE.txt", licenses / "onnxruntime-LICENSE.txt")
    sources = []
    if ffmpeg:
        notice, sources = common.stage_ffmpeg_notices(ffmpeg, licenses, args.out, "ffmpeg.exe")
    else:
        notice = "FFmpeg is not bundled.\n"
    (licenses / "FFMPEG.txt").write_text(notice, newline="\r\n")
    shutil.copyfile(ICON, tree / "overtalk.ico")
    return {"onnxruntime": {name: digest(tree / name) for name in ONNXRUNTIME},
            "vc_runtime": {path.name.lower(): digest(path) for path in vc_runtime},
            "vc_runtime_source": str(redist) if vc_runtime else None,
            "ffmpeg": common.ffmpeg_record(ffmpeg, sources)}


def nsis_string(text):
    text = str(text)
    if '"' in text:
        raise SystemExit(f"Cannot package a path containing a quote: {text}")
    return text.replace("$", "$$")


def installed_path(relative):
    """`$INSTDIR\\a\\b` for a path relative to the staged folder."""
    return "\\".join(["$INSTDIR", *(nsis_string(part) for part in relative.parts)])


def nsis_file_lists(tree):
    """Install and uninstall macros naming every staged file, so the uninstaller
    removes exactly what the installer wrote and never the user's other files."""
    install, uninstall = ["!macro OVERTALK_INSTALL_FILES"], ["!macro OVERTALK_UNINSTALL_FILES"]
    files = sorted(path for path in tree.rglob("*") if path.is_file())
    # Every folder holding a file, and every folder between those and the root.
    directories = {parent for path in files for parent in path.relative_to(tree).parents}
    for directory in sorted(directories, key=lambda path: (len(path.parts), path.as_posix())):
        contents = [path for path in files if path.parent == tree / directory]
        if not contents:
            continue
        install.append(f'  SetOutPath "{installed_path(directory)}"')
        for path in contents:
            install.append(f'  File "{nsis_string(path)}"')
            uninstall.append(f'  Delete "{installed_path(path.relative_to(tree))}"')
    for directory in sorted(directories, key=lambda path: (-len(path.parts), path.as_posix())):
        if directory.parts:
            uninstall.append(f'  RMDir "{installed_path(directory)}"')
    install.append('  SetOutPath "$INSTDIR"')
    return "\n".join([*install, "!macroend", "", *uninstall, "!macroend", ""])


def makensis():
    for candidate in [os.environ.get("MAKENSIS"), shutil.which("makensis"),
                      Path(os.environ.get("ProgramFiles(x86)", r"C:\Program Files (x86)")) / "NSIS/makensis.exe",
                      Path(os.environ.get("ProgramFiles", r"C:\Program Files")) / "NSIS/makensis.exe"]:
        if candidate and Path(candidate).is_file():
            return Path(candidate)
    raise SystemExit("makensis was not found: install NSIS 3 (choco install nsis) or set MAKENSIS")


def build_installer(args, tree, version, work, signer):
    files = work / "files.nsh"
    files.write_text(nsis_file_lists(tree), encoding="utf-8")
    name = args.display_name.replace(" ", "-")
    installer = args.out / f"{name}-{version}-windows-x64-setup.exe"
    defines = {"APP_NAME": args.display_name, "REGISTRY_NAME": REGISTRY_NAME, "VERSION": version,
               "VERSION_NUMERIC": common.numeric_version(version, 4), "STAGE": tree, "FILES": files,
               "OUTFILE": installer, "PUBLISHER": "Jude Mingay", "HOMEPAGE": common.HOMEPAGE}
    if signer:
        # NSIS signs the uninstaller it writes into the installer (%1 is its path).
        defines["SIGN_COMMAND"] = uninstaller_signer(signer, work)
    run([makensis(), "-V3", "-INPUTCHARSET", "UTF8",
         *(f"-D{key}={value}" for key, value in defines.items()), HERE / "overtalk.nsi"])
    sign(signer, [installer])
    return installer


def main():
    os.umask(0o022)
    parser = argparse.ArgumentParser(description=__doc__)
    common.common_arguments(parser, "windows")
    parser.add_argument("--formats", default="installer,zip", help="Comma-separated: installer, zip")
    parser.add_argument("--vc-runtime", help="Visual C++ runtime redistributable folder, or 'none'; default found with vswhere")
    args = common.resolve_arguments(parser.parse_args())
    formats = set(args.formats.split(","))
    if not formats <= {"installer", "zip"}:
        raise SystemExit(f"Unknown format in {args.formats}")
    if not args.skip_build:
        # llama.cpp picks its instruction set from Rust's target features;
        # without this it is built for SSE2 only, which makes notes very slow.
        msvc = "msvc" in (args.target or common.output(["rustc", "-vV"]))
        common.build(args, windows=True, llama_rustflags=["-C", "target-cpu=x86-64-v3"], c_prefix_map=not msvc)
    binaries = common.built_binaries(args, windows=True)
    epoch = common.source_epoch()
    args.version = args.version or common.default_version(epoch, args.engine_manifest)
    args.out.mkdir(parents=True, exist_ok=True)
    signer = signing_command(args.display_name)
    artifacts, sources = [], []
    with tempfile.TemporaryDirectory(prefix="overtalk-package-", dir=args.out.parent) as temporary:
        work = Path(temporary)
        tree = work / args.display_name
        bundled = stage(args, binaries, tree, signer)
        sources = [args.out / name for name in bundled["ffmpeg"]["source_files"]]
        if "installer" in formats:
            artifacts.append(build_installer(args, tree, args.version, work, signer))
        if "zip" in formats:
            name = args.display_name.replace(" ", "-")
            artifacts.append(common.deterministic_zip(
                tree, args.out / f"{name}-{args.version}-windows-x64-portable.zip", epoch, args.display_name))
    info = common.build_record(args, binaries, epoch, {
        **bundled, "signed": bool(signer),
        "artifacts": {path.name: digest(path) for path in artifacts}})
    record = args.out / f"{args.package}-windows-build-info.json"
    record.write_text(json.dumps(info, indent=2) + "\n", newline="\n")
    sums = common.write_checksums(args.out, [*artifacts, *sources, record], args.sign_key)
    for path in [*artifacts, record, sums]:
        print(f"{path}  {path.stat().st_size / 1e6:.1f} MB")


if __name__ == "__main__":
    main()
