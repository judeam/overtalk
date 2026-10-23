"""Run a real sandboxed workload through an installed `overtalk-worker`.

The launcher gets the same policy the engine writes (see
`crates/worker-sandbox`), and runs its own `--isolation-probe` inside the
sandbox: one granted file readable, a sibling file, the granted input's
writability, the host network and an environment secret all checked from
within. With `--expect-blocked` it instead requires bubblewrap to fail the way
Ubuntu's AppArmor restriction makes it fail, which proves a later pass is due to
the profile. With `--injection-attack` it injects a library through LD_PRELOAD
that tries to create a user namespace inside the launcher, as a local attacker
could, and requires that to be denied. Linux only.
"""
import argparse
import json
from pathlib import Path
import socket
import subprocess
import sys
import tempfile

# What bubblewrap prints when it cannot set up the sandbox's namespaces.
BLOCKED = ("RTM_NEWADDR", "uid map", "Operation not permitted", "Permission denied", "Creating new namespace failed")
EXPECTED = {"read_allowed": True, "write_input": False, "read_denied": False, "write_output": True,
            "host_network": False, "inherited_secret": False}


ATTACK = r"""
#define _GNU_SOURCE
#include <sched.h>
#include <stdio.h>
#include <stdlib.h>
__attribute__((constructor)) static void attack(void) {
  FILE *out = fopen(getenv("ATTACK_OUT"), "a");
  fprintf(out, "%d\n", unshare(CLONE_NEWUSER));
  fclose(out);
}
"""


def injection_attack(worker):
    with tempfile.TemporaryDirectory(prefix="overtalk-attack-") as name:
        source, library, result = Path(name) / "attack.c", Path(name) / "attack.so", Path(name) / "result"
        source.write_text(ATTACK)
        subprocess.run(["gcc", "-shared", "-fPIC", "-o", library, source], check=True)
        # A launcher that is not run in a sandbox ends with a usage error; only the constructor matters.
        subprocess.run([worker], env={"PATH": "/usr/bin:/bin", "LD_PRELOAD": str(library), "ATTACK_OUT": str(result)},
                       capture_output=True)
        outcome = result.read_text().split() if result.exists() else []
    if outcome != ["-1"]:
        sys.exit(f"Injected code in the launcher was not denied a user namespace (unshare returned {outcome})")
    print("Injected code in the launcher cannot create a user namespace")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("worker", type=Path, help="Installed overtalk-worker")
    parser.add_argument("--injection-attack", action="store_true", help="Also require a preloaded library to be denied user namespaces")
    parser.add_argument("--expect-blocked", action="store_true", help="Require the sandbox to fail to start")
    args = parser.parse_args()
    worker = args.worker.resolve()
    with tempfile.TemporaryDirectory(prefix="overtalk-worker-") as name, \
            socket.socket() as listener:
        root = Path(name)
        for folder in ["inputs", "work", "home", "cache"]:
            (root / folder).mkdir()
        allowed, denied, output = root / "inputs/allowed.txt", root / "secret.txt", root / "work/output.txt"
        allowed.write_text("permitted")
        denied.write_text("private")
        output.write_text("")
        listener.bind(("127.0.0.1", 0))
        listener.listen()
        policy = root / "policy.json"
        policy.write_text(json.dumps({
            "read": [str(root / "inputs"), str(worker)],
            "write": [str(root / "work"), str(root / "home"), str(root / "cache")],
            "root": str(root), "cwd": str(root / "work"), "gpu": False, "wrapper": None}))
        address = "%s:%d" % listener.getsockname()
        result = subprocess.run([worker, policy, worker, "--isolation-probe", allowed, denied, output, address],
                                capture_output=True, text=True, env={"PATH": "/usr/bin:/bin", "OVERTALK_TEST_SECRET": "x"})
        output_text = output.read_text()
    if args.expect_blocked:
        if result.returncode == 0:
            sys.exit("The sandbox started although it was expected to be blocked")
        if not any(text in result.stderr for text in BLOCKED):
            sys.exit(f"Failed, but not like a namespace restriction: {result.stderr.strip()}")
        print(f"Blocked as expected: {result.stderr.strip()}")
        return
    if result.returncode != 0:
        sys.exit(f"The sandbox did not start (exit {result.returncode}): {result.stderr.strip()}")
    report = json.loads(result.stdout)
    if report != EXPECTED or output_text != "permitted output":
        sys.exit(f"Isolation differs from the policy: {report}, output file {output_text!r}")
    print(f"Sandboxed worker ran and was isolated: {report}")
    if args.injection_attack:
        injection_attack(worker)


if __name__ == "__main__":
    main()
