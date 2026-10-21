// Sidecar process lifecycle management for llama-helper
// Handles spawning, health checking, keep-alive, and graceful shutdown

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout};
use tokio::sync::{Mutex, RwLock};

use super::models;

// ============================================================================
// Sidecar State Management
// ============================================================================

/// Sidecar process manager with keep-alive and health monitoring
pub struct SidecarManager {
    /// Child process handle
    child_process: Arc<Mutex<Option<Child>>>,
    sandbox: Arc<Mutex<Option<overtalk_worker_sandbox::Sandbox>>>,
    request_lock: Arc<Mutex<()>>,
    generation: Arc<AtomicU64>,
    owns_lifecycle: bool,

    /// Stdin writer for sending requests
    stdin_writer: Arc<Mutex<Option<ChildStdin>>>,

    /// Stdout reader for receiving responses
    stdout_reader: Arc<Mutex<Option<BufReader<ChildStdout>>>>,

    /// Last activity timestamp
    last_activity: Arc<RwLock<Instant>>,

    /// Health status
    is_healthy: Arc<AtomicBool>,

    /// Shutdown flag
    should_shutdown: Arc<AtomicBool>,

    /// Active request count (for graceful shutdown)
    active_request_count: Arc<AtomicUsize>,

    /// Path to llama-helper binary
    helper_binary_path: PathBuf,

    /// Current model path (if loaded)
    current_model_path: Arc<RwLock<Option<PathBuf>>>,

    /// Idle timeout in seconds (configurable via env var)
    idle_timeout_secs: u64,
}

/// RAII guard for tracking active requests
/// Decrements the active request count when dropped
struct RequestGuard {
    counter: Arc<AtomicUsize>,
}

impl RequestGuard {
    fn new(counter: Arc<AtomicUsize>) -> Self {
        counter.fetch_add(1, Ordering::SeqCst);
        Self { counter }
    }
}

impl Drop for RequestGuard {
    fn drop(&mut self) {
        self.counter.fetch_sub(1, Ordering::SeqCst);
    }
}

impl SidecarManager {
    /// Create a new sidecar manager
    pub fn new(_app_data_dir: PathBuf) -> Result<Self> {
        let helper_binary_path = Self::resolve_helper_binary()?;

        // Get idle timeout from env var or use default
        let idle_timeout_secs = std::env::var("LLAMA_IDLE_TIMEOUT")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(models::DEFAULT_IDLE_TIMEOUT_SECS);

        log::info!(
            "SidecarManager initialized with idle timeout: {}s",
            idle_timeout_secs
        );
        log::info!("Helper binary path: {}", helper_binary_path.display());

        Ok(Self {
            child_process: Arc::new(Mutex::new(None)),
            sandbox: Arc::new(Mutex::new(None)),
            request_lock: Arc::new(Mutex::new(())),
            generation: Arc::new(AtomicU64::new(0)),
            owns_lifecycle: true,
            stdin_writer: Arc::new(Mutex::new(None)),
            stdout_reader: Arc::new(Mutex::new(None)),
            last_activity: Arc::new(RwLock::new(Instant::now())),
            is_healthy: Arc::new(AtomicBool::new(false)),
            should_shutdown: Arc::new(AtomicBool::new(false)),
            active_request_count: Arc::new(AtomicUsize::new(0)),
            helper_binary_path,
            current_model_path: Arc::new(RwLock::new(None)),
            idle_timeout_secs,
        })
    }

    /// Only explicit absolute overrides and exact installed names may execute.
    /// Never choose a prefix match, directory, or working-directory binary.
    fn resolve_helper_binary() -> Result<PathBuf> {
        if let Some(value) = std::env::var_os("SONEFOLD_LLAMA_HELPER") {
            let path = PathBuf::from(value);
            if !path.is_absolute() || !path.is_file() {
                return Err(anyhow!(
                    "SONEFOLD_LLAMA_HELPER must name an absolute executable file"
                ));
            }
            return Ok(path);
        }
        if let Ok(exe) = std::env::current_exe() {
            if let Some(directory) = exe.parent() {
                for directory in [directory.to_path_buf(), directory.join("../Resources")] {
                    if let Some(path) = Self::installed_helper(&directory) {
                        return Ok(path);
                    }
                }
            }
        }
        if let Some(value) = std::env::var_os("RESOURCE_DIR") {
            let directory = PathBuf::from(value);
            if directory.is_absolute() {
                if let Some(path) = Self::installed_helper(&directory) {
                    return Ok(path);
                }
            }
        }
        #[cfg(debug_assertions)]
        {
            let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
            for directory in [
                workspace.join("target/release"),
                workspace.join("target/debug"),
            ] {
                if let Some(path) = Self::installed_helper(&directory) {
                    return Ok(path);
                }
            }
        }
        Err(anyhow!("Installed llama-helper is missing. Reinstall Overtalk or configure its absolute executable path."))
    }

