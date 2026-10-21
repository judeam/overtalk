#!/usr/bin/env python3
"""Run the local Semgrep security regression rules and their positive/negative canaries.

No registry, login, source upload or telemetry. The wider investigative scan and
its false-positive review are recorded separately in the audit report.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--semgrep", default="semgrep")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    output = args.output.resolve()
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="overtalk-semgrep-") as temp:
        env = dict(os.environ, SEMGREP_SETTINGS_FILE=f"{temp}/settings.yml",
                   SEMGREP_LOG_FILE=f"{temp}/scan.log")
        # An inherited login must not change a local CI regression scan's scope.
        env.pop("SEMGREP_APP_TOKEN", None)
        base = [args.semgrep, "scan", "--metrics=off", "--disable-version-check",
                "--config", str(root / ".semgrep/security.yml")]
        for fixture in sorted((root / ".semgrep/tests").glob("security.*")):
            subprocess.run([*base, "--test", str(fixture)],
                           env=env, cwd=root, check=True, timeout=120)
        scan = subprocess.run([*base, "--error", "--strict", "--jobs", "2",
                               "--json", "--output", str(output),
                               "--exclude", "crates/vendor", "--exclude", "target",
                               "frontend/src-tauri/src", "native/src", "crates",
                               "speech-worker", "integrations", "scripts"],
                              env=env, cwd=root, timeout=180)
        if scan.returncode:
            raise SystemExit(scan.returncode)
        report = json.loads(output.read_text())
        if report.get("errors") or report.get("results") or not report["paths"]["scanned"]:
            raise SystemExit("Static analysis did not complete cleanly")
        print(f"Local security rules passed on {len(report['paths']['scanned'])} files; canaries passed.")


if __name__ == "__main__":
    main()
