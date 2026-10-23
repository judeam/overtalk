"""Run a real sandboxed workload through an installed `overtalk-worker`.

The launcher gets the same policy the engine writes (see
`crates/worker-sandbox`), and runs its own `--isolation-probe` inside the
sandbox: one granted file readable, a sibling file, the granted input's
writability, the host network and an environment secret all checked from
within. It also checks that LD_LIBRARY_PATH reaches the workload. With
`--expect-blocked` it instead requires bubblewrap to fail the way Ubuntu's
AppArmor restriction makes it fail, which proves a later pass is due to the
profile. With `--injection-attack` it injects a library through LD_PRELOAD that,
inside the launcher, tries to create a user namespace, to run bubblewrap with
itself preloaded and to ptrace and process_vm_writev a running bubblewrap, as a
local attacker could, and requires every one of those to be refused. Linux only.
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
LIBRARY_PATH = "/opt/overtalk-test-libs"
DENIED = (1, 13)  # EPERM, EACCES

# Code a local attacker injects into the launcher. It writes what happened to ATTACK_OUT.
# If the library is ever loaded into bubblewrap it says so there.
ATTACK = r"""
#define _GNU_SOURCE
#include <errno.h>
#include <limits.h>
#include <sched.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ptrace.h>
#include <sys/uio.h>
#include <sys/wait.h>
#include <unistd.h>

static void note(const char *text) {
  FILE *out = fopen(getenv("ATTACK_OUT"), "a");
  if (out) { fputs(text, out); fclose(out); }
}

static void number(const char *name, long value, int with_errno) {
  char text[128];
  if (with_errno) snprintf(text, sizeof text, "%s=%ld errno=%d\n", name, value, value < 0 ? errno : 0);
  else snprintf(text, sizeof text, "%s=%ld\n", name, value);
  note(text);
}

__attribute__((constructor)) static void attack(void) {
  char exe[PATH_MAX] = {0};
  if (readlink("/proc/self/exe", exe, sizeof exe - 1) <= 0) return;
  const char *name = strrchr(exe, '/') ? strrchr(exe, '/') + 1 : exe;
  if (!strcmp(name, "bwrap")) {
    number("loaded_in_bwrap_unshare", unshare(CLONE_NEWUSER), 0);
    return;
  }
  if (strcmp(name, "overtalk-worker")) return;
  number("unshare", unshare(CLONE_NEWUSER), 0);

  char preload[PATH_MAX + 16], output[PATH_MAX + 16];
  snprintf(preload, sizeof preload, "LD_PRELOAD=%s", getenv("ATTACK_LIB"));
  snprintf(output, sizeof output, "ATTACK_OUT=%s", getenv("ATTACK_OUT"));
  int status = 0;
  pid_t child = fork();
  if (child == 0) {
    char *argv[] = {"bwrap", "--unshare-user", "--unshare-net", "--ro-bind", "/", "/", "/bin/true", NULL};
    char *env[] = {preload, output, NULL};
    execve("/usr/bin/bwrap", argv, env);
    _exit(127);
  }
  waitpid(child, &status, 0);
  number("bwrap_preload_exit", WIFEXITED(status) ? WEXITSTATUS(status) : -1, 0);

  // `--args` makes bubblewrap wait for its arguments on a pipe nobody writes to. It is then
  // still in the caller's user namespace, where only the security policy stops a trace.
  int pipes[2];
  if (pipe(pipes)) return;
  child = fork();
  if (child == 0) {
    char descriptor[16];
    close(pipes[1]);
    snprintf(descriptor, sizeof descriptor, "%d", pipes[0]);
    char *argv[] = {"bwrap", "--args", descriptor, NULL};
    char *env[] = {"PATH=/usr/bin:/bin", NULL};
    execve("/usr/bin/bwrap", argv, env);
    _exit(127);
  }
  usleep(700000);
  char path[64], comm[32] = {0};
  snprintf(path, sizeof path, "/proc/%d/comm", child);
  FILE *file = fopen(path, "r");
  if (file) { if (fgets(comm, sizeof comm, file)) comm[strcspn(comm, "\n")] = 0; fclose(file); }
  number("target_is_bwrap", !strcmp(comm, "bwrap"), 0);
  errno = 0;
  long attached = ptrace(PTRACE_ATTACH, child, 0, 0);
  number("ptrace_attach", attached, 1);
  if (attached == 0) { waitpid(child, &status, 0); ptrace(PTRACE_DETACH, child, 0, 0); }
  char buffer[8] = "hijacked";
  struct iovec local = {buffer, sizeof buffer}, remote = {(void *)0x400000, sizeof buffer};
  errno = 0;
  number("process_vm_writev", process_vm_writev(child, &local, 1, &remote, 1, 0), 1);
  close(pipes[1]);
  kill(child, SIGKILL);
  waitpid(child, &status, 0);
}
"""


def injection_attack(worker):
    with tempfile.TemporaryDirectory(prefix="overtalk-attack-") as name:
        source, library, result = Path(name) / "attack.c", Path(name) / "attack.so", Path(name) / "result"
        source.write_text(ATTACK)
        subprocess.run(["gcc", "-shared", "-fPIC", "-o", library, source], check=True)
        # A launcher that is not run in a sandbox ends with a usage error; only the constructor matters.
        subprocess.run([worker], env={"PATH": "/usr/bin:/bin", "LD_PRELOAD": str(library), "ATTACK_OUT": str(result),
                                      "ATTACK_LIB": str(library)}, capture_output=True, timeout=60)
        lines = result.read_text().splitlines() if result.exists() else []
    print("Injected code reported:", "; ".join(lines))
    report = dict(line.split("=", 1) for line in lines)
    failures = []
    if report.get("unshare") != "-1":
        failures.append("the launcher could create a user namespace")
    if "loaded_in_bwrap_unshare" in report:
        failures.append(f"the preloaded library was loaded into bubblewrap (unshare={report['loaded_in_bwrap_unshare']})")
    if report.get("bwrap_preload_exit") != "0":
        failures.append("bubblewrap did not run normally with the library preloaded")
    if report.get("target_is_bwrap") != "1":
        failures.append("the attack did not find a running bubblewrap to trace")
    for key in ("ptrace_attach", "process_vm_writev"):
        value = report.get(key, "").split()
        denied = len(value) == 2 and value[0] == "-1" and int(value[1].removeprefix("errno=")) in DENIED
        if not denied:
            failures.append(f"{key} on bubblewrap was not refused ({report.get(key)})")
    if failures:
        sys.exit("Injected code was not contained: " + "; ".join(failures))
    print("Injected code cannot create a user namespace, load into bubblewrap, or trace it")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("worker", type=Path, help="Installed overtalk-worker")
    parser.add_argument("--injection-attack", action="store_true", help="Also require the attacks of an injected library to be refused")
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
        library_path = None
        if not args.expect_blocked:
            # The library path must still reach the workload, now through bubblewrap's --setenv.
            library_path = subprocess.run([worker, policy, "/usr/bin/env"], capture_output=True, text=True,
                                          env={"PATH": "/usr/bin:/bin", "LD_LIBRARY_PATH": LIBRARY_PATH})
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
    if library_path.returncode != 0 or f"LD_LIBRARY_PATH={LIBRARY_PATH}" not in library_path.stdout.splitlines():
        sys.exit(f"LD_LIBRARY_PATH did not reach the workload: {library_path.stdout!r} {library_path.stderr.strip()}")
    print("LD_LIBRARY_PATH reached the sandboxed workload")
    if args.injection_attack:
        injection_attack(worker)


if __name__ == "__main__":
    main()
