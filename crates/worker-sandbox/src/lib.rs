//! OS-enforced worker isolation. The parent grants exact inputs, model/runtime
//! directories and outputs. No credentials, app database or ambient network.
#![deny(clippy::undocumented_unsafe_blocks)]
use serde::{Deserialize, Serialize};
use std::{
    ffi::OsStr,
    fs, io,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Debug, Serialize, Deserialize)]
pub struct Policy {
    pub read: Vec<PathBuf>,
    pub write: Vec<PathBuf>,
    pub root: PathBuf,
    pub cwd: PathBuf,
    pub gpu: bool,
    pub wrapper: Option<PathBuf>,
}
pub struct Sandbox {
    lock: Option<fs::File>,
    root: tempfile::TempDir,
    policy: Policy,
}
pub struct Input {
    path: PathBuf,
}
impl Input {
    pub fn path(&self) -> &Path {
        &self.path
    }
}
impl Drop for Input {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}
impl Sandbox {
    pub fn new(gpu: bool) -> io::Result<Self> {
        cleanup_stale()?;
        let root = tempfile::Builder::new()
            .prefix("overtalk-worker-")
            .tempdir()?;
        let lock = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(root.path().join(".owner-lock"))?;
        fs2::FileExt::lock_exclusive(&lock)?;
        for name in ["inputs", "work", "home", "cache"] {
            fs::create_dir(root.path().join(name))?;
        }
        let policy = Policy {
            read: vec![root.path().join("inputs")],
            write: vec![
                root.path().join("work"),
                root.path().join("home"),
                root.path().join("cache"),
            ],
            root: root.path().to_owned(),
            cwd: root.path().join("work"),
            gpu,
            wrapper: None,
        };
        Ok(Self {
            lock: Some(lock),
            root,
            policy,
        })
    }
    pub fn read(&mut self, path: impl AsRef<Path>) -> io::Result<()> {
        let path = path.as_ref();
        if !path.is_absolute() || path.parent().is_none() {
            return Err(io::Error::other("Sandbox grants require an absolute path"));
        }
        let path = lexical(path)?;
        if path == Path::new("/")
            || path.join("meeting_minutes.sqlite").exists()
            || path.join("integrations.json").exists()
            || covers(&path, private_folders())
        {
            return Err(io::Error::other(
                "Refusing a broad grant over application data",
            ));
        }
        fs::metadata(&path)?;
        if !self.policy.read.contains(&path) {
            self.policy.read.push(path);
        }
        Ok(())
    }
    pub fn allows_read(&self, path: &Path) -> bool {
        self.policy
            .read
            .iter()
            .any(|root| path == root || path.starts_with(root))
    }
    pub fn cwd(&mut self, path: impl AsRef<Path>) -> io::Result<()> {
        self.read(&path)?;
        self.policy.cwd = lexical(path.as_ref())?;
        Ok(())
    }
    pub fn input(&self, path: &Path) -> io::Result<Input> {
        let source = path.canonicalize()?;
        if !source.is_file() {
            return Err(io::Error::other("Worker input must be a regular file"));
        }
        let suffix = source
            .extension()
            .and_then(OsStr::to_str)
            .filter(|s| s.len() < 16 && s.bytes().all(|b| b.is_ascii_alphanumeric()))
            .unwrap_or("data");
        let file = tempfile::Builder::new()
            .suffix(&format!(".{suffix}"))
            .tempfile_in(self.root.path().join("inputs"))?;
        let (_, destination) = file.keep().map_err(|e| e.error)?;
        #[cfg(unix)]
        {
            fs::remove_file(&destination)?;
            if fs::hard_link(&source, &destination).is_err() {
                fs::copy(&source, &destination)?;
            }
        }
        #[cfg(not(unix))]
        fs::copy(&source, &destination)?;
        Ok(Input { path: destination })
    }
    /// Grant writes to this exact output, never the recording's parent folder.
    pub fn output(&mut self, path: &Path) -> io::Result<()> {
        let path = lexical(path)?;
        open_regular(&path, true, true)?;
        self.policy.write.push(path);
        Ok(())
    }
    pub fn command(&mut self, program: &Path) -> io::Result<Command> {
        self.read(program)?;
        if let Some(parent) = program.parent() {
            // A virtualenv needs its pyvenv.cfg, stdlib links and site-packages.
            if let Some(prefix) = parent.parent().filter(|p| p.join("pyvenv.cfg").is_file()) {
                self.read(prefix)?;
            }
        }
        let canonical = program.canonicalize()?;
        // Windows loads a program's DLLs from its own folder first (ONNX Runtime
        // and the Visual C++ runtime ship there). An AppContainer may read only
        // what it is granted, so grant those exact files, not the folder.
        #[cfg(windows)]
        for library in sibling_libraries(&canonical) {
            self.read(library)?;
        }
        if let Some(prefix) = canonical
            .parent()
            .and_then(Path::parent)
            .filter(|p| p.join("lib").is_dir() && p.parent().is_some())
        {
            if prefix != Path::new("/") {
                self.read(prefix)?;
            }
        }
        if let Some(paths) = std::env::var_os("LD_LIBRARY_PATH") {
            for path in std::env::split_paths(&paths) {
                if path.is_absolute() && path.is_dir() {
                    self.read(&path)?;
                    // Development/package library directories may contain links
                    // into a versioned runtime. Grant those exact library files,
                    // not the other contents of their destination directories.
                    for entry in fs::read_dir(&path)?.flatten() {
                        if let Ok(target) = fs::read_link(entry.path()) {
                            let target = if target.is_absolute() {
                                target
                            } else {
                                path.join(target)
                            };
                            if target.is_file() {
                                self.read(target)?;
                            }
                        }
                    }
                }
            }
        }
        let policy_path = self.root.path().join("policy.json");
        fs::write(&policy_path, serde_json::to_vec(&self.policy)?)?;
        let helper = worker_executable()?;
        let mut command = Command::new(helper);
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // The launcher is a console program and the engine has no console:
            // without this, Windows opens a console window for every worker.
            command.creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW);
        }
        command.arg(&policy_path).arg(program);
        Ok(command)
    }
    /// Run `program` under a launcher, such as a GPU-sharing wrapper, that receives the
    /// worker command as its arguments.
    pub fn command_with_wrapper(&mut self, program: &Path, wrapper: &Path) -> io::Result<Command> {
        self.read(wrapper)?;
        self.policy.wrapper = Some(wrapper.to_owned());
        self.command(program)
    }
}

