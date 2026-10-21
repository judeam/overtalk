#!/usr/bin/env python3
"""Check a standalone synthetic native fixture with Memcheck and a fault canary."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile
import xml.etree.ElementTree as ET


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--valgrind", default="valgrind")
    parser.add_argument("--debug-info", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    args.output = args.output.resolve()
    args.output.parent.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ, DEBUGINFOD_URLS="")
    version = subprocess.check_output([args.valgrind, "--version"], env=env, text=True, timeout=10).strip()
    command = [args.valgrind, "--tool=memcheck", "--enable-debuginfod=no",
               "--leak-check=full", "--show-leak-kinds=all",
               "--errors-for-leak-kinds=definite,indirect,possible",
               "--track-origins=yes", "--error-exitcode=97", "--xml=yes"]
    if args.debug_info:
        command += [f"--extra-debuginfo-path={args.debug_info.resolve()}"]
    with tempfile.TemporaryDirectory(prefix="overtalk-memory-canary-") as temp:
        root = Path(temp)
        source, binary, report = root / "canary.c", root / "canary", root / "canary.xml"
        # Deliberate use-after-free in this disposable test process only.
        source.write_text("#include <stdlib.h>\nint main(void) { char *p=malloc(8); if(!p) return 1; p[0]=0; free(p); return *(volatile char*)p; }\n")
        subprocess.run(["cc", "-O0", "-g", str(source), "-o", str(binary)], check=True, timeout=30)
        canary = subprocess.run([*command, f"--xml-file={report}", str(binary)], env=env,
                                capture_output=True, timeout=30)
        assert canary.returncode == 97 and any(
            e.findtext("kind") == "InvalidRead" for e in ET.parse(report).getroot().findall("error")
        ), "Memcheck did not detect the intentional invalid read"
    xml = args.output.with_suffix(".xml")
    result = subprocess.run([*command, f"--xml-file={xml}", str(args.binary.resolve())],
                            env=env, capture_output=True, text=True, timeout=180)
    args.output.with_suffix(".log").write_text(result.stdout + result.stderr)
    root = ET.parse(xml).getroot()
    errors = [{"kind": e.findtext("kind"), "description": e.findtext("what") or e.findtext("xwhat/text")}
              for e in root.findall("error")]
    failures = [e for e in errors if e["kind"] != "Leak_StillReachable"]
    finished = any(s.findtext("state") == "FINISHED" for s in root.findall("status"))
    payload = dict(tool=version, canary=True, exit_code=result.returncode,
                   completed=finished, findings=errors, passed=result.returncode == 0 and finished and not failures)
    args.output.write_text(json.dumps(payload, indent=2) + "\n")
    print(json.dumps(payload, indent=2))
    raise SystemExit(0 if payload["passed"] else 1)


if __name__ == "__main__":
    main()
