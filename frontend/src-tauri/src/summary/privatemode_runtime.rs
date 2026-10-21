//! Privatemode's official proxy, run by Overtalk on this computer so that no
//! container runtime is needed. The binary is Privatemode's own build, fetched
//! from Privatemode's registry and checked against pinned hashes; Overtalk never
//! ships it. The proxy verifies the remote deployment and encrypts every request
//! on this device before it leaves.
use sha2::{Digest, Sha256};
use std::{
    io::Read,
    path::{Path, PathBuf},
    process::Stdio,
    sync::OnceLock,
    time::Duration,
};
use tokio::{process::Child, sync::Mutex};

const VERSION: &str = "v1.59.0";
const REGISTRY: &str = "https://ghcr.io";
const REPOSITORY: &str = "edgelesssys/privatemode/privatemode-proxy";
const MAX_LAYER: usize = 64 * 1024 * 1024;

/// One layer of image index sha256:3cd4da0ee6ceee91ca84dde4e3e3f0e94e8b354d7c1d46987e4331a5c2f8bcff,
/// the same image scripts/run-privatemode-proxy.py runs. Update all three together.
struct Pin {
    layer: &'static str,
    path: &'static str,
    binary: &'static str,
}

fn pin() -> Option<Pin> {
    // The image carries Linux binaries only.
    if !cfg!(target_os = "linux") {
        return None;
    }
    match std::env::consts::ARCH {
        "x86_64" => Some(Pin {
            layer: "sha256:69495146b4946cb53bc192b89ce263c2e614c443dbd6cb08a0b3cabc31d62194",
            path: "nix/store/4wa51g6wgapjc6dfhf3gjk327p5k1k9g-privatemode-proxy-v1.59.0/bin/privatemode-proxy",
            binary: "0113ea23b7035fd3e3dd078cbb0d5ddc9471cfeac44a49c392802e55ee7f6487",
        }),
        "aarch64" => Some(Pin {
            layer: "sha256:ac00bcb01834be96aaaf095fe2e9654ba8815192c60b3f182f7c3fe5f6f62569",
            path: "nix/store/hf3qig4nqaw20j483yrqnd17im3n8ljh-privatemode-proxy-aarch64-unknown-linux-gnu-v1.59.0/bin/privatemode-proxy",
            binary: "8b939f5c47718408a012895ff170052ddab5e5ce150fe2988c733aac3d135ee9",
        }),
        _ => None,
    }
}

static DIRECTORY: OnceLock<PathBuf> = OnceLock::new();

/// Where the fetched proxy, its workspace and its key file live.
pub fn set_directory(directory: PathBuf) {
    let _ = DIRECTORY.set(directory);
}

struct Running {
    child: Child,
    url: url::Url,
    key: String,
    key_file: PathBuf,
}

static RUNNING: Mutex<Option<Running>> = Mutex::const_new(None);

fn hex(bytes: &[u8]) -> String {
    crate::security::storage::hex(&Sha256::digest(bytes))
}

/// The local proxy for `key`, starting Privatemode's proxy when it is not
/// already running with that key.
pub async fn endpoint(key: &str) -> Result<url::Url, String> {
    if !cfg!(target_os = "linux") {
        return Err(
            "On this system, start Privatemode's proxy app and enter its address in Settings."
                .into(),
        );
    }
    let pin = pin().ok_or("Privatemode's proxy is not available for this processor")?;
    let directory = DIRECTORY
        .get()
        .cloned()
        .ok_or("Private AI summaries are not ready yet")?;
    let key_hash = hex(key.as_bytes());
    let mut running = RUNNING.lock().await;
    if let Some(current) = running.as_mut() {
        if current.key == key_hash && matches!(current.child.try_wait(), Ok(None)) {
            return Ok(current.url.clone());
        }
    }
    if let Some(previous) = running.take() {
        stop(previous).await;
    }
    let binary = binary(&directory, &pin).await?;
    *running = Some(start(&directory, &binary, key, key_hash).await?);
    Ok(running.as_ref().expect("just started").url.clone())
}