/// The home folder and the per-user data and configuration folders. They hold
/// the app library (in its data folder), settings and other programs' credentials.
fn private_folders() -> impl Iterator<Item = PathBuf> {
    [
        dirs::home_dir(),
        dirs::data_dir(),
        dirs::data_local_dir(),
        dirs::config_dir(),
    ]
    .into_iter()
    .flatten()
}

/// Whether `path` is one of `folders` or contains one, after resolving links:
/// granting `~/.local` would expose the library in `~/.local/share`.
fn covers(path: &Path, folders: impl IntoIterator<Item = PathBuf>) -> bool {
    let Ok(path) = path.canonicalize() else {
        return false;
    };
    folders
        .into_iter()
        .filter_map(|folder| folder.canonicalize().ok())
        .any(|folder| folder.starts_with(&path))
}

/// The launcher GPU workers run under, named by `OVERTALK_GPU_LAUNCHER`. It must be an
/// absolute path to an existing file; otherwise workers start directly.
pub fn gpu_launcher() -> Option<PathBuf> {
    let path = PathBuf::from(std::env::var_os("OVERTALK_GPU_LAUNCHER")?);
    (path.is_absolute() && path.is_file()).then_some(path)
}

/// The DLLs in a program's own folder, which Windows searches first.
#[cfg_attr(not(any(windows, test)), allow(dead_code))]
fn sibling_libraries(program: &Path) -> Vec<PathBuf> {
    let Some(folder) = program.parent() else {
        return Vec::new();
    };
    let Ok(entries) = fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut libraries: Vec<_> = entries
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .and_then(OsStr::to_str)
                .is_some_and(|extension| extension.eq_ignore_ascii_case("dll"))
        })
        .collect();
    libraries.sort();
    libraries
}

/// Each launcher owns a process group, so cancellation also stops descendants
/// on platforms without a Windows Job Object or a Linux PID namespace.
pub fn terminate(child: &mut std::process::Child) {
    kill_process_group(child.id());
    let _ = child.kill();
}
pub fn kill_process_group(pid: u32) {
    #[cfg(unix)]
    if let Ok(pid) = i32::try_from(pid) {
        if pid > 1 {
            // SAFETY: kill takes no pointers. With pid > 1, -pid names only that
            // process group, never every process (-1) or our own group (0).
            unsafe {
                libc::kill(-pid, libc::SIGKILL);
            }
        }
    }
    #[cfg(not(unix))]
    let _ = pid;
}
impl Drop for Sandbox {
    fn drop(&mut self) {
        self.lock.take();
    }
}
/// A crashed app must not retain staged audio indefinitely. Locks keep live
/// workers safe across multiple app/service processes; names alone are not proof.
fn cleanup_stale() -> io::Result<()> {
    // Playback/metadata can launch several workers in quick succession. One
    // stale-directory sweep per minute is enough; never serialize every launch
    // behind a scan of the user's entire temporary directory.
    static LAST_SWEEP: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);
    {
        let mut last = LAST_SWEEP.lock().unwrap_or_else(|e| e.into_inner());
        if last.is_some_and(|at| at.elapsed() < std::time::Duration::from_secs(60)) {
            return Ok(());
        }
        *last = Some(std::time::Instant::now());
    }
    cleanup_stale_in(&std::env::temp_dir())
}

