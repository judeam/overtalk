#!/usr/bin/env python3
"""Run local quality scanners with reviewed entry points and save their evidence.

Provision the pinned tools in docs/QUALITY-TOOLS.md first.
This command never installs tools, uploads source or applies automatic fixes.
Clones are review candidates; unreachable files/dependencies and Python errors
are gates. Compiler, feature-matrix and runtime checks remain separate.
"""
import argparse
import json
from pathlib import Path
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    for name in ("knip", "jscpd", "machete", "ruff"):
        parser.add_argument("--" + name, default="cargo-machete" if name == "machete" else name)
    args = parser.parse_args()
    output = args.output.resolve()
    # Fail before starting scanners if the tool environment is incomplete.
    tools = {}
    for name in ("knip", "jscpd", "machete", "ruff"):
        path = shutil.which(getattr(args, name))
        if not path:
            parser.error(f"Missing {name}; provision it or pass --{name} /path/to/tool")
        tools[name] = str(Path(path).resolve())
    output.mkdir(parents=True, exist_ok=True)
    jobs = {
        "knip": (ROOT / "frontend", ["--include", "files,exports,types,dependencies,unlisted,unresolved,cycles",
                                     "--reporter", "json", "--no-progress"]),
        "machete": (ROOT, ["--skip-target-dir", "."]),
        "ruff": (ROOT, ["check", "--no-cache", "--no-fix", "--select", "F401,F841,F821,RUF059",
                        "--exclude", "**/vendor/**", "--output-format", "json",
                        "speech-worker/sonefold_speech", "speech-worker/tools", "speech-worker/tests",
                        "scripts", "integrations", "evaluations"]),
        "jscpd": (ROOT, ["native", "frontend/src", "frontend/tests", "frontend/scripts",
                          "frontend/src-tauri/src", "frontend/src-tauri/tests", "frontend/src-tauri/build.rs",
                          "crates", "llama-helper/src", "speech-worker/sonefold_speech",
                          "speech-worker/tests", "speech-worker/tools", "scripts", "integrations", "evaluations", ".github",
                          "--workers", "3", "--format", "rust,python,javascript,jsx,typescript,tsx",
                          "--min-lines", "10", "--min-tokens", "80", "--reporters", "json",
                          "--fail-on-empty", "--ignore",
                          "**/vendor/**,**/target/**,**/node_modules/**,**/dispatch.rs,**/web_dispatch.rs",
                          "--output", str(output / "duplicates")]),
    }
    report = {}
    for name, (cwd, options) in jobs.items():
        with (output / f"{name}.stdout").open("w") as stdout, (output / f"{name}.stderr").open("w") as stderr:
            try:
                result = subprocess.run([tools[name], *options], cwd=cwd, stdout=stdout,
                                        stderr=stderr, timeout=180, check=False)
                report[name] = {"exit_code": result.returncode}
            except subprocess.TimeoutExpired:
                report[name] = {"exit_code": -1, "error": "180-second timeout"}
        print(f"{name}: exit {report[name]['exit_code']}", flush=True)
    (output / "result.json").write_text(json.dumps(report, indent=2) + "\n")
    return int(any(row["exit_code"] != 0 for row in report.values()))


if __name__ == "__main__":
    raise SystemExit(main())