/// Stops the proxy and removes its key file, for example when signing out.
pub async fn shutdown() {
    if let Some(previous) = RUNNING.lock().await.take() {
        stop(previous).await;
    }
}

async fn stop(mut running: Running) {
    let _ = running.child.kill().await;
    let _ = std::fs::remove_file(&running.key_file);
}

async fn binary(directory: &Path, pin: &Pin) -> Result<PathBuf, String> {
    let path = directory.join(format!("privatemode-proxy-{VERSION}"));
    if let Ok(bytes) = std::fs::read(&path) {
        if hex(&bytes) == pin.binary {
            return Ok(path);
        }
    }
    crate::security::privacy::guard_process_destination(REGISTRY)?;
    let client = reqwest::Client::builder()
        .no_proxy()
        .https_only(true)
        .redirect(reqwest::redirect::Policy::limited(3))
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|_| "Could not prepare the Privatemode download")?;
    let failed = |_| "Could not download Privatemode's proxy. Check the connection and try again.";
    let token: serde_json::Value = client
        .get(format!(
            "{REGISTRY}/token?scope=repository:{REPOSITORY}:pull"
        ))
        .send()
        .await
        .map_err(failed)?
        .error_for_status()
        .map_err(failed)?
        .json()
        .await
        .map_err(failed)?;
    let token = token["token"]
        .as_str()
        .ok_or("Privatemode's registry refused the download")?;
    let response = client
        .get(format!("{REGISTRY}/v2/{REPOSITORY}/blobs/{}", pin.layer))
        .bearer_auth(token)
        .send()
        .await
        .map_err(failed)?
        .error_for_status()
        .map_err(failed)?;
    if response
        .content_length()
        .is_some_and(|n| n as usize > MAX_LAYER)
    {
        return Err("Privatemode's proxy download is larger than expected".into());
    }
    let layer = response.bytes().await.map_err(failed)?;
    if layer.len() > MAX_LAYER || format!("sha256:{}", hex(&layer)) != pin.layer {
        return Err("Privatemode's proxy download did not match the expected version".into());
    }
    let wanted = pin.path.to_owned();
    let expected = pin.binary.to_owned();
    let path_out = path.clone();
    tokio::task::spawn_blocking(move || extract(&layer, &wanted, &expected, &path_out))
        .await
        .map_err(|_| "Could not unpack Privatemode's proxy".to_owned())??;
    Ok(path)
}

fn extract(layer: &[u8], wanted: &str, expected: &str, path: &Path) -> Result<(), String> {
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(layer));
    let unpack = |_| "Could not unpack Privatemode's proxy".to_owned();
    for entry in archive.entries().map_err(unpack)? {
        let mut entry = entry.map_err(unpack)?;
        let name = entry.path().map_err(unpack)?.to_string_lossy().into_owned();
        if name.trim_start_matches("./").trim_start_matches('/') != wanted
            || !entry.header().entry_type().is_file()
            || entry.size() as usize > MAX_LAYER
        {
            continue;
        }
        let mut bytes = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut bytes).map_err(unpack)?;
        if hex(&bytes) != expected {
            return Err("Privatemode's proxy did not match the expected version".into());
        }
        let parent = path.parent().ok_or("Invalid Privatemode directory")?;
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        let temporary = parent.join(format!(".privatemode-proxy-{}", uuid::Uuid::new_v4()));
        write_private(&temporary, &bytes, 0o700)?;
        return std::fs::rename(&temporary, path).map_err(|e| e.to_string());
    }
    Err("Privatemode's proxy was not found in the download".into())
}

fn write_private(path: &Path, bytes: &[u8], mode: u32) -> Result<(), String> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(mode).custom_flags(libc::O_NOFOLLOW);
    }
    #[cfg(not(unix))]
    let _ = mode;
    let mut file = options.open(path).map_err(|e| e.to_string())?;
    file.write_all(bytes).map_err(|e| e.to_string())
}