    fn installed_helper(directory: &std::path::Path) -> Option<PathBuf> {
        let architecture = std::env::consts::ARCH;
        let target = match std::env::consts::OS {
            "linux" => format!("{architecture}-unknown-linux-gnu"),
            "macos" => format!("{architecture}-apple-darwin"),
            "windows" => format!("{architecture}-pc-windows-msvc"),
            _ => return None,
        };
        let extension = if cfg!(windows) { ".exe" } else { "" };
        [
            format!("llama-helper{extension}"),
            format!("llama-helper-{target}{extension}"),
        ]
        .into_iter()
        .map(|name| directory.join(name))
        .find(|path| path.is_file())
    }

    /// Ensure sidecar is running, spawn if needed
    pub async fn ensure_running(&self, model_path: PathBuf) -> Result<()> {
        // Check if already running with correct model
        {
            let current_model = self.current_model_path.read().await;
            if current_model.as_ref() == Some(&model_path) && self.is_healthy() {
                log::debug!("Sidecar already running with correct model");
                self.update_activity().await;
                return Ok(());
            }
        }

        // Need to spawn or restart
        self.spawn(model_path).await
    }

    /// Spawn the sidecar process
    async fn spawn(&self, model_path: PathBuf) -> Result<()> {
        // Shutdown existing process if running
        self.shutdown().await?;

        log::info!("Spawning llama-helper sidecar");
        log::info!("Model path: {}", model_path.display());

        let mut sandbox = overtalk_worker_sandbox::Sandbox::new(true)?;
        sandbox.read(&model_path)?;
        // Driver/runtime libraries are read-only; credentials and the database
        // are never mounted into the inference worker.
        #[cfg(target_os = "linux")]
        let raw = if let Some(wrapper) = overtalk_worker_sandbox::gpu_launcher() {
            let mut command = sandbox.command_with_wrapper(&self.helper_binary_path, &wrapper)?;
            // A launcher that reads TRAIN_CPU_NICE keeps background work below the interface.
            command.env("TRAIN_CPU_NICE", "5");
            command
        } else {
            sandbox.command(&self.helper_binary_path)?
        };
        #[cfg(not(target_os = "linux"))]
        let raw = sandbox.command(&self.helper_binary_path)?;
        let mut command = tokio::process::Command::from(raw);

        command
            .kill_on_drop(true)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null()) // Protocol errors are returned on bounded stdout; never retain model stderr.
            .env("LLAMA_IDLE_TIMEOUT", self.idle_timeout_secs.to_string());

        #[cfg(target_os = "windows")]
        {
            const CREATE_NO_WINDOW: u32 = 0x08000000;
            const BELOW_NORMAL_PRIORITY_CLASS: u32 = 0x00004000;

            command.creation_flags(CREATE_NO_WINDOW | BELOW_NORMAL_PRIORITY_CLASS);
        }

        let mut child = command.spawn().with_context(|| {
            format!(
                "Failed to spawn llama-helper at {:?}",
                self.helper_binary_path
            )
        })?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("Failed to get stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("Failed to get stdout"))?;

        *self.sandbox.lock().await = Some(sandbox);
        // Store handles
        {
            let mut child_lock = self.child_process.lock().await;
            *child_lock = Some(child);
        }

        {
            let mut stdin_lock = self.stdin_writer.lock().await;
            *stdin_lock = Some(stdin);
        }

        {
            let mut stdout_lock = self.stdout_reader.lock().await;
            *stdout_lock = Some(BufReader::new(stdout));
        }

        // Update state
        {
            let mut current_model = self.current_model_path.write().await;
            *current_model = Some(model_path);
        }

        self.generation.fetch_add(1, Ordering::AcqRel);
        self.is_healthy.store(true, Ordering::SeqCst);
        self.should_shutdown.store(false, Ordering::SeqCst);
        self.update_activity().await;

        log::info!("Sidecar spawned successfully");

        // Start background tasks
        self.start_health_check_loop();
        self.start_idle_check_loop();

        Ok(())
    }