fn cleanup_stale_in(directory: &Path) -> io::Result<()> {
    for entry in fs::read_dir(directory)?.flatten() {
        if !entry
            .file_name()
            .to_string_lossy()
            .starts_with("overtalk-worker-")
            || !entry.file_type().is_ok_and(|kind| kind.is_dir())
        {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            // SAFETY: geteuid has no preconditions and cannot fail.
            if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o077 != 0 {
                continue;
            }
        }
        let path = entry.path();
        if metadata
            .modified()
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_none_or(|age| age < std::time::Duration::from_secs(30))
        {
            continue;
        }
        let Ok(lock) = open_regular(&path.join(".owner-lock"), true, false) else {
            continue;
        };
        if fs2::FileExt::try_lock_exclusive(&lock).is_ok() {
            // Keep the lock open while unlinking on Unix. Windows cannot unlink
            // the handle itself, so release it immediately before private cleanup.
            #[cfg(windows)]
            drop(lock);
            let _ = fs::remove_dir_all(path);
        }
    }
    Ok(())
}

/// Opening a FIFO for write can block before metadata is checked. Do not follow
/// a substituted final symlink or reparse point; only regular files are usable.
/// Open a regular file without following a final link. Writes never truncate;
/// `create` only creates a missing file and does not relax type checks.
pub fn open_regular(path: &Path, write: bool, create: bool) -> io::Result<fs::File> {
    let mut options = fs::OpenOptions::new();
    options
        .read(!write)
        .write(write)
        .create(create)
        .truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(io::Error::other(
            "Expected a regular file, not a link or special file",
        ));
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes()
            & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT
            != 0
        {
            return Err(io::Error::other("Files cannot be reparse points"));
        }
    }
    Ok(file)
}
fn lexical(path: &Path) -> io::Result<PathBuf> {
    if !path.is_absolute() {
        return Err(io::Error::other("An absolute worker path is required"));
    }
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            part => out.push(part.as_os_str()),
        }
    }
    Ok(out)
}
fn worker_executable() -> io::Result<PathBuf> {
    let exe = std::env::current_exe()?;
    let name = if cfg!(windows) {
        "overtalk-worker.exe"
    } else {
        "overtalk-worker"
    };
    let candidates = [
        exe.parent().unwrap().join(name),
        exe.parent().unwrap().join("..").join(name),
    ];
    candidates
        .into_iter()
        .find(|p| p.is_file())
        .ok_or_else(|| io::Error::other("Worker sandbox helper is missing; reinstall Overtalk"))
}

pub fn run() -> io::Result<i32> {
    let mut args = std::env::args_os().skip(1);
    let file = args
        .next()
        .ok_or_else(|| io::Error::other("Missing worker policy"))?;
    let program = PathBuf::from(
        args.next()
            .ok_or_else(|| io::Error::other("Missing worker executable"))?,
    );
    use std::io::Read;
    let mut bytes = Vec::new();
    open_regular(Path::new(&file), false, false)?
        .take(128 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 128 * 1024 {
        return Err(io::Error::other("Worker policy is too large"));
    }
    let policy: Policy = serde_json::from_slice(&bytes)?;
    #[cfg(unix)]
    close_inherited_descriptors()?;
    #[cfg(target_os = "linux")]
    let mut command = linux(&policy, &program)?;
    #[cfg(target_os = "macos")]
    let mut command = macos(&policy, &program)?;
    #[cfg(windows)]
    return windows::run(&policy, &program, args.collect());
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    return Err(io::Error::other(
        "Worker sandbox is unavailable on this operating system",
    ));
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.args(args);
        Err(command.exec())
    }
}

