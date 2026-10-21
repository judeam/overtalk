"""Fail CI on reachable advisories; retain the complete upstream audit report."""
import argparse
import json
from pathlib import Path
import re
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]


def run(command, cwd=ROOT):
    result = subprocess.run(command, cwd=cwd, text=True, capture_output=True)
    if result.returncode not in (0, 1):
        raise RuntimeError(result.stderr or result.stdout)
    return result


def rust():
    result = run(["cargo", "audit", "--json"])
    report = json.loads(result.stdout)
    if "vulnerabilities" not in report:
        raise RuntimeError("cargo audit did not return vulnerability results")
    tree = run(["cargo", "tree", "--locked", "--workspace", "--target", "all",
                "--edges", "normal,build", "--prefix", "none", "--format", "{p}"])
    if tree.returncode:
        raise RuntimeError(tree.stderr)
    active = set(re.findall(r"^([\w-]+) v([^\s]+)", tree.stdout, re.MULTILINE))
    failures = []
    for finding in report["vulnerabilities"]["list"]:
        package = finding["package"]
        reachable = (package["name"], package["version"]) in active
        finding["reachable_in_workspace_build"] = reachable
        label = "FAIL" if reachable else "INACTIVE OPTIONAL DEPENDENCY"
        print(f'{label}: {finding["advisory"]["id"]} {package["name"]} {package["version"]}')
        if reachable:
            failures.append(finding)
    for kind, warnings in report.get("warnings", {}).items():
        for warning in warnings:
            package = warning.get("package", {})
            print(f'UPSTREAM {kind}: {package.get("name", "")} {package.get("version", "")}')
    return report, failures


def node():
    # These exceptions are conditional on tests of the installed dependencies,
    # not just the version strings in the lockfile.
    subprocess.run(["node", ".github/verify-dependency-security.cjs"], cwd=ROOT, check=True)
    result = run(["pnpm", "audit", "--prod", "--json"], ROOT / "frontend")
    report = json.loads(result.stdout)
    if "advisories" not in report:
        raise RuntimeError("pnpm audit did not return advisory results")
    verified = {
        ("GHSA-cp6q-959q-f8rh", "@tiptap/core", "2.27.3"),
        ("GHSA-vfj7-8cjw-p6xm", "braces", "3.0.3"),
    }
    failures = []
    for advisory in report["advisories"].values():
        ghsa = advisory.get("github_advisory_id", "")
        if not ghsa:
            match = re.search(r"GHSA-[\w-]+", advisory.get("url", ""))
            ghsa = match.group(0) if match else ""
        for finding in advisory["findings"]:
            patched = (ghsa, advisory["module_name"], finding["version"]) in verified
            finding["verified_patch_regression"] = patched
            label = "VERIFIED PATCH" if patched else "FAIL"
            print(f'{label}: {ghsa} {advisory["module_name"]} {finding["version"]}')
            if not patched:
                failures.append(finding)
    return report, failures


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("ecosystem", choices=["rust", "node"])
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    report, failures = rust() if args.ecosystem == "rust" else node()
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    if failures:
        raise SystemExit(f"{len(failures)} unpatched reachable dependency findings")
    print("Dependency audit passed; complete report saved to", args.output)


if __name__ == "__main__":
    try:
        main()
    except (ValueError, RuntimeError, subprocess.CalledProcessError) as error:
        print(error, file=sys.stderr)
        raise SystemExit(1)