    /// Send a request to the sidecar and wait for response
    pub async fn send_request(&self, request_json: String, timeout: Duration) -> Result<String> {
        // Track active request
        let _guard = RequestGuard::new(self.active_request_count.clone());

        let _exchange = self.request_lock.lock().await;
        if request_json.len() > 32 * 1024 * 1024 {
            return Err(anyhow!("Inference request is too large"));
        }
        let result = tokio::time::timeout(timeout, async {
            {
                let mut lock = self.stdin_writer.lock().await;
                let stdin = lock
                    .as_mut()
                    .ok_or_else(|| anyhow!("Sidecar not running"))?;
                stdin.write_all(request_json.as_bytes()).await?;
                stdin.write_all(b"\n").await?;
                stdin.flush().await?;
            }
            self.read_response().await
        })
        .await;
        match result {
            Ok(Ok(response)) => {
                self.update_activity().await;
                Ok(response)
            }
            Ok(Err(error)) => {
                self.shutdown().await?;
                Err(error)
            }
            Err(_) => {
                self.shutdown().await?;
                Err(anyhow!("Inference request timed out"))
            }
        }
    }

    /// Read a single line response from stdout
    async fn read_response(&self) -> Result<String> {
        let mut stdout_lock = self.stdout_reader.lock().await;
        let reader = stdout_lock
            .as_mut()
            .ok_or_else(|| anyhow!("Sidecar not running"))?;

        bounded_response(reader).await
    }

    /// Send ping to keep sidecar alive
    async fn send_ping(&self) -> Result<()> {
        let Ok(_exchange) = self.request_lock.try_lock() else {
            return Ok(());
        };
        let request = serde_json::json!({"type": "ping"}).to_string();
        let timeout = Duration::from_secs(5);

        // Note: We don't use send_request here to avoid incrementing active_request_count
        // for internal health checks, as that would prevent graceful shutdown

        // Write request
        {
            let mut stdin_lock = self.stdin_writer.lock().await;
            if let Some(stdin) = stdin_lock.as_mut() {
                stdin.write_all(request.as_bytes()).await?;
                stdin.write_all(b"\n").await?;
                stdin.flush().await?;
            } else {
                return Err(anyhow!("Sidecar not running"));
            }
        }

        // Read response
        let response = tokio::time::timeout(timeout, self.read_response()).await??;

        let resp: serde_json::Value = serde_json::from_str(&response)?;
        if resp.get("type").and_then(|t| t.as_str()) == Some("pong") {
            Ok(())
        } else {
            Err(anyhow!("Unexpected ping response: {}", response))
        }
    }

    /// Gracefully shutdown the sidecar
    /// Waits for active requests to complete before killing the process
    pub async fn shutdown_gracefully(&self) -> Result<()> {
        log::info!("Initiating graceful shutdown of sidecar");

        // Set shutdown flag to prevent new internal tasks
        self.should_shutdown.store(true, Ordering::SeqCst);

        // Wait for active requests to complete
        // We poll every 500ms
        let start = Instant::now();
        let max_wait = Duration::from_secs(600); // Wait up to 10 minutes for long generations

        loop {
            let count = self.active_request_count.load(Ordering::SeqCst);
            if count == 0 {
                log::info!("No active requests, proceeding with shutdown");
                break;
            }

            if start.elapsed() > max_wait {
                log::warn!(
                    "Timed out waiting for active requests ({} active), forcing shutdown",
                    count
                );
                break;
            }

            log::debug!("Waiting for {} active requests to complete...", count);
            tokio::time::sleep(Duration::from_millis(500)).await;
        }

        self.shutdown().await
    }