#[cfg(unix)]
fn close_inherited_descriptors() -> io::Result<()> {
    // A file or socket inherited from the service is a capability too. Close
    // everything except explicit stdio across exec; the Linux filter fd is
    // created afterwards and bwrap consumes it before launching the workload.
    let directory = if cfg!(target_os = "linux") {
        "/proc/self/fd"
    } else {
        "/dev/fd"
    };
    for entry in fs::read_dir(directory)?.flatten() {
        let Some(fd) = entry
            .file_name()
            .to_str()
            .and_then(|n| n.parse::<i32>().ok())
            .filter(|fd| *fd >= 3)
        else {
            continue;
        };
        // SAFETY: fcntl with F_GETFD takes no pointers; a number that is no longer
        // open only returns EBADF.
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
        // SAFETY: F_SETFD takes an integer flag set and only marks the descriptor
        // close-on-exec in this launcher, which is about to exec.
        if flags >= 0 && unsafe { libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC) } < 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

fn safe_environment(command: &mut Command, policy: &Policy) {
    command.env_clear();
    for (name, value) in std::env::vars_os() {
        let mut key = name.to_string_lossy().into_owned();
        // Windows names ignore case and arrive as `SystemRoot` and `windir`.
        if cfg!(windows) {
            key.make_ascii_uppercase();
        }
        // Windows points LOCALAPPDATA at the AppContainer's own folder and
        // refuses to start an AppContainer process without it.
        if matches!(
            key.as_str(),
            "LD_LIBRARY_PATH"
                | "DYLD_LIBRARY_PATH"
                | "SYSTEMROOT"
                | "WINDIR"
                | "LOCALAPPDATA"
                | "CUDA_VISIBLE_DEVICES"
                | "CUDA_MPS_PIPE_DIRECTORY"
                | "CUDA_MPS_LOG_DIRECTORY"
                | "CUDA_MPS_ACTIVE_THREAD_PERCENTAGE"
                | "SONEFOLD_CPU_THREADS"
                | "SONEFOLD_SPEECH_DEVICE"
                | "SONEFOLD_DIARIZATION_PYTHON"
                | "SONEFOLD_FFMPEG_PATH"
                | "SONEFOLD_PARAKEET_V3_MODEL"
                | "HF_HUB_CACHE"
                | "TORCH_FORCE_WEIGHTS_ONLY_LOAD"
                | "LLAMA_IDLE_TIMEOUT"
                | "TRAIN_CPU_NICE"
                | "TRAIN_GPU_PERCENT"
        ) || (key.starts_with("SONEFOLD_")
            && (key.ends_with("_MODEL_DIR")
                || key.ends_with("_MODEL_PATH")
                || key.ends_with("_ASR_1_7B_DIR")
                || key.ends_with("_ASR_0_6B_DIR")
                || key.ends_with("_ALIGNER_DIR")))
        {
            command.env(name, value);
        }
    }
    command
        .env("PATH", "/usr/local/bin:/usr/bin:/bin")
        .env("LANG", "C.UTF-8")
        .env("HOME", policy.root.join("home"))
        .env("TMPDIR", policy.root.join("work"))
        .env("TEMP", policy.root.join("work"))
        .env("TMP", policy.root.join("work"))
        .env("XDG_CACHE_HOME", policy.root.join("cache"))
        .env("HF_HUB_OFFLINE", "1")
        .env("TRANSFORMERS_OFFLINE", "1")
        .env("HF_HUB_DISABLE_TELEMETRY", "1")
        .env("TRITON_CACHE_DIR", policy.root.join("cache/triton"))
        .env("TORCHINDUCTOR_CACHE_DIR", policy.root.join("cache/torch"))
        .env("CUDA_CACHE_PATH", policy.root.join("cache/cuda"))
        .env("TOKENIZERS_PARALLELISM", "false")
        .env("PYTHONDONTWRITEBYTECODE", "1");
}
#[cfg(target_os = "linux")]
fn linux(policy: &Policy, program: &Path) -> io::Result<Command> {
    let bwrap = Path::new("/usr/bin/bwrap");
    if !bwrap.is_file() {
        return Err(io::Error::other(
            "Install bubblewrap to run isolated workers",
        ));
    }
    let mut command = Command::new(bwrap);
    safe_environment(&mut command, policy);
    command.args([
        "--unshare-all",
        "--unshare-user",
        "--disable-userns",
        "--die-with-parent",
        "--new-session",
        "--cap-drop",
        "ALL",
        "--proc",
        "/proc",
        "--dev",
        "/dev",
        "--tmpfs",
        "/tmp",
    ]);
    command
        .arg("--seccomp")
        .arg(seccomp_fd(&policy.root)?.to_string());
    command.args(["--dir", "/etc/alternatives"]);
    for path in [
        "/usr",
        "/bin",
        "/sbin",
        "/lib",
        "/lib64",
        "/etc/ld.so.cache",
        "/etc/ld.so.conf",
        "/etc/ld.so.conf.d",
        "/etc/alternatives/libblas.so.3-x86_64-linux-gnu",
        "/etc/alternatives/liblapack.so.3-x86_64-linux-gnu",
        "/etc/alternatives/cc",
        "/etc/alternatives/gcc",
        "/etc/alternatives/g++",
        "/etc/alternatives/c++",
        "/etc/alternatives/cpp",
        "/sys/devices",
        "/sys/bus/pci",
        "/sys/class/drm",
        "/sys/module/nvidia",
        "/sys/module/nvidia_uvm",
        "/sys/module/nvidia_modeset",
        "/sys/module/nvidia_drm",
        "/sys/module/amdgpu",
        "/sys/class/kfd",
    ] {
        if Path::new(path).exists() {
            if path.starts_with("/etc/alternatives/")
                && fs::symlink_metadata(path)?.file_type().is_symlink()
            {
                // Preserve compiler aliases: replacing the symlink with a bind
                // mounted executable gives GCC the wrong installation prefix.
                command.arg("--symlink").arg(fs::read_link(path)?).arg(path);
            } else {
                command.args(["--ro-bind", path, path]);
            }
        }
    }
    // Runtime directories must exist before binding executable/library symlinks
    // that resolve into them (uv Python installations are a common example).
    let mut reads: Vec<_> = policy.read.iter().collect();
    reads.sort_by_key(|path| (!path.is_dir(), path.components().count()));
    for path in reads {
        if path.is_file()
            && policy
                .read
                .iter()
                .any(|directory| directory.is_dir() && path.starts_with(directory))
        {
            continue;
        }
        command.arg("--ro-bind").arg(path).arg(path);
    }
    for path in &policy.write {
        command.arg("--bind").arg(path).arg(path);
    }
    // Expose only GPU devices and this launcher's private MPS socket directory.
    if policy.gpu {
        for entry in fs::read_dir("/dev")?.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with("nvidia") || name == "dri" || name == "kfd" {
                let path = entry.path();
                command.arg("--dev-bind").arg(&path).arg(&path);
            }
        }
    }
    command.arg("--chdir").arg(&policy.cwd).arg("--");
    if let Some(wrapper) = &policy.wrapper {
        command.arg(wrapper);
    }
    command.arg(program);
    Ok(command)
}