async fn start(
    directory: &Path,
    binary: &Path,
    key: &str,
    key_hash: String,
) -> Result<Running, String> {
    let workspace = directory.join("workspace");
    std::fs::create_dir_all(&workspace).map_err(|e| e.to_string())?;
    // Key files of a proxy that outlived an earlier engine are no longer needed.
    if let Ok(entries) = std::fs::read_dir(directory) {
        for entry in entries.flatten() {
            if entry.file_name().to_string_lossy().starts_with("key-") {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
    let key_file = directory.join(format!("key-{}", uuid::Uuid::new_v4()));
    write_private(&key_file, key.as_bytes(), 0o600)?;
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|listener| listener.local_addr())
        .map_err(|e| e.to_string())?
        .port();
    let mut command = tokio::process::Command::new(binary);
    command
        .arg("--listen-address=127.0.0.1")
        .arg(format!("--port={port}"))
        .arg(format!("--api-key=@{}", key_file.display()))
        .arg("--api-key-fallback=always")
        .arg(format!("--workspace={}", workspace.display()))
        .arg("--nvidia-ocsp-allow-unknown=false")
        .arg("--nvidia-ocsp-revoked-grace-period=0")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(target_os = "linux")]
    // SAFETY: the closure runs in the forked child before exec, so it may only make
    // async-signal-safe calls: one prctl syscall, and an io::Error built from errno
    // without allocating. It asks the kernel to stop the proxy when the engine
    // exits, so the key file is not left in use.
    unsafe {
        command.pre_exec(|| {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = command.spawn().map_err(|e| {
        let _ = std::fs::remove_file(&key_file);
        format!("Could not start Privatemode's proxy: {e}")
    })?;
    // Drain the proxy's log so it never blocks, keeping only its last lines to
    // explain a failed start. Nothing is written to Overtalk's own logs.
    let stderr = child.stderr.take();
    let log = tokio::spawn(async move {
        use tokio::io::AsyncBufReadExt;
        let mut last = std::collections::VecDeque::new();
        if let Some(stderr) = stderr {
            let mut lines = tokio::io::BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if last.len() == 8 {
                    last.pop_front();
                }
                last.push_back(line);
            }
        }
        last
    });
    // Startup attests the Privatemode deployment, which takes a few seconds.
    for _ in 0..200 {
        if let Ok(Some(_)) = child.try_wait() {
            let _ = std::fs::remove_file(&key_file);
            let last = tokio::time::timeout(Duration::from_secs(2), log)
                .await
                .ok()
                .and_then(Result::ok)
                .unwrap_or_default();
            return Err(if last.iter().any(|l| l.contains("Unauthorized")) {
                "Privatemode did not accept the summary key.".into()
            } else {
                "Privatemode's proxy stopped while starting. Check the connection and try again."
                    .into()
            });
        }
        if tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .is_ok()
        {
            // The port was free when chosen, but another local program could have
            // bound it since; only the proxy's own socket may receive transcripts.
            if !child.id().is_some_and(|pid| listens_on(pid, port)) {
                let _ = child.kill().await;
                let _ = std::fs::remove_file(&key_file);
                return Err(
                    "Another program took the port chosen for Privatemode's proxy. Try again."
                        .into(),
                );
            }
            let url = url::Url::parse(&format!("http://127.0.0.1:{port}/v1/"))
                .map_err(|_| "Invalid proxy address")?;
            return Ok(Running {
                child,
                url,
                key: key_hash,
                key_file,
            });
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
    let _ = child.kill().await;
    let _ = std::fs::remove_file(&key_file);
    Err("Privatemode's proxy did not start in time".into())
}

/// Whether process `pid` holds the socket listening on 127.0.0.1:`port`.
#[cfg(target_os = "linux")]
fn listens_on(pid: u32, port: u16) -> bool {
    let address = format!("0100007F:{port:04X}");
    let sockets: Vec<String> = std::fs::read_to_string("/proc/net/tcp")
        .unwrap_or_default()
        .lines()
        .skip(1)
        .filter_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            // State 0A is LISTEN; field 9 is the socket's inode.
            (fields.get(1) == Some(&address.as_str()) && fields.get(3) == Some(&"0A"))
                .then(|| fields.get(9).map(|inode| format!("socket:[{inode}]")))
                .flatten()
        })
        .collect();
    !sockets.is_empty()
        && std::fs::read_dir(format!("/proc/{pid}/fd")).is_ok_and(|entries| {
            entries.flatten().any(|entry| {
                std::fs::read_link(entry.path()).is_ok_and(|target| {
                    sockets
                        .iter()
                        .any(|socket| target.as_os_str() == socket.as_str())
                })
            })
        })
}

/// `pin` offers no proxy outside Linux, so `start` never runs there.
#[cfg(not(target_os = "linux"))]
fn listens_on(_pid: u32, _port: u16) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "linux")]
    #[test]
    fn only_the_process_holding_the_listener_owns_the_port() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        assert!(listens_on(std::process::id(), port));
        let mut other = std::process::Command::new("sleep")
            .arg("5")
            .spawn()
            .unwrap();
        assert!(
            !listens_on(other.id(), port),
            "another process does not own it"
        );
        other.kill().unwrap();
        other.wait().unwrap();
        drop(listener);
        assert!(
            !listens_on(std::process::id(), port),
            "nothing listens any more"
        );
    }

    fn layer(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(
            Vec::new(),
            flate2::Compression::fast(),
        ));
        for (name, data) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_size(data.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();
            builder.append_data(&mut header, name, *data).unwrap();
        }
        builder.into_inner().unwrap().finish().unwrap()
    }

    #[test]
    fn only_the_pinned_binary_at_the_pinned_path_is_unpacked() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("proxy");
        let good = b"the official proxy";
        let archive = layer(&[
            ("nix/store/other/bin/privatemode-proxy", b"something else"),
            ("./nix/store/x/bin/privatemode-proxy", good),
        ]);
        extract(
            &archive,
            "nix/store/x/bin/privatemode-proxy",
            &hex(good),
            &target,
        )
        .unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), good);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&target).unwrap().permissions().mode() & 0o777,
                0o700
            );
        }
        let tampered = layer(&[("nix/store/x/bin/privatemode-proxy", b"tampered")]);
        let other = directory.path().join("other");
        assert!(extract(
            &tampered,
            "nix/store/x/bin/privatemode-proxy",
            &hex(good),
            &other
        )
        .is_err());
        assert!(!other.exists(), "a mismatching binary is never written");
        assert!(extract(&archive, "nix/store/missing", &hex(good), &other).is_err());
    }

    /// Downloads Privatemode's real proxy and lets it attest with a made-up key,
    /// which Privatemode must refuse. Run with `--ignored` when online.
    #[tokio::test]
    #[ignore]
    async fn fetches_verifies_and_starts_the_official_proxy_which_refuses_a_bad_key() {
        let directory = tempfile::tempdir().unwrap();
        set_directory(directory.path().to_owned());
        let error = endpoint("pm_made_up_key_that_privatemode_rejects")
            .await
            .unwrap_err();
        assert_eq!(error, "Privatemode did not accept the summary key.");
        let binary = directory
            .path()
            .join(format!("privatemode-proxy-{VERSION}"));
        assert_eq!(hex(&std::fs::read(&binary).unwrap()), pin().unwrap().binary);
        assert!(
            std::fs::read_dir(directory.path())
                .unwrap()
                .flatten()
                .all(|e| !e.file_name().to_string_lossy().starts_with("key-")),
            "the key file is removed after a failed start"
        );
    }

    #[test]
    fn every_supported_architecture_has_a_full_pin() {
        if let Some(pin) = pin() {
            assert!(pin.layer.starts_with("sha256:") && pin.layer.len() == 71);
            assert_eq!(pin.binary.len(), 64);
            assert!(pin.path.ends_with("/bin/privatemode-proxy") && pin.path.contains(VERSION));
        }
    }
}