    /// Force shutdown the sidecar
    pub async fn shutdown(&self) -> Result<()> {
        // Set shutdown flag
        self.should_shutdown.store(true, Ordering::SeqCst);

        // Send shutdown command
        if self.is_healthy() {
            let request = serde_json::json!({"type": "shutdown"}).to_string();
            let timeout = Duration::from_millis(500);

            // Try to send shutdown command, but ignore errors
            // We don't use send_request to avoid incrementing counter
            let _ = tokio::time::timeout(timeout, async {
                let mut stdin_lock = self.stdin_writer.lock().await;
                if let Some(stdin) = stdin_lock.as_mut() {
                    stdin.write_all(request.as_bytes()).await?;
                    stdin.write_all(b"\n").await?;
                    stdin.flush().await?;
                }
                Ok::<(), anyhow::Error>(())
            })
            .await;
        }

        // Kill process if still running
        {
            let mut child_lock = self.child_process.lock().await;
            if let Some(mut child) = child_lock.take() {
                match tokio::time::timeout(Duration::from_secs(3), child.wait()).await {
                    Ok(Ok(status)) => {
                        log::info!("Sidecar exited with status: {}", status);
                    }
                    Ok(Err(e)) => {
                        log::error!("Failed to wait for sidecar: {}", e);
                    }
                    Err(_) => {
                        log::warn!("Sidecar didn't exit gracefully, killing");
                        if let Some(pid) = child.id() {
                            overtalk_worker_sandbox::kill_process_group(pid);
                        }
                        let _ = child.kill().await;
                    }
                }
            }
        }

        // Clear handles
        {
            let mut stdin_lock = self.stdin_writer.lock().await;
            *stdin_lock = None;
        }

        {
            let mut stdout_lock = self.stdout_reader.lock().await;
            *stdout_lock = None;
        }

        {
            let mut current_model = self.current_model_path.write().await;
            *current_model = None;
        }

        self.sandbox.lock().await.take();
        self.is_healthy.store(false, Ordering::SeqCst);

        log::info!("Sidecar shutdown complete");
        Ok(())
    }

    /// Check if sidecar is healthy
    pub fn is_healthy(&self) -> bool {
        self.is_healthy.load(Ordering::SeqCst)
    }

    /// Update last activity timestamp
    async fn update_activity(&self) {
        let mut last_activity = self.last_activity.write().await;
        *last_activity = Instant::now();
    }

    /// Get seconds since last activity
    async fn seconds_since_activity(&self) -> u64 {
        let last_activity = self.last_activity.read().await;
        last_activity.elapsed().as_secs()
    }

    /// Start health check loop (runs in background)
    fn start_health_check_loop(&self) {
        let manager = Self {
            child_process: self.child_process.clone(),
            sandbox: self.sandbox.clone(),
            request_lock: self.request_lock.clone(),
            generation: self.generation.clone(),
            owns_lifecycle: false,
            stdin_writer: self.stdin_writer.clone(),
            stdout_reader: self.stdout_reader.clone(),
            last_activity: self.last_activity.clone(),
            is_healthy: self.is_healthy.clone(),
            should_shutdown: self.should_shutdown.clone(),
            active_request_count: self.active_request_count.clone(),
            helper_binary_path: self.helper_binary_path.clone(),
            current_model_path: self.current_model_path.clone(),
            idle_timeout_secs: self.idle_timeout_secs,
        };

        let generation = self.generation.load(Ordering::Acquire);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(30));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

            loop {
                interval.tick().await;

                if manager.should_shutdown.load(Ordering::SeqCst)
                    || manager.generation.load(Ordering::Acquire) != generation
                {
                    log::debug!("Health check loop: shutdown flag set, exiting");
                    break;
                }

                if !manager.is_healthy() {
                    log::debug!("Health check loop: sidecar unhealthy, skipping ping");
                    continue;
                }

                // Don't ping if we are busy with a request
                if manager.active_request_count.load(Ordering::SeqCst) > 0 {
                    continue;
                }

                log::debug!("Health check: sending ping");
                if let Err(e) = manager.send_ping().await {
                    log::warn!("Health check failed: {}", e);
                    manager.is_healthy.store(false, Ordering::SeqCst);
                }
            }

