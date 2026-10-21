#!/usr/bin/env python3
"""Scan source and optional Git history locally. Never print secret values."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
VERSION = "8.30.1"
LINUX_X64_SHA256 = "551f6fc83ea457d62a0d98237cbad105af8d557003051f41f3e7ca7b3f2470eb"
MAX_FILE = 16 * 1024 * 1024


def download(directory):
    import platform
    if platform.system() != "Linux" or platform.machine() not in {"x86_64", "AMD64"}:
        raise RuntimeError("Supply --binary on platforms other than Linux x64")
    archive = directory / "gitleaks.tar.gz"
    url = f"https://github.com/gitleaks/gitleaks/releases/download/v{VERSION}/gitleaks_{VERSION}_linux_x64.tar.gz"
    urllib.request.urlretrieve(url, archive)
    if hashlib.sha256(archive.read_bytes()).hexdigest() != LINUX_X64_SHA256:
        raise RuntimeError("Gitleaks download did not match the pinned SHA-256")
    with tarfile.open(archive) as bundle:
        entry = bundle.getmember("gitleaks")
        if not entry.isfile():
            raise RuntimeError("Invalid Gitleaks executable")
        executable = directory / "gitleaks"
        executable.write_bytes(bundle.extractfile(entry).read())
    executable.chmod(0o700)
    return str(executable)


def scan(binary, kind, source, directory, history=False):
    report = directory / f"{kind}.json"
    command = [binary, kind, str(source), "--no-banner", "--redact=100",
               "--config", str(ROOT / ".gitleaks.toml"), "--report-format=json",
               "--report-path", str(report)]
    if history:
        command.append("--log-opts=--all")
    result = subprocess.run(command, cwd=ROOT, capture_output=True, text=True)
    if result.returncode not in (0, 1) or not report.exists():
        # Do not forward scanner diagnostics: a future tool version could expose
        # source values even though redaction was requested.
        raise RuntimeError(f"Gitleaks {kind} scan failed (exit {result.returncode})")
    findings = []
    for row in json.loads(report.read_text()):
        path = Path(row["File"])
        if kind == "dir" and path.is_absolute():
            path = path.relative_to(source)
        findings.append({"rule": row["RuleID"], "file": str(path),
                         "line": row["StartLine"], "commit": row.get("Commit", "")})
    return findings


def verify_scanner(binary, directory):
    """Prove narrow vendor exceptions still detect keys in those same files."""
    source = directory / "scanner-canary"
    source.mkdir(mode=0o700)
    # A synthetic value, constructed at runtime; never a provider credential.
    canary = hashlib.sha256(b"Overtalk secret-scan negative control").hexdigest()
    macros = "".join(
        f"#define sqlite3_{name} sqlite3_api->{name}\n"
        for name in ("column_bytes16", "soft_heap_limit64", "hard_heap_limit64")
    )
    fts_row = "if( {}=={} ){{\n".format(
        "iKey", "_".join(("FTS5", "AVERAGES", "ROWID"))
    )
    fixtures = {
        "crates/vendor/libsqlite3-sys/sqlite3/sqlite3.c": macros + fts_row,
        "crates/vendor/libsqlite3-sys/sqlite3/sqlite3ext.h": macros,
        "canary.py": "",
    }
    for name, public_code in fixtures.items():
        path = source / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(public_code + f'api_key = "{canary}"\n')
        path.chmod(0o600)
    findings = scan(binary, "dir", source, directory)
    if len(findings) != len(fixtures) or {
        row["file"] for row in findings
    } != set(fixtures) or any(row["rule"] != "generic-api-key" for row in findings):
        raise RuntimeError("Secret scanner failed its vendor-exception canary")
    if canary in (directory / "dir.json").read_text():
        raise RuntimeError("Secret scanner failed to redact its synthetic canary")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", default=shutil.which("gitleaks"))
    parser.add_argument("--download", action="store_true")
    parser.add_argument("--history", action="store_true")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    paths = set(subprocess.check_output(
        ["git", "ls-files", "-co", "--exclude-standard", "-z"], cwd=ROOT
    ).decode().split("\0")) - {""}
    # Also inspect common ignored local configuration locations, but never
    # copy model caches, recordings or dependency trees into the scanner.
    for base in (ROOT, ROOT / "frontend", ROOT / "speech-worker", ROOT / "integrations"):
        for pattern in (".env*", "*.pem", "*.key", "*credentials*.json", "*service-account*.json"):
            for path in base.glob(pattern):
                if path.is_file():
                    paths.add(str(path.relative_to(ROOT)))
    count = size = 0
    exclusions = []
    with tempfile.TemporaryDirectory(prefix="overtalk-secret-audit-") as temp:
        private = Path(temp)
        private.chmod(0o700)
        binary = download(private) if args.download else args.binary
        if not binary:
            raise RuntimeError("Install Gitleaks, supply --binary, or use --download")
        verify_scanner(binary, private)
        snapshot = private / "source"
        snapshot.mkdir(mode=0o700)
        for name in sorted(paths):
            path = ROOT / name
            if path.is_symlink() or not path.is_file():
                continue
            with path.open("rb") as stream:
                if b"\0" in stream.read(8192):
                    continue
            if path.stat().st_size > MAX_FILE:
                exclusions.append(name)
                continue
            data = path.read_bytes()
            destination = snapshot / name
            destination.parent.mkdir(parents=True, exist_ok=True)
            destination.write_bytes(data)
            destination.chmod(0o600)
            count += 1
            size += len(data)
        findings = scan(binary, "dir", snapshot, private)
        if args.history:
            findings += scan(binary, "git", ROOT, private, history=True)
    report = {"scanner": "gitleaks", "canary_checked": True,
              "files": count, "text_bytes": size,
              "history": args.history, "large_file_exclusions": exclusions, "findings": findings}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    print(f"Secret scan: {count} text files; history={args.history}; findings={len(findings)}")
    for finding in findings:
        print(f"{finding['rule']}: {finding['file']}:{finding['line']}")
    raise SystemExit(1 if findings else 0)


if __name__ == "__main__":
    try:
        main()
    except Exception as error:
        raise SystemExit(f"Secret audit failed: {type(error).__name__}") from None