#[cfg(target_os = "linux")]
fn seccomp_fd(root: &Path) -> io::Result<i32> {
    use std::{io::Write, os::fd::IntoRawFd};
    #[cfg(target_arch = "x86_64")]
    const ARCH: u32 = 0xc000003e;
    #[cfg(target_arch = "aarch64")]
    const ARCH: u32 = 0xc00000b7;
    // BPF checks the syscall ABI before its number; this also rejects x32/compat
    // entry points rather than allowing a second ABI to bypass the deny list.
    let mut rules = vec![
        (0x20u16, 0, 0, 4),
        (0x15, 1, 0, ARCH),
        (0x06, 0, 0, 0x80000000),
        (0x20, 0, 0, 0),
    ];
    rules.push((0x35, 0, 1, 0x40000000));
    rules.push((0x06, 0, 0, 0x80000000));
    for syscall in [
        libc::SYS_ptrace,
        libc::SYS_process_vm_readv,
        libc::SYS_process_vm_writev,
        libc::SYS_bpf,
        libc::SYS_perf_event_open,
        libc::SYS_keyctl,
        libc::SYS_add_key,
        libc::SYS_request_key,
        libc::SYS_userfaultfd,
        libc::SYS_mount,
        libc::SYS_umount2,
        libc::SYS_pivot_root,
        libc::SYS_open_by_handle_at,
        libc::SYS_fanotify_init,
        libc::SYS_unshare,
        libc::SYS_setns,
        libc::SYS_reboot,
        libc::SYS_kexec_load,
        libc::SYS_init_module,
        libc::SYS_finit_module,
        libc::SYS_delete_module,
        // io_uring is a large kernel surface that no worker needs.
        libc::SYS_io_uring_setup,
        libc::SYS_io_uring_enter,
        libc::SYS_io_uring_register,
    ] {
        rules.push((0x15, 0, 1, syscall as u32));
        rules.push((0x06, 0, 0, 0x00050000 | libc::EPERM as u32));
    }
    // glibc falls back to clone when clone3 reports ENOSYS.
    rules.push((0x15, 0, 1, libc::SYS_clone3 as u32));
    rules.push((0x06, 0, 0, 0x00050000 | libc::ENOSYS as u32));
    rules.push((0x06, 0, 0, 0x7fff0000));
    let mut file = tempfile::tempfile_in(root)?;
    for (code, jt, jf, k) in rules {
        file.write_all(&code.to_ne_bytes())?;
        file.write_all(&[jt, jf])?;
        file.write_all(&k.to_ne_bytes())?;
    }
    use std::io::{Seek, SeekFrom};
    file.seek(SeekFrom::Start(0))?;
    let fd = file.into_raw_fd();
    // The dedicated launcher execs bwrap immediately; this sole extra descriptor
    // contains only the filter. bwrap consumes/closes it before the workload.
    // SAFETY: `fd` is the descriptor this function just took from `file`; F_SETFD
    // takes an integer flag set and no pointers.
    if unsafe { libc::fcntl(fd, libc::F_SETFD, 0) } == -1 {
        return Err(io::Error::last_os_error());
    }
    let limit = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: `limit` is a valid rlimit that outlives the call, which only reads it.
    if unsafe { libc::setrlimit(libc::RLIMIT_CORE, &limit) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(fd)
}
#[cfg(target_os = "macos")]
fn macos(policy: &Policy, program: &Path) -> io::Result<Command> {
    // Seatbelt matches resolved paths, so a grant in /var/folders must name
    // /private/var/folders. A path that does not exist keeps its spelling.
    fn quote(path: &Path) -> io::Result<String> {
        let path = path.canonicalize().unwrap_or_else(|_| path.to_owned());
        let text = path
            .to_str()
            .ok_or_else(|| io::Error::other("Sandbox paths must be UTF-8"))?;
        Ok(format!(
            "\"{}\"",
            text.replace('\\', "\\\\").replace('"', "\\\"")
        ))
    }
    // The loader reads the root directory itself while it finds the system
    // library cache; without `(literal "/")` every program aborts before main.
    let mut profile = String::from(
        "(version 1)(deny default)(allow process-fork)(allow process-exec)(allow sysctl-read)\
         (allow file-read-metadata)(allow mach-lookup (global-name \"com.apple.system.logger\"))\
         (allow file-read* (literal \"/\") (subpath \"/System\") (subpath \"/usr\") \
         (subpath \"/Library/Apple\") (subpath \"/opt/homebrew\") \
         (literal \"/dev/null\") (literal \"/dev/urandom\"))\
         (allow file-write* (literal \"/dev/null\"))",
    );
    for path in &policy.read {
        profile.push_str(&format!("(allow file-read* (subpath {}))", quote(path)?));
    }
    for path in &policy.write {
        profile.push_str(&format!(
            "(allow file-read* file-write* (subpath {}))",
            quote(path)?
        ));
    }
    if policy.gpu {
        profile.push_str(
            "(allow iokit-open)(allow mach-lookup (global-name \"com.apple.MTLCompilerService\") \
             (global-name \"com.apple.cvmsServ\"))",
        );
    }
    let mut command = Command::new("/usr/bin/sandbox-exec");
    safe_environment(&mut command, policy);
    command
        .current_dir(&policy.cwd)
        .arg("-p")
        .arg(profile)
        .arg(program);
    Ok(command)
}
#[cfg(windows)]
mod windows;

/// Make the current user the owner and only grantee of `path`, replacing
/// inherited entries; a folder passes the grant on to what it will contain.
#[cfg(windows)]
pub fn restrict_to_current_user(path: &Path, directory: bool) -> io::Result<()> {
    windows::restrict_to_current_user(path, directory)
}

/// Collect short worker responses without unbounded output or a stuck child.
/// Callers must not use this for a long inference or PCM stream.
pub fn run_bounded(
    mut command: Command,
    limit: usize,
    timeout: std::time::Duration,
) -> io::Result<std::process::Output> {
    use std::{
        io::Read,
        process::Stdio,
        sync::{
            atomic::{AtomicBool, Ordering},
            mpsc, Arc,
        },
        time::{Duration, Instant},
    };
    // Also reap on setup/read/wait errors; Child itself has no Drop cleanup.
    struct Reap(std::process::Child);
    impl Drop for Reap {
        fn drop(&mut self) {
            terminate(&mut self.0);
            let _ = self.0.wait();
        }
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = Reap(command.spawn()?);
    let stdout = child
        .0
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("No worker stdout"))?;
    let stderr = child
        .0
        .stderr
        .take()
        .ok_or_else(|| io::Error::other("No worker stderr"))?;
    #[cfg(unix)]
    let descriptors = {
        use std::os::fd::AsRawFd;
        let descriptors = [stdout.as_raw_fd(), stderr.as_raw_fd()];
        for fd in descriptors {
            // SAFETY: `fd` belongs to `stdout` or `stderr`, which stay open in this
            // scope; F_GETFL takes no pointers.
            let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
            // SAFETY: as above; F_SETFL takes an integer flag set.
            if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
            {
                return Err(io::Error::last_os_error());
            }
        }
        descriptors
    };
    #[cfg(windows)]
    let handles = {
        use std::os::windows::io::AsRawHandle;
        // Pass the integer value across threads; ownership remains with each pipe.
        [
            stdout.as_raw_handle() as usize,
            stderr.as_raw_handle() as usize,
        ]
    };
    let cancelled = Arc::new(AtomicBool::new(false));
    let (send, receive) = mpsc::sync_channel(2);
    let readers: Vec<_> = [Box::new(stdout) as Box<dyn Read + Send>, Box::new(stderr)]
        .into_iter()
        .enumerate()
        .map(|(index, mut pipe)| {
            #[cfg(unix)]
            let descriptor = descriptors[index];
            #[cfg(windows)]
            let handle = handles[index];
            let send = send.clone();
            let cancelled = cancelled.clone();
            std::thread::spawn(move || {
                let result = (|| {
                    let mut bytes = Vec::with_capacity(limit.min(8192));
                    let mut scratch = [0; 8192];
                    while !cancelled.load(Ordering::Relaxed) {
                        let read_size = scratch.len();
                        #[cfg(windows)]
                        let read_size = {
                            use windows_sys::Win32::{
                                Foundation::ERROR_BROKEN_PIPE, System::Pipes::PeekNamedPipe,
                            };
                            let mut available = 0;
                            // A blocking ReadFile could keep this thread alive after
                            // the deadline if a descendant inherited the write end.
                            // SAFETY: `handle` belongs to `pipe`, which this thread owns
                            // and keeps open for the whole loop. No data is read (null
                            // buffer, size 0); the only out-pointer is `available`.
                            let ok = unsafe {
                                PeekNamedPipe(
                                    handle as _,
                                    std::ptr::null_mut(),
                                    0,
                                    std::ptr::null_mut(),
                                    &mut available,
                                    std::ptr::null_mut(),
                                )
                            };
                            if ok == 0 {
                                let error = io::Error::last_os_error();
                                if error.raw_os_error() == Some(ERROR_BROKEN_PIPE as i32) {
                                    return Ok(bytes);
                                }
                                return Err(error);
                            }
                            if available == 0 {
                                std::thread::sleep(Duration::from_millis(5));
                                continue;
                            }
                            read_size.min(available as usize)
                        };
                        match pipe.read(&mut scratch[..read_size]) {
                            Ok(0) => return Ok(bytes),
                            Ok(n) => {
                                if bytes.len().saturating_add(n) > limit {
                                    return Err(io::Error::other(
                                        "Worker response exceeds its size limit",
                                    ));
                                }
                                bytes.extend_from_slice(&scratch[..n]);
                            }
                            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                                #[cfg(unix)]
                                {
                                    let mut event = libc::pollfd {
                                        fd: descriptor,
                                        events: libc::POLLIN,
                                        revents: 0,
                                    };
                                    // Wake immediately for output, while retaining a
                                    // bounded cancellation latency and low idle CPU.
                                    // SAFETY: `event` is one valid pollfd for the call, and
                                    // its descriptor belongs to `pipe`, which this thread
                                    // owns and keeps open.
                                    unsafe {
                                        libc::poll(&mut event, 1, 5);
                                    }
                                }
                                #[cfg(not(unix))]
                                std::thread::sleep(Duration::from_millis(5));
                            }
                            Err(e) => return Err(e),
                        }
                    }
                    Err(io::Error::new(
                        io::ErrorKind::Interrupted,
                        "Worker read cancelled",
                    ))
                })();
                let _ = send.send((index, result));
            })
        })
        .collect();
    drop(send);
    let started = Instant::now();
    let mut status = None;
    let mut output = [None, None];
    let result = loop {
        if status.is_none() {
            match child.0.try_wait() {
                Ok(Some(exited)) => {
                    status = Some(exited);
                    // A child can exit while a grandchild retains the pipe.
                    kill_process_group(child.0.id());
                }
                Ok(None) => {}
                Err(error) => break Err(error),
            }
        }
        if let Some(status) = status.filter(|_| output.iter().all(Option::is_some)) {
            break Ok(std::process::Output {
                status,
                stdout: output[0].take().unwrap(),
                stderr: output[1].take().unwrap(),
            });
        }
        let remaining = timeout.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            break Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Worker exceeded its response deadline",
            ));
        }
        match receive.recv_timeout(remaining.min(Duration::from_millis(5))) {
            Ok((index, Ok(bytes))) => output[index] = Some(bytes),
            Ok((_, Err(error))) => break Err(error),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) if output.iter().all(Option::is_some) => {
                std::thread::sleep(remaining.min(Duration::from_millis(5)));
            }
            Err(_) => break Err(io::Error::other("Worker readers stopped unexpectedly")),
        }
    };
    cancelled.store(true, Ordering::Relaxed);
    drop(child);
    for reader in readers {
        let _ = reader.join();
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sibling_libraries_are_the_dlls_beside_a_program() {
        let folder = tempfile::tempdir().unwrap();
        let program = folder.path().join("sonefold-service.exe");
        for name in [
            "sonefold-service.exe",
            "onnxruntime.dll",
            "VCRUNTIME140.DLL",
            "notes.txt",
        ] {
            fs::write(folder.path().join(name), "").unwrap();
        }
        fs::create_dir(folder.path().join("plugins.dll")).unwrap();
        assert_eq!(
            sibling_libraries(&program),
            vec![
                folder.path().join("VCRUNTIME140.DLL"),
                folder.path().join("onnxruntime.dll")
            ]
        );
        assert!(sibling_libraries(Path::new("program")).is_empty());
    }
}