            log::debug!("Health check loop exited");
        });
    }

    /// Start idle check loop (runs in background)
    fn start_idle_check_loop(&self) {
        let manager = Self {
            child_process: self.child_process.clone(),
            sandbox: self.sandbox.clone(),
            request_lock: self.request_lock.clone(),
            generation: self.generation.clone(),
            owns_lifecycle: false,
            stdin_writer: self.stdin_writer.clone(),
            stdout_reader: self.stdout_reader.clone(),
            last_activity: self.last_activity.clone(),
            is_healthy: self.is_healthy.clone(),
            should_shutdown: self.should_shutdown.clone(),
            active_request_count: self.active_request_count.clone(),
            helper_binary_path: self.helper_binary_path.clone(),
            current_model_path: self.current_model_path.clone(),
            idle_timeout_secs: self.idle_timeout_secs,
        };

        let generation = self.generation.load(Ordering::Acquire);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

            loop {
                interval.tick().await;

                if manager.should_shutdown.load(Ordering::SeqCst)
                    || manager.generation.load(Ordering::Acquire) != generation
                {
                    log::debug!("Idle check loop: shutdown flag set, exiting");
                    break;
                }

                // Don't shutdown if we are busy
                if manager.active_request_count.load(Ordering::SeqCst) > 0 {
                    // Update activity to prevent timeout immediately after request finishes
                    manager.update_activity().await;
                    continue;
                }

                let idle_secs = manager.seconds_since_activity().await;
                log::debug!("Idle check: {}s since last activity", idle_secs);

                if idle_secs > manager.idle_timeout_secs {
                    log::info!(
                        "Sidecar idle for {}s (timeout: {}s), shutting down",
                        idle_secs,
                        manager.idle_timeout_secs
                    );

                    if let Err(e) = manager.shutdown().await {
                        log::error!("Failed to shutdown idle sidecar: {}", e);
                    }

                    break;
                }
            }

            log::debug!("Idle check loop exited");
        });
    }
}

impl Drop for SidecarManager {
    fn drop(&mut self) {
        // Set shutdown flag
        if self.owns_lifecycle {
            self.should_shutdown.store(true, Ordering::SeqCst);
        }

        // Note: Actual cleanup happens in shutdown() method
        // We can't do async work in Drop, so this is best-effort
        log::debug!("SidecarManager dropped");
    }
}

async fn bounded_response<R: AsyncBufRead + Unpin>(reader: &mut R) -> Result<String> {
    const LIMIT: u64 = 8 * 1024 * 1024;
    let mut bytes = Vec::new();
    (&mut *reader)
        .take(LIMIT + 1)
        .read_until(b'\n', &mut bytes)
        .await
        .context("Failed to read worker response")?;
    if bytes.len() > LIMIT as usize || bytes.last() != Some(&b'\n') {
        return Err(anyhow!("Worker response was truncated or exceeded 8 MiB"));
    }
    Ok(String::from_utf8(bytes)?.trim().to_owned())
}

#[cfg(test)]
mod executable_resolution_tests {
    use super::*;
    #[tokio::test]
    async fn worker_cannot_grow_the_parent_with_unbounded_output() {
        let bytes = vec![b'x'; 8 * 1024 * 1024 + 1];
        assert!(bounded_response(&mut BufReader::new(bytes.as_slice()))
            .await
            .is_err());
        assert!(bounded_response(&mut BufReader::new(&b"truncated"[..]))
            .await
            .is_err());
        assert_eq!(
            bounded_response(&mut BufReader::new(&b"{\"ok\":true}\n"[..]))
                .await
                .unwrap(),
            "{\"ok\":true}"
        );
    }
    #[test]
    fn prefix_matches_and_directories_cannot_become_executables() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("llama-helper-untrusted"), b"not the helper").unwrap();
        let name = if cfg!(windows) {
            "llama-helper.exe"
        } else {
            "llama-helper"
        };
        std::fs::create_dir(dir.path().join(name)).unwrap();
        assert!(SidecarManager::installed_helper(dir.path()).is_none());
        std::fs::remove_dir(dir.path().join(name)).unwrap();
        std::fs::write(dir.path().join(name), b"packaged helper").unwrap();
        assert_eq!(
            SidecarManager::installed_helper(dir.path()),
            Some(dir.path().join(name))
        );
    }
}
