#![deny(clippy::undocumented_unsafe_blocks)]
use overtalk_worker_sandbox::Sandbox;
use std::fs;
#[cfg(target_os = "linux")]
use std::process::Stdio;

#[test]
fn installed_worker_identity_matches_the_compiled_sources() {
    let result = std::process::Command::new(env!("CARGO_BIN_EXE_overtalk-worker"))
        .arg("--build-identity")
        .output()
        .unwrap();
    assert!(result.status.success());
    assert_eq!(
        String::from_utf8(result.stdout).unwrap().trim(),
        env!("OVERTALK_WORKER_SOURCE_ID")
    );
}

#[test]
fn launcher_rejects_oversized_policy_before_parsing_or_launching() {
    let directory = tempfile::tempdir().unwrap();
    let policy = directory.path().join("policy.json");
    fs::write(&policy, vec![b' '; 128 * 1024 + 1]).unwrap();
    let executable = env!("CARGO_BIN_EXE_overtalk-worker");
    let result = std::process::Command::new(executable)
        .arg(policy)
        .arg(executable)
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("policy is too large"));
}

#[test]
#[cfg(target_os = "linux")]
fn compiler_can_resolve_its_installation_without_home_access() {
    use std::io::Write;
    let compiler = std::path::Path::new("/usr/bin/gcc");
    if !compiler.is_file() {
        return;
    }
    let mut sandbox = Sandbox::new(false).unwrap();
    let mut child = sandbox
        .command(compiler)
        .unwrap()
        .args(["-x", "c", "-fsyntax-only", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"int main(void) { return 0; }\n")
        .unwrap();
    let result = child.wait_with_output().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}
#[test]
#[cfg(target_os = "linux")]
fn worker_can_read_only_granted_input_and_cannot_reach_host() {
    let directory = tempfile::tempdir().unwrap();
    let allowed = directory.path().join("allowed.txt");
    let denied = directory.path().join("secret.txt");
    fs::write(&allowed, "synthetic permitted input").unwrap();
    fs::write(&denied, "synthetic secret").unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    use std::os::{fd::AsRawFd, unix::process::CommandExt};
    let secret_file = fs::File::open(&denied).unwrap();
    let descriptor = secret_file.as_raw_fd();
    let mut sandbox = Sandbox::new(false).unwrap();
    let input = sandbox.input(&allowed).unwrap();
    let mut command = sandbox
        .command(std::path::Path::new("/usr/bin/python3"))
        .unwrap();
    let script = r#"import sys,socket,os
assert open(sys.argv[1]).read()=='synthetic permitted input'
for path,mode in [(sys.argv[2],'r'),(sys.argv[1],'w')]:
 try:
  open(path,mode);raise AssertionError('forbidden file access')
 except OSError:pass
try:
 socket.create_connection(('127.0.0.1',int(sys.argv[3])),timeout=.2);raise AssertionError('host network accessible')
except OSError:pass
assert 'OVERTALK_TEST_SECRET' not in os.environ
try:
 assert os.pread(int(sys.argv[4]),100,0) != b'synthetic secret'
except OSError:pass
import subprocess,ctypes
assert subprocess.run(['/usr/bin/unshare','-Ur','true'],stderr=subprocess.DEVNULL).returncode != 0
libc=ctypes.CDLL(None,use_errno=True)
assert libc.ptrace(0,0,0,0) == -1
libc.syscall.restype=ctypes.c_long
assert libc.syscall(425,1,None) == -1 and ctypes.get_errno() == 1, 'io_uring_setup allowed'
print('isolated')"#;
    // Deliberately hand the launcher an inheritable secret fd. Its subprocess
    // must close it rather than bypassing the filesystem policy through /proc.
    // SAFETY: the closure runs in the forked child before exec and makes only one
    // async-signal-safe fcntl call on `descriptor`, which `secret_file` keeps open
    // in the parent across the spawn, so the child inherits it.
    unsafe {
        command.pre_exec(move || {
            if libc::fcntl(descriptor, libc::F_SETFD, 0) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let result = command
        .args(["-c", script])
        .arg(input.path())
        .arg(&denied)
        .arg(port.to_string())
        .arg(descriptor.to_string())
        .env("OVERTALK_TEST_SECRET", "synthetic secret")
        .stdout(Stdio::piped())
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        fs::read_to_string(&allowed).unwrap(),
        "synthetic permitted input"
    );
    assert_eq!(String::from_utf8_lossy(&result.stdout).trim(), "isolated");
}

#[test]
fn native_worker_denies_sibling_files_secrets_and_host_network() {
    let directory = tempfile::tempdir().unwrap();
    let allowed = directory.path().join("allowed.txt");
    let denied = directory.path().join("secret.txt");
    let output = directory.path().join("output.txt");
    fs::write(&allowed, "permitted").unwrap();
    fs::write(&denied, "private").unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let mut sandbox = Sandbox::new(false).unwrap();
    let input = sandbox.input(&allowed).unwrap();
    sandbox.output(&output).unwrap();
    let executable = std::path::Path::new(env!("CARGO_BIN_EXE_overtalk-worker"));
    let result = sandbox
        .command(executable)
        .unwrap()
        .arg("--isolation-probe")
        .arg(input.path())
        .arg(&denied)
        .arg(&output)
        .arg(listener.local_addr().unwrap().to_string())
        .env("OVERTALK_TEST_SECRET", "synthetic")
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}\nstdout: {}\nstderr: {}",
        result.status,
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    let result: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(
        result,
        serde_json::json!({"read_allowed":true,"write_input":false,"read_denied":false,"write_output":true,"host_network":false,"inherited_secret":false})
    );
    assert_eq!(fs::read_to_string(allowed).unwrap(), "permitted");
    assert_eq!(fs::read_to_string(output).unwrap(), "permitted output");
}