#[cfg(all(test, unix))]
mod adversarial_tests {
    use super::*;
    use std::{
        os::unix::{
            fs::{symlink, PermissionsExt},
            process::CommandExt,
        },
        time::{Duration, SystemTime},
    };

    #[test]
    fn stale_cleanup_rejects_special_locks_and_preserves_live_workers() {
        let root = tempfile::tempdir().unwrap();
        let old = SystemTime::now() - Duration::from_secs(90);
        let directory = |name: &str| {
            let path = root.path().join(format!("overtalk-worker-{name}"));
            fs::create_dir(&path).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
            path
        };
        let stale = directory("stale");
        fs::write(stale.join(".owner-lock"), "").unwrap();
        let active = directory("active");
        let lock = fs::File::create(active.join(".owner-lock")).unwrap();
        fs2::FileExt::lock_exclusive(&lock).unwrap();
        let fifo = directory("fifo");
        let name = std::ffi::CString::new(fifo.join(".owner-lock").as_os_str().as_encoded_bytes())
            .unwrap();
        // SAFETY: `name` is a NUL-terminated C string that outlives the call; mkfifo only reads it.
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        let linked = directory("linked");
        symlink(active.join(".owner-lock"), linked.join(".owner-lock")).unwrap();
        let shared = directory("shared");
        fs::write(shared.join(".owner-lock"), "").unwrap();
        fs::set_permissions(&shared, fs::Permissions::from_mode(0o755)).unwrap();
        for path in [&stale, &active, &fifo, &linked, &shared] {
            fs::File::open(path)
                .unwrap()
                .set_times(fs::FileTimes::new().set_modified(old))
                .unwrap();
        }
        cleanup_stale_in(root.path()).unwrap();
        assert!(!stale.exists());
        for path in [&active, &fifo, &linked, &shared] {
            assert!(path.exists());
        }
    }

