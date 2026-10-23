"""Cut a release, and the checks and notes the Release workflow runs for it.

    python scripts/release.py 0.2.0           # set the version, commit, tag v0.2.0
    python scripts/release.py 0.2.0 --push    # ... and push main and the tag
    python scripts/release.py --check v0.2.0  # the workflow's tag check
    python scripts/release.py --notes 0.2.0 --packages DIR

Pushing the tag starts .github/workflows/release.yml, which builds, tests and
publishes the packages. Write the version's CHANGELOG section first.
"""
import argparse
import json
from pathlib import Path
import re
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
# Every manifest whose version people or other programs see; packages take theirs
# from overtalk-native.
CARGO_MANIFESTS = ["native/Cargo.toml", "frontend/src-tauri/Cargo.toml", "crates/cli/Cargo.toml"]
JSON_MANIFESTS = ["frontend/src-tauri/tauri.conf.json", "frontend/package.json"]
TAG = re.compile(r"^v(\d+\.\d+\.\d+(?:-[0-9A-Za-z.]+)?)$")
PACKAGE_VERSION = re.compile(r'(?m)^(version\s*=\s*")([^"]+)(")')


def versions(root=ROOT):
    found = {}
    for name in CARGO_MANIFESTS:
        match = PACKAGE_VERSION.search((root / name).read_text())
        found[name] = match[2] if match else "no version"
    for name in JSON_MANIFESTS:
        found[name] = json.loads((root / name).read_text())["version"]
    return found


def changelog_section(version, root=ROOT):
    """The CHANGELOG text under `## <version>`, up to the next `## ` heading."""
    text = (root / "CHANGELOG.md").read_text()
    match = re.search(rf"(?m)^## {re.escape(version)}(?=[:\s]|$).*\n", text)
    if not match:
        return None
    rest = text[match.end():]
    following = re.search(r"(?m)^## ", rest)
    return rest[:following.start() if following else None].strip()


def check(tag, root=ROOT):
    """Problems that stop `tag` from being released; empty when it can go out."""
    match = TAG.match(tag)
    if not match:
        return [f"{tag} is not a version tag such as v0.2.0 or v0.2.0-rc.1"]
    version = match[1]
    problems = [f"{name} says {found}, not {version}" for name, found in versions(root).items() if found != version]
    if not changelog_section(version, root):
        problems.append(f"CHANGELOG.md has no '## {version}' section")
    return problems


def notes(version, packages, root=ROOT):
    """Release notes: the changelog section, what to download and how each build is signed."""
    files = sorted(path.name for path in Path(packages).iterdir())
    records = {path.name: json.loads(path.read_text()) for path in Path(packages).glob("*build-info.json")}
    windows = next((record for name, record in records.items() if "windows" in name), {})
    macos = next((record for name, record in records.items() if "macos" in name), {})
    pick = lambda pattern: next((name for name in files if re.search(pattern, name)), None)
    downloads = [
        ("Linux (Ubuntu 24.04+, Debian 13+), `.deb`", pick(r"_amd64\.deb$")),
        ("Linux, other distributions, AppImage", pick(r"\.AppImage$")),
        ("Windows 10/11 x64, installer", pick(r"-setup\.exe$")),
        ("Windows 10/11 x64, portable", pick(r"-portable\.zip$")),
        ("macOS 14.2+ (Apple silicon), `.dmg`", pick(r"\.dmg$")),
    ]
    # Relative links in the changelog point into the repository at this tag.
    section = re.sub(r"\]\((?!https?://|#)([^)]+)\)", rf"](https://github.com/judeam/overtalk/blob/v{version}/\1)",
                     changelog_section(version, root) or "")
    lines = [section, "", "## Downloads", ""]
    lines += [f"- {label}: `{name}`" for label, name in downloads if name]
    lines += ["", "Verify a download with `sha256sum -c --ignore-missing SHA256SUMS`, or its build",
              "provenance with `gh attestation verify <file> --repo judeam/overtalk`.", ""]
    if windows and not windows.get("signed"):
        lines.append("- **Windows:** not code-signed yet, so SmartScreen warns; choose More info → Run anyway.")
    if macos and macos.get("signature") != "developer-id":
        lines.append("- **macOS:** not notarised yet; after the first launch is blocked, open System Settings → "
                     "Privacy & Security → Open Anyway.")
    lines += ["- Recording is tested on Linux; Windows and macOS builds pass CI but have not yet been tried on "
              "real hardware ([release status](https://github.com/judeam/overtalk/blob/main/docs/RELEASE-STATUS.md)).",
              "- Source code for the bundled FFmpeg and libopus is attached to this release."
              if pick(r"^ffmpeg-.*\.tar\.") else ""]
    return "\n".join(lines).rstrip() + "\n"


def bump(version, push):
    git = lambda *args: subprocess.run(["git", *args], cwd=ROOT, check=True)
    tag = f"v{version}"
    if not TAG.match(tag):
        raise SystemExit(f"{version} is not a version such as 0.2.0 or 0.2.0-rc.1")
    if not changelog_section(version):
        raise SystemExit(f"Write the '## {version}' section of CHANGELOG.md first")
    for name in CARGO_MANIFESTS:
        path = ROOT / name
        path.write_text(PACKAGE_VERSION.sub(rf"\g<1>{version}\g<3>", path.read_text(), count=1))
    for name in JSON_MANIFESTS:
        path = ROOT / name
        path.write_text(re.sub(r'("version"\s*:\s*")[^"]+(")', rf"\g<1>{version}\g<2>", path.read_text(), count=1))
    subprocess.run(["cargo", "update", "--workspace", "--offline"], cwd=ROOT, check=True)
    if problems := check(tag):
        raise SystemExit("\n".join(problems))
    changed = [*CARGO_MANIFESTS, *JSON_MANIFESTS, "Cargo.lock", "CHANGELOG.md"]
    git("add", "--", *changed)
    git("commit", "-m", f"release: Overtalk {version}", "--", *changed)
    git("tag", "-a", tag, "-m", f"Overtalk {version}")
    if push:
        git("push", "origin", "HEAD:main", tag)
    else:
        print(f"Tagged {tag}. Push with: git push origin HEAD:main {tag}")


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("version", nargs="?", help="Version to release, such as 0.2.0")
    parser.add_argument("--push", action="store_true", help="Push main and the tag after tagging")
    parser.add_argument("--check", metavar="TAG", help="Check a tag against the manifests and CHANGELOG")
    parser.add_argument("--notes", metavar="VERSION", help="Print release notes for VERSION")
    parser.add_argument("--packages", type=Path, help="Folder of package files, with --notes")
    args = parser.parse_args()
    if args.check:
        if problems := check(args.check):
            for problem in problems:
                print(f"::error::{problem}", file=sys.stderr)
            raise SystemExit(1)
    elif args.notes:
        if not args.packages:
            parser.error("--notes needs --packages")
        sys.stdout.write(notes(args.notes, args.packages))
    elif args.version:
        bump(args.version, args.push)
    else:
        parser.error("give a version, --check or --notes")


if __name__ == "__main__":
    main()