    #[test]
    fn read_grants_refuse_folders_that_contain_private_data() {
        // A program installed in ~/.local must not grant ~/.local, which holds
        // the library in ~/.local/share, directly or through a link.
        let root = tempfile::tempdir().unwrap();
        let prefix = root.path().join("prefix");
        let data = prefix.join("share");
        let models = data.join("models");
        fs::create_dir_all(&models).unwrap();
        let link = root.path().join("link");
        symlink(&prefix, &link).unwrap();
        for granted in [&prefix, &data, &link] {
            assert!(covers(granted, [data.clone()]), "{}", granted.display());
        }
        assert!(!covers(&models, [data.clone()]));

        let mut sandbox = Sandbox::new(false).unwrap();
        let home = dirs::home_dir().unwrap().canonicalize().unwrap();
        for refused in home.ancestors() {
            assert!(sandbox.read(refused).is_err(), "{}", refused.display());
        }
        sandbox.read(&models).unwrap();
        assert!(sandbox.allows_read(&models));
        assert!(!sandbox.allows_read(&home));
    }

    #[test]
    fn output_grants_reject_links_and_fifos_without_modifying_the_target() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("target");
        fs::write(&target, "unchanged").unwrap();
        let link = root.path().join("link");
        symlink(&target, &link).unwrap();
        assert!(open_regular(&link, true, true).is_err());
        let fifo = root.path().join("fifo");
        let name = std::ffi::CString::new(fifo.as_os_str().as_encoded_bytes()).unwrap();
        // SAFETY: `name` is a NUL-terminated C string that outlives the call; mkfifo only reads it.
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        assert!(open_regular(&fifo, true, false).is_err());
        assert!(open_regular(&fifo, false, false).is_err());
        assert_eq!(fs::read_to_string(target).unwrap(), "unchanged");
    }

    #[test]
    fn bounded_output_rejects_floods_and_enforces_silent_worker_deadlines() {
        let mut flood = Command::new("python3");
        flood.args(["-c", "import os,time\nos.write(1,b'x'*4096)\ntime.sleep(5)"]);
        let started = std::time::Instant::now();
        let error = run_bounded(flood, 16, Duration::from_secs(4)).unwrap_err();
        assert!(error.to_string().contains("size limit"));
        assert!(started.elapsed() < Duration::from_secs(2));
        let mut silent = Command::new("python3");
        silent.args(["-c", "import time;time.sleep(5)"]);
        let error = run_bounded(silent, 16, Duration::from_millis(50)).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    }

    #[test]
    fn bounded_response_closes_descendant_pipes_after_parent_exits() {
        let directory = tempfile::tempdir().unwrap();
        let pid_file = directory.path().join("pid");
        let watchdog_file = pid_file.clone();
        // Keep the regression finite even when run against the old collector.
        let watchdog = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(2));
            if let Ok(pid) = fs::read_to_string(watchdog_file) {
                kill_process_group(pid.parse().unwrap());
            }
        });
        let mut command = Command::new("python3");
        command.process_group(0).args([
            "-c",
            "import os,sys,time\nopen(sys.argv[1],'w').write(str(os.getpid()))\nif os.fork()==0: time.sleep(5)\n",
        ]).arg(pid_file);
        let started = std::time::Instant::now();
        let result = run_bounded(command, 1024, Duration::from_millis(500));
        let elapsed = started.elapsed();
        watchdog.join().unwrap();
        assert!(
            elapsed < Duration::from_millis(1500),
            "collector stalled for {elapsed:?}"
        );
        assert!(result.unwrap().status.success());
    }
}
