//! Optional local speech runtime. A cached stdio worker keeps model loads off the UI thread.
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, Runtime, State};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

static WORKER: Lazy<Mutex<Option<Worker>>> = Lazy::new(|| Mutex::new(None));
static KNOWLEDGE_WORKER: Lazy<Mutex<Option<Worker>>> = Lazy::new(|| Mutex::new(None));
static STREAM_WORKER: Lazy<Mutex<Option<Worker>>> = Lazy::new(|| Mutex::new(None));
static JOB: Lazy<Mutex<Option<(String, CancellationToken)>>> = Lazy::new(|| Mutex::new(None));
const ASR_IDLE_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SpeechSettings {
    pub enabled: bool,
    pub python: String,
    pub diarization_python: String,
    pub device: String,
    pub model: String,
    pub diarization_model: String,
    pub asr_model_dir: String,
    pub aligner_model_dir: String,
    pub diarization_model_path: String,
    pub live_diarization: bool,
    pub streaming_asr_model: String,
    pub streaming_asr_model_path: String,
}
impl Default for SpeechSettings {
    fn default() -> Self {
        let profile = super::quality_profile::detected();
        Self {
            enabled: true,
            python: if cfg!(windows) { "python" } else { "python3" }.into(),
            diarization_python: String::new(),
            device: profile.device.into(),
            model: profile.speech_model.into(),
            diarization_model: "nemotron-3".into(),
            asr_model_dir: String::new(),
            aligner_model_dir: String::new(),
            diarization_model_path: String::new(),
            live_diarization: false,
            streaming_asr_model: "nemotron-3.5".into(),
            streaming_asr_model_path: String::new(),
        }
    }
}

pub fn validate_settings(settings: &SpeechSettings) -> Result<(), String> {
    if !matches!(settings.device.as_str(), "cpu" | "cuda" | "auto") {
        return Err("Device must be cpu, cuda, or auto".into());
    }
    if !matches!(
        settings.model.as_str(),
        "qwen3-asr-1.7b"
            | "qwen3-asr-0.6b"
            | "parakeet-tdt-0.6b-v3"
            | "cohere-transcribe"
            | "moss-transcribe-diarize"
    ) {
        return Err("Unsupported local speech model".into());
    }
    if !matches!(
        settings.diarization_model.as_str(),
        "community-1" | "nemotron-3"
    ) {
        return Err("Unsupported diarization model".into());
    }
    if settings.python.trim().is_empty() || settings.python.len() > 4096 {
        return Err("Provide a Python executable path".into());
    }
    Ok(())
}

fn settings_path<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("speech-runtime.json"))
}
pub(crate) fn read_settings<R: Runtime>(app: &AppHandle<R>) -> Result<SpeechSettings, String> {
    match crate::security::storage::read_limited(&settings_path(app)?, 64 * 1024) {
        Ok(bytes) => {
            serde_json::from_slice(&bytes).map_err(|e| format!("Invalid speech configuration: {e}"))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(SpeechSettings::default()),
        Err(error) => Err(error.to_string()),
    }
}
fn runtime_dir<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, String> {
    let path = app
        .path()
        .resolve("speech-worker", tauri::path::BaseDirectory::Resource)
        .map_err(|e| e.to_string())?;
    if path.join("sonefold_speech/__main__.py").is_file() {
        return Ok(path);
    }
    #[cfg(debug_assertions)]
    {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../speech-worker");
        if path.join("sonefold_speech/__main__.py").is_file() {
            return path.canonicalize().map_err(|e| e.to_string());
        }
    }
    Err("Local speech worker is missing from this build".into())
}

// AppImage launchers set Python paths for their own payload. External virtual
// environments must use their own standard library and shared libraries.
fn configure_python_environment(command: &mut Command, bundle: Option<&Path>) {
    command.env_remove("PYTHONHOME").env_remove("PYTHONPATH");
    if let (Some(bundle), Some(paths)) = (bundle, std::env::var_os("LD_LIBRARY_PATH")) {
        let paths: Vec<_> = std::env::split_paths(&paths)
            .filter(|path| !path.starts_with(bundle))
            .collect();
        if paths.is_empty() {
            command.env_remove("LD_LIBRARY_PATH");
        } else if let Ok(paths) = std::env::join_paths(paths) {
            command.env("LD_LIBRARY_PATH", paths);
        }
    }
}

/// The installation a Python interpreter loads its packages from. A virtual
/// environment keeps them under its own prefix; otherwise follow links first, so
/// `~/.local/bin/python` resolves to its real installation rather than `~/.local`.
fn interpreter_prefix(python: &Path) -> std::io::Result<Option<PathBuf>> {
    if let Some(venv) = python
        .parent()
        .and_then(Path::parent)
        .filter(|prefix| prefix.join("pyvenv.cfg").is_file())
    {
        return Ok(Some(venv.to_path_buf()));
    }
    Ok(python
        .canonicalize()?
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf))
}

struct Worker {
    sandbox: overtalk_worker_sandbox::Sandbox,
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    last_activity: tokio::time::Instant,
}
impl Worker {
    fn launch<R: Runtime>(
        app: &AppHandle<R>,
        settings: &SpeechSettings,
        streaming: bool,
        extra_read: Option<&Path>,
    ) -> Result<Self, String> {
        if !settings.enabled {
            return Err("Enable the local speech runtime in Settings first".into());
        }
        validate_settings(settings)?;
        let selected_python = if streaming && !settings.diarization_python.trim().is_empty() {
            &settings.diarization_python
        } else {
            &settings.python
        };
        let python = which::which(selected_python).map_err(|_| {
            "Python executable not found; configure your speech environment path".to_string()
        })?;
        let mut sandbox = overtalk_worker_sandbox::Sandbox::new(settings.device != "cpu")
            .map_err(|e| e.to_string())?;
        let directory = runtime_dir(app)?;
        sandbox.cwd(&directory).map_err(|e| e.to_string())?;
        for path in [
            &settings.asr_model_dir,
            &settings.aligner_model_dir,
            &settings.diarization_model_path,
            &settings.streaming_asr_model_path,
        ] {
            if !path.trim().is_empty() {
                sandbox.read(Path::new(path)).map_err(|e| e.to_string())?;
            }
        }
        if let Some(path) = extra_read {
            sandbox.read(path).map_err(|e| e.to_string())?;
        }
        if let Some(cache) = dirs::cache_dir()
            .map(|p| p.join("huggingface/hub"))
            .filter(|p| p.is_dir())
        {
            sandbox.read(&cache).map_err(|e| e.to_string())?;
        }
        if !settings.diarization_python.trim().is_empty() {
            let other = which::which(&settings.diarization_python).map_err(|e| e.to_string())?;
            sandbox.read(&other).map_err(|e| e.to_string())?;
            if let Some(prefix) = interpreter_prefix(&other).map_err(|e| e.to_string())? {
                sandbox.read(prefix).map_err(|e| e.to_string())?;
            }
        }
        if let Some(ffmpeg) = crate::audio::ffmpeg::find_ffmpeg_path() {
            sandbox.read(ffmpeg).map_err(|e| e.to_string())?;
        }
        let mut command;
        #[cfg(target_os = "linux")]
        {
            let wrapper = overtalk_worker_sandbox::gpu_launcher();
            if let Some(wrapper) = wrapper.filter(|_| settings.device != "cpu") {
                command = Command::from(
                    sandbox
                        .command_with_wrapper(&python, &wrapper)
                        .map_err(|e| e.to_string())?,
                );
                // A launcher that reads TRAIN_CPU_NICE keeps background work below the interface,
                // even when the app itself runs at normal priority.
                command.env("TRAIN_CPU_NICE", "5");
            } else {
                command = Command::from(sandbox.command(&python).map_err(|e| e.to_string())?);
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            command = Command::from(sandbox.command(&python).map_err(|e| e.to_string())?);
        }
        configure_python_environment(
            &mut command,
            std::env::var_os("APPDIR").as_deref().map(Path::new),
        );
        command
            .args(["-E", "-u", "-m", "sonefold_speech"])
            .env(
                "SONEFOLD_CPU_THREADS",
                if super::jobs::low_resource() {
                    "2"
                } else {
                    "8"
                },
            )
            .current_dir(&directory)
            .env("HF_HUB_OFFLINE", "1")
            .env("TRANSFORMERS_OFFLINE", "1")
            .env("HF_HUB_DISABLE_TELEMETRY", "1")
            .env("PYANNOTE_METRICS_ENABLED", "0")
            .env("SONEFOLD_SPEECH_DEVICE", &settings.device)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        if let Some(cache) = dirs::cache_dir()
            .map(|p| p.join("huggingface/hub"))
            .filter(|p| p.is_dir())
        {
            command.env("HF_HUB_CACHE", cache);
        }
        if !settings.diarization_python.trim().is_empty() {
            command.env("SONEFOLD_DIARIZATION_PYTHON", &settings.diarization_python);
        }
        if !settings.asr_model_dir.trim().is_empty() {
            let variable = match settings.model.as_str() {
                "cohere-transcribe" => "SONEFOLD_COHERE_MODEL_DIR",
                "moss-transcribe-diarize" => "SONEFOLD_MOSS_MODEL_DIR",
                "qwen3-asr-0.6b" => "SONEFOLD_QWEN3_ASR_0_6B_DIR",
                "parakeet-tdt-0.6b-v3" => "SONEFOLD_PARAKEET_V3_MODEL",
                _ => "SONEFOLD_QWEN3_ASR_1_7B_DIR",
            };
            command.env(variable, &settings.asr_model_dir);
        }
        if !settings.aligner_model_dir.trim().is_empty() {
            command.env("SONEFOLD_QWEN3_ALIGNER_DIR", &settings.aligner_model_dir);
        }
        if !settings.diarization_model_path.trim().is_empty() {
            command.env(
                "SONEFOLD_DIARIZATION_MODEL_DIR",
                &settings.diarization_model_path,
            );
        }
        if !settings.streaming_asr_model_path.trim().is_empty() {
            command.env(
                "SONEFOLD_STREAMING_ASR_MODEL_PATH",
                &settings.streaming_asr_model_path,
            );
        }
        if let Some(path) = crate::audio::ffmpeg::find_ffmpeg_path() {
            command.env("SONEFOLD_FFMPEG_PATH", path);
        }
        #[cfg(windows)]
        {
            command.creation_flags(0x08000000);
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.as_std_mut().process_group(0);
        }
        let mut child = command
            .spawn()
            .map_err(|e| format!("Cannot start local speech worker: {e}"))?;
        let stdin = child.stdin.take().ok_or("Worker stdin is unavailable")?;
        let stdout = BufReader::new(child.stdout.take().ok_or("Worker stdout is unavailable")?);
        if let Some(stderr) = child.stderr.take() {
            tokio::spawn(async move {
                let mut reader = BufReader::new(stderr);
                // Drain without retaining sensitive transcript text or unbounded output.
                let mut buffer = [0u8; 8192];
                while let Ok(n) = reader.read(&mut buffer).await {
                    if n == 0 {
                        break;
                    }
                }
            });
        }
        Ok(Self {
            sandbox,
            child,
            stdin,
            stdout,
            last_activity: tokio::time::Instant::now(),
        })
    }

    async fn terminate(&mut self) {
        #[cfg(unix)]
        if let Some(pid) = self.child.id() {
            // Signal the private process group, including the responsive wrapper
            // and decoder children. TERM lets the wrapper clean up its MPS daemon.
            // SAFETY: kill takes no pointers. The child was spawned as the leader of
            // its own process group, and `id()` is Some only until it is reaped, so
            // the negative pid names that group and not one that reused the number.
            unsafe {
                libc::kill(-(pid as i32), libc::SIGTERM);
            }
            if tokio::time::timeout(Duration::from_secs(3), self.child.wait())
                .await
                .is_err()
            {
                // SAFETY: as above; `pid` still names the worker's process group,
                // which keeps its number until the leader is reaped below.
                unsafe {
                    libc::kill(-(pid as i32), libc::SIGKILL);
                }
                let _ = self.child.wait().await;
            }
            return;
        }
        #[cfg(windows)]
        if let Some(pid) = self.child.id() {
            let mut taskkill = Command::new("taskkill");
            taskkill
                .args(["/PID", &pid.to_string(), "/T", "/F"])
                .creation_flags(0x08000000)
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            let _ = tokio::time::timeout(Duration::from_secs(5), taskkill.status()).await;
        }
        let _ = self.child.kill().await;
        let _ = self.child.wait().await;
    }

    async fn request(
        &mut self,
        mut request: Value,
        token: &CancellationToken,
    ) -> Result<Value, String> {
        let tracks_activity = !request
            .get("operation")
            .and_then(Value::as_str)
            .is_some_and(|op| {
                matches!(op, "health" | "stream_health" | "ping" | "knowledge_health")
            });
        let input = request
            .get("audio_path")
            .and_then(Value::as_str)
            .map(|path| self.sandbox.input(Path::new(path)))
            .transpose()
            .map_err(|e| e.to_string())?;
        if let Some(input) = &input {
            request["audio_path"] = json!(input.path());
        }
        let id = uuid::Uuid::new_v4().to_string();
        request["id"] = json!(&id);
        let mut bytes = serde_json::to_vec(&request).map_err(|e| e.to_string())?;
        bytes.push(b'\n');
        self.stdin
            .write_all(&bytes)
            .await
            .map_err(|e| format!("Speech worker disconnected: {e}"))?;
        self.stdin.flush().await.map_err(|e| e.to_string())?;
        let read = async {
            let mut response = Vec::new();
            (&mut self.stdout)
                .take(64 * 1024 * 1024)
                .read_until(b'\n', &mut response)
                .await
                .map_err(|e| e.to_string())?;
            if response.len() >= 64 * 1024 * 1024 && response.last() != Some(&b'\n') {
                return Err("Speech worker response exceeded the 64 MiB limit".into());
            }
            if response.is_empty() || response.last() != Some(&b'\n') {
                let status = tokio::time::timeout(Duration::from_secs(1), self.child.wait())
                    .await
                    .ok()
                    .and_then(Result::ok)
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "no response".into());
                return Err(format!("The local speech runtime stopped ({status}). Check the Python executable and its installed dependencies in Transcription settings."));
            }
            let value: Value = serde_json::from_slice(&response)
                .map_err(|_| "Worker returned invalid JSON".to_string())?;
            if value.get("id").and_then(Value::as_str) != Some(id.as_str()) {
                return Err("Worker response belongs to another request".into());
            }
            if value.get("ok").and_then(Value::as_bool) != Some(true) {
                return Err(value
                    .get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("Local speech processing failed")
                    .to_string());
            }
            Ok(value)
        };
        let result = tokio::select! {
            result = read => result,
            _ = token.cancelled() => Err("Speech processing cancelled".into()),
            _ = tokio::time::sleep(Duration::from_secs(3600)) => Err("Speech worker timed out".into()),
        };
        if result.is_ok() && tracks_activity {
            self.last_activity = tokio::time::Instant::now();
        }
        result
    }
}

pub(crate) async fn request<R: Runtime>(
    app: &AppHandle<R>,
    value: Value,
    token: &CancellationToken,
) -> Result<Value, String> {
    let streaming = value
        .get("operation")
        .and_then(Value::as_str)
        .is_some_and(|op| op.starts_with("stream_"));
    let diagnostic = value
        .get("operation")
        .and_then(Value::as_str)
        .is_some_and(|op| matches!(op, "health" | "stream_health" | "ping" | "knowledge_health"));
    let knowledge = value
        .get("operation")
        .and_then(Value::as_str)
        .is_some_and(|op| op.starts_with("knowledge_"));
    // Same lock order as recording startup: lifecycle, then inference budget.
    let _lifecycle = if knowledge && !diagnostic {
        Some(crate::audio::common::acquire_engine_lifecycle_lock().await)
    } else {
        None
    };
    let _budget = if diagnostic {
        None
    } else {
        Some(super::model_budget::acquire(token).await?)
    };
    if knowledge && !diagnostic {
        if crate::audio::recording_commands::is_recording().await {
            return Err("Stop recording before running the quality retrieval model".into());
        }
        if let Some(mut idle) = WORKER.lock().await.take() {
            idle.terminate().await;
        }
        if let Some(mut idle) = STREAM_WORKER.lock().await.take() {
            idle.terminate().await;
        }
        crate::summary::summary_engine::client::release_idle_model()
            .await
            .map_err(|e| e.to_string())?;
    } else if !knowledge && !diagnostic {
        shutdown_knowledge().await;
        crate::summary::summary_engine::client::release_idle_model()
            .await
            .map_err(|e| e.to_string())?;
    }
    let mut worker = if knowledge {
        KNOWLEDGE_WORKER.lock().await
    } else if streaming {
        STREAM_WORKER.lock().await
    } else {
        WORKER.lock().await
    };
    if token.is_cancelled() {
        return Err("Speech processing cancelled".into());
    }
    let extra_read = value
        .get("directory")
        .and_then(Value::as_str)
        .map(Path::new);
    if extra_read.is_some_and(|path| {
        worker
            .as_ref()
            .is_some_and(|w| !w.sandbox.allows_read(path))
    }) {
        if let Some(mut old) = worker.take() {
            old.terminate().await;
        }
    }
    if worker.is_none() {
        let settings = if knowledge {
            let config = super::retrieval::read_settings(app)?;
            SpeechSettings {
                enabled: true,
                python: config.python,
                device: config.device,
                ..Default::default()
            }
        } else {
            read_settings(app)?
        };
        *worker = Some(Worker::launch(app, &settings, streaming, extra_read)?);
    }
    let result = worker.as_mut().unwrap().request(value, token).await;
    if result.is_err() {
        if streaming {
            crate::speech_stream::invalidate_prepared();
        }
        if let Some(mut process) = worker.take() {
            process.terminate().await;
        }
    }
    result
}

pub(crate) async fn release_idle_asr() {
    if !crate::audio::recording_commands::is_recording().await {
        if let Some(mut idle) = WORKER.lock().await.take() {
            idle.terminate().await;
        }
        if let Some(mut idle) = STREAM_WORKER.lock().await.take() {
            idle.terminate().await;
        }
    }
}

async fn expire_idle_worker(worker: &mut Option<Worker>, now: tokio::time::Instant) {
    if worker
        .as_ref()
        .is_some_and(|w| w.last_activity + ASR_IDLE_TIMEOUT <= now)
    {
        if let Some(mut idle) = worker.take() {
            idle.terminate().await;
        }
    }
}

/// Keep successful batch ASR warm briefly; failures/cancellation release it now.
/// Summary and retrieval jobs can reclaim it immediately through the GPU budget.
pub(crate) async fn finish_batch(success: bool) {
    if !success || super::jobs::low_resource() {
        shutdown().await;
        return;
    }
    tokio::spawn(async {
        tokio::time::sleep(ASR_IDLE_TIMEOUT).await;
        // Serialize with recording startup and batch jobs before reclaiming memory.
        let _lifecycle = crate::audio::common::acquire_engine_lifecycle_lock().await;
        if !crate::audio::recording_commands::is_recording().await {
            expire_idle_worker(&mut *WORKER.lock().await, tokio::time::Instant::now()).await;
        }
    });
}
pub(crate) async fn shutdown_knowledge() {
    if let Some(mut worker) = KNOWLEDGE_WORKER.lock().await.take() {
        worker.terminate().await;
    }
}
pub async fn shutdown() {
    shutdown_knowledge().await;
    crate::speech_stream::invalidate_prepared();
    if let Some((_, token)) = JOB.lock().await.as_ref() {
        token.cancel();
    }
    if let Some(mut worker) = WORKER.lock().await.take() {
        worker.terminate().await;
    }
    if let Some(mut worker) = STREAM_WORKER.lock().await.take() {
        worker.terminate().await;
    }
}

#[tauri::command]
pub async fn speech_get_settings<R: Runtime>(app: AppHandle<R>) -> Result<SpeechSettings, String> {
    read_settings(&app)
}

#[tauri::command]
pub async fn speech_save_settings<R: Runtime>(
    app: AppHandle<R>,
    settings: SpeechSettings,
) -> Result<SpeechSettings, String> {
    let _lifecycle = crate::audio::common::acquire_engine_lifecycle_lock().await;
    validate_settings(&settings)?;
    if crate::audio::recording_commands::is_recording().await {
        return Err("Stop recording before changing speech settings".into());
    }
    if JOB.lock().await.is_some() {
        return Err("Wait for or cancel speech processing before changing settings".into());
    }
    shutdown().await;
    crate::integrations::write_private_json(&settings_path(&app)?, &settings)?;
    Ok(settings)
}

#[tauri::command]
pub async fn speech_health<R: Runtime>(app: AppHandle<R>) -> Result<Value, String> {
    if is_processing().await {
        return Err("Wait for or cancel speech processing before checking the runtime".into());
    }
    let settings = read_settings(&app)?;
    let operation = if settings.live_diarization {
        "stream_health"
    } else {
        "health"
    };
    request(&app, json!({"operation": operation, "model": settings.model, "asr_model":settings.streaming_asr_model}), &CancellationToken::new()).await
}

pub async fn load<R: Runtime>(app: &AppHandle<R>, model: &str) -> Result<(), String> {
    let settings = read_settings(app)?;
    if settings.live_diarization {
        if let Some(mut worker) = WORKER.lock().await.take() {
            worker.terminate().await;
        }
        return crate::speech_stream::prepare(app).await;
    }
    request(
        app,
        json!({"operation": "load", "model": model, "device": settings.device, "offline": true}),
        &CancellationToken::new(),
    )
    .await?;
    Ok(())
}

pub(crate) async fn is_processing() -> bool {
    JOB.lock().await.is_some()
}

#[derive(Debug, Deserialize)]
pub struct AnalyzedSegment {
    pub text: String,
    pub start: f64,
    pub end: f64,
    #[serde(default)]
    pub speaker: Option<String>,
    #[serde(default)]
    pub speaker_confidence: Option<f64>,
    #[serde(default)]
    pub overlap: bool,
}
pub fn validate_segments(segments: &[AnalyzedSegment]) -> Result<(), String> {
    if segments.is_empty() {
        return Err("No speech detected; existing transcripts were preserved".into());
    }
    if segments.len() > 100_000 {
        return Err("Too many transcript segments".into());
    }
    for segment in segments {
        if segment.text.trim().is_empty()
            || !segment.start.is_finite()
            || !segment.end.is_finite()
            || segment.start < 0.0
            || segment.end < segment.start
            || segment.end > 86400.0
            || segment.speaker.as_ref().is_some_and(|s| s.len() > 100)
            || segment
                .speaker_confidence
                .is_some_and(|c| !c.is_finite() || !(0.0..=1.0).contains(&c))
        {
            return Err(
                "Speech worker returned invalid transcript segments; original transcript preserved"
                    .into(),
            );
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn speech_cancel(job_id: String) -> bool {
    let job = JOB.lock().await;
    if let Some((id, token)) = job.as_ref() {
        if *id == job_id {
            token.cancel();
            return true;
        }
    }
    false
}

#[tauri::command]
pub async fn speech_analyze_meeting<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, crate::state::AppState>,
    meeting_id: String,
    job_id: String,
    diarize: bool,
    num_speakers: Option<u32>,
) -> Result<Value, String> {
    analyze_with_token(
        app,
        state,
        meeting_id,
        job_id,
        diarize,
        num_speakers,
        CancellationToken::new(),
    )
    .await
}

pub(crate) async fn analyze_with_token<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, crate::state::AppState>,
    meeting_id: String,
    job_id: String,
    diarize: bool,
    num_speakers: Option<u32>,
    token: CancellationToken,
) -> Result<Value, String> {
    let lifecycle = tokio::select! {
        biased;
        _ = token.cancelled() => return Err("Speech processing cancelled".into()),
        lock = crate::audio::common::acquire_engine_lifecycle_lock() => lock,
    };
    if crate::audio::recording_commands::is_recording().await {
        return Err("Stop recording before analyzing a saved meeting".into());
    }
    if num_speakers.is_some_and(|n| n == 0 || n > 32) {
        return Err("Speaker count must be between 1 and 32".into());
    }
    {
        let mut job = JOB.lock().await;
        if job.is_some() {
            return Err("Another speech job is running".into());
        }
        *job = Some((job_id.clone(), token.clone()));
    }
    drop(lifecycle);
    let result = analyze(
        &app,
        state.db_manager.pool(),
        &meeting_id,
        diarize,
        num_speakers,
        &token,
    )
    .await;
    *JOB.lock().await = None;
    finish_batch(result.is_ok()).await;
    let _ = app.emit(
        "speech-analysis-finished",
        json!({"meeting_id": meeting_id, "job_id": job_id, "success": result.is_ok()}),
    );
    result
}

async fn analyze<R: Runtime>(
    app: &AppHandle<R>,
    pool: &sqlx::SqlitePool,
    meeting_id: &str,
    diarize: bool,
    num_speakers: Option<u32>,
    token: &CancellationToken,
) -> Result<Value, String> {
    let folder: Option<(Option<String>,)> =
        sqlx::query_as("SELECT folder_path FROM meetings WHERE id = ?")
            .bind(meeting_id)
            .fetch_optional(pool)
            .await
            .map_err(|e| e.to_string())?;
    let folder = folder
        .and_then(|f| f.0)
        .ok_or("This meeting has no stored recording")?;
    let audio = crate::audio::retranscription::find_audio_file(Path::new(&folder))
        .map_err(|e| e.to_string())?;
    let settings = read_settings(app)?;
    let vocabulary = super::projects::vocabulary_for_meeting(pool, meeting_id).await?;
    let response = request(app, json!({"operation":"analyze", "audio_path":audio, "model":settings.model,
        "context":vocabulary,
        "diarize":diarize, "diarization_model":settings.diarization_model, "num_speakers":num_speakers,
        "language":recognition_language(crate::get_language_preference_internal()), "device":settings.device,"offline":true}), token).await?;
    if token.is_cancelled() {
        return Err("Speech processing cancelled".into());
    }
    let segments: Vec<AnalyzedSegment> = serde_json::from_value(response["segments"].clone())
        .map_err(|e| format!("Invalid worker result: {e}"))?;
    validate_segments(&segments)?;
    let (turns, exclusive_turns): (
        Vec<crate::speakers::DiarizationTurn>,
        Vec<crate::speakers::DiarizationTurn>,
    ) = if diarize {
        let turns = serde_json::from_value(response["turns"].clone())
            .map_err(|e| format!("Invalid diarization turns: {e}"))?;
        let exclusive = serde_json::from_value(response["exclusive_turns"].clone())
            .map_err(|e| format!("Invalid exclusive diarization turns: {e}"))?;
        (turns, exclusive)
    } else {
        (Vec::new(), Vec::new())
    };
    crate::speakers::validate_turns(&turns)?;
    crate::speakers::validate_turns(&exclusive_turns)?;
    let mut transaction = crate::database::begin_write(pool)
        .await
        .map_err(|e| e.to_string())?;
    snapshot_transcript(&mut transaction, meeting_id).await?;
    sqlx::query("DELETE FROM transcripts WHERE meeting_id=?")
        .bind(meeting_id)
        .execute(&mut *transaction)
        .await
        .map_err(|e| e.to_string())?;
    for segment in &segments {
        let seconds = segment.start.floor() as u64;
        let timestamp = format!(
            "{:02}:{:02}:{:02}",
            seconds / 3600,
            seconds / 60 % 60,
            seconds % 60
        );
        sqlx::query("INSERT INTO transcripts(id,meeting_id,transcript,timestamp,audio_start_time,audio_end_time,duration,speaker_id,speaker_confidence,overlap) VALUES(?,?,?,?,?,?,?,?,?,?)")
            .bind(format!("transcript-{}",uuid::Uuid::new_v4())).bind(meeting_id).bind(&segment.text).bind(timestamp)
            .bind(segment.start).bind(segment.end).bind(segment.end-segment.start).bind(&segment.speaker)
            .bind(segment.speaker_confidence).bind(segment.overlap).execute(&mut *transaction).await.map_err(|e| e.to_string())?;
    }
    // A fresh clustering run can reorder anonymous IDs. Reset names instead of
    // silently attaching a previously named person to a different voice.
    sqlx::query("DELETE FROM speaker_turns WHERE meeting_id=?")
        .bind(meeting_id)
        .execute(&mut *transaction)
        .await
        .map_err(|e| e.to_string())?;
    sqlx::query("DELETE FROM meeting_speakers WHERE meeting_id=?")
        .bind(meeting_id)
        .execute(&mut *transaction)
        .await
        .map_err(|e| e.to_string())?;
    if diarize {
        crate::speakers::apply_diarization_on_connection(
            &mut transaction,
            meeting_id,
            &turns,
            &exclusive_turns,
            response["diarization_model"]
                .as_str()
                .ok_or("Missing diarization model")?,
        )
        .await?;
    }
    sqlx::query("UPDATE meetings SET updated_at=? WHERE id=?")
        .bind(chrono::Utc::now().to_rfc3339())
        .bind(meeting_id)
        .execute(&mut *transaction)
        .await
        .map_err(|e| e.to_string())?;
    if token.is_cancelled() {
        return Err("Speech processing cancelled".into());
    }
    transaction.commit().await.map_err(|e| e.to_string())?;
    if diarize {
        if let Err(error) = super::voice_memory::consolidate(app, pool, meeting_id, false).await {
            log::warn!("Transcript saved; speaker consolidation was unavailable: {error}");
        }
        if let Err(error) = super::voice_memory::recognize_if_automatic(app, pool, meeting_id).await
        {
            log::warn!("Transcript saved; voice memory was unavailable: {error}");
        }
    }
    let _ = app.emit("meeting-speakers-updated", meeting_id);
    Ok(
        json!({"segments":segments.len(),"model":settings.model,"diarized":diarize,"backup_saved":true,"warnings":response.get("warnings").cloned().unwrap_or(json!([]))}),
    )
}

pub(crate) async fn snapshot_transcript(
    connection: &mut sqlx::SqliteConnection,
    meeting_id: &str,
) -> Result<(), String> {
    sqlx::query(
        "INSERT INTO speech_transcript_backups(meeting_id,created_at,transcripts) \
         SELECT ?,?,json_group_array(json_object('id',id,'text',transcript,'timestamp',timestamp,\
         'summary',summary,'action_items',action_items,'key_points',key_points,'audio_start_time',\
         audio_start_time,'audio_end_time',audio_end_time,'duration',duration,'speaker',speaker,\
         'speaker_id',speaker_id,'speaker_confidence',speaker_confidence,'overlap',overlap,\
         'is_partial',is_partial)) FROM transcripts WHERE meeting_id=?",
    )
    .bind(meeting_id)
    .bind(chrono::Utc::now().to_rfc3339())
    .bind(meeting_id)
    .execute(&mut *connection)
    .await
    .map_err(|e| e.to_string())?;
    sqlx::query(
        "UPDATE speech_transcript_backups SET speakers=(SELECT \
         json_group_array(json_object('id',id,'name',name)) FROM meeting_speakers \
         WHERE meeting_id=?), turns=(SELECT json_group_array(json_object('speaker_id',speaker_id,\
         'start_time',start_time,'end_time',end_time,'confidence',confidence,'model',model)) \
         FROM speaker_turns WHERE meeting_id=?) WHERE id=last_insert_rowid()",
    )
    .bind(meeting_id)
    .bind(meeting_id)
    .execute(connection)
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn speech_backup_status(
    state: State<'_, crate::state::AppState>,
    meeting_id: String,
) -> Result<Option<String>, String> {
    sqlx::query_scalar("SELECT created_at FROM speech_transcript_backups WHERE meeting_id=? ORDER BY id DESC LIMIT 1")
        .bind(meeting_id).fetch_optional(state.db_manager.pool()).await.map_err(|e| e.to_string())
}

async fn restore_transcript(pool: &sqlx::SqlitePool, meeting_id: &str) -> Result<(), String> {
    let mut transaction = crate::database::begin_write(pool)
        .await
        .map_err(|e| e.to_string())?;
    let (id, transcripts, speakers, turns): (i64, String, String, String) = sqlx::query_as(
        "SELECT id,transcripts,speakers,turns FROM speech_transcript_backups WHERE meeting_id=? \
         ORDER BY id DESC LIMIT 1",
    )
    .bind(meeting_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|e| e.to_string())?
    .ok_or("No transcript recovery copy exists")?;
    // Validate the recovery copy before changing any live rows.
    for (label, value) in [
        ("transcript", &transcripts),
        ("speakers", &speakers),
        ("turns", &turns),
    ] {
        if !serde_json::from_str::<Value>(value)
            .map_err(|e| format!("Invalid {label} recovery copy: {e}"))?
            .is_array()
        {
            return Err(format!("Invalid {label} recovery copy"));
        }
    }
    // Preserve the current version, so restoring can itself be undone.
    snapshot_transcript(&mut transaction, meeting_id).await?;
    sqlx::query("DELETE FROM transcripts WHERE meeting_id=?")
        .bind(meeting_id)
        .execute(&mut *transaction)
        .await
        .map_err(|e| e.to_string())?;
    sqlx::query("DELETE FROM speaker_turns WHERE meeting_id=?")
        .bind(meeting_id)
        .execute(&mut *transaction)
        .await
        .map_err(|e| e.to_string())?;
    sqlx::query("DELETE FROM meeting_speakers WHERE meeting_id=?")
        .bind(meeting_id)
        .execute(&mut *transaction)
        .await
        .map_err(|e| e.to_string())?;
    sqlx::query(
        "INSERT INTO transcripts(id,meeting_id,transcript,timestamp,summary,action_items,\
         key_points,audio_start_time,audio_end_time,duration,speaker,speaker_id,speaker_confidence,\
         overlap,is_partial) SELECT json_extract(value,'$.id'),?,json_extract(value,'$.text'),\
         json_extract(value,'$.timestamp'),json_extract(value,'$.summary'),\
         json_extract(value,'$.action_items'),json_extract(value,'$.key_points'),\
         json_extract(value,'$.audio_start_time'),json_extract(value,'$.audio_end_time'),\
         json_extract(value,'$.duration'),json_extract(value,'$.speaker'),\
         json_extract(value,'$.speaker_id'),json_extract(value,'$.speaker_confidence'),\
         COALESCE(json_extract(value,'$.overlap'),0),COALESCE(json_extract(value,'$.is_partial'),\
         0) FROM json_each(?)",
    )
    .bind(meeting_id)
    .bind(transcripts)
    .execute(&mut *transaction)
    .await
    .map_err(|e| e.to_string())?;
    sqlx::query(
        "INSERT INTO meeting_speakers(meeting_id,id,name) SELECT \
         ?,json_extract(value,'$.id'),json_extract(value,'$.name') FROM json_each(?)",
    )
    .bind(meeting_id)
    .bind(speakers)
    .execute(&mut *transaction)
    .await
    .map_err(|e| e.to_string())?;
    sqlx::query(
        "INSERT INTO speaker_turns(meeting_id,speaker_id,start_time,end_time,confidence,model) \
         SELECT ?,json_extract(value,'$.speaker_id'),json_extract(value,'$.start_time'),\
         json_extract(value,'$.end_time'),json_extract(value,'$.confidence'),\
         json_extract(value,'$.model') FROM json_each(?)",
    )
    .bind(meeting_id)
    .bind(turns)
    .execute(&mut *transaction)
    .await
    .map_err(|e| e.to_string())?;
    sqlx::query("DELETE FROM speech_transcript_backups WHERE id=? AND meeting_id=?")
        .bind(id)
        .bind(meeting_id)
        .execute(&mut *transaction)
        .await
        .map_err(|e| e.to_string())?;
    sqlx::query("UPDATE meetings SET updated_at=? WHERE id=?")
        .bind(chrono::Utc::now().to_rfc3339())
        .bind(meeting_id)
        .execute(&mut *transaction)
        .await
        .map_err(|e| e.to_string())?;
    transaction.commit().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn speech_restore_transcript<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, crate::state::AppState>,
    meeting_id: String,
) -> Result<(), String> {
    let lifecycle = crate::audio::common::acquire_engine_lifecycle_lock().await;
    if crate::audio::recording_commands::is_recording().await {
        return Err("Stop recording before restoring a transcript".into());
    }
    let mut job = JOB.lock().await;
    if job.is_some() {
        return Err("Wait for or cancel speech processing before restoring a transcript".into());
    }
    *job = Some((uuid::Uuid::new_v4().to_string(), CancellationToken::new()));
    drop(job);
    drop(lifecycle);
    let result = restore_transcript(state.db_manager.pool(), &meeting_id).await;
    *JOB.lock().await = None;
    if result.is_ok() {
        let _ = app.emit("meeting-speakers-updated", &meeting_id);
    }
    result
}

/// Provider adapter uses the existing live VAD pipeline; ASR model remains loaded between chunks.
pub struct SpeechProvider<R: Runtime> {
    pub app: AppHandle<R>,
    pub model: String,
}
impl<R: Runtime> SpeechProvider<R> {
    pub(crate) async fn prepare(&self, token: &CancellationToken) -> Result<(), String> {
        let settings = read_settings(&self.app)?;
        request(
            &self.app,
            json!({"operation":"load", "model":self.model,
            "device":settings.device,"offline":true}),
            token,
        )
        .await?;
        Ok(())
    }
    pub(crate) async fn transcribe_with_cancellation(
        &self,
        audio: Vec<f32>,
        language: Option<String>,
        token: &CancellationToken,
    ) -> Result<
        crate::audio::transcription::TranscriptResult,
        crate::audio::transcription::TranscriptionError,
    > {
        use crate::audio::transcription::{TranscriptResult, TranscriptionError};
        let file = tempfile::NamedTempFile::new()
            .map_err(|e| TranscriptionError::EngineFailed(e.to_string()))?;
        write_wav(file.path(), &audio).map_err(TranscriptionError::EngineFailed)?;
        let settings = read_settings(&self.app).map_err(TranscriptionError::EngineFailed)?;
        let result = request(&self.app, json!({"operation":"analyze", "audio_path":file.path(),"model":self.model,
            "diarize":false,"word_timestamps":false,"device":settings.device,"language":recognition_language(language),"offline":true}), token).await.map_err(TranscriptionError::EngineFailed)?;
        let segments: Vec<AnalyzedSegment> = serde_json::from_value(result["segments"].clone())
            .map_err(|e| TranscriptionError::EngineFailed(e.to_string()))?;
        Ok(TranscriptResult {
            text: segments
                .into_iter()
                .map(|s| s.text)
                .collect::<Vec<_>>()
                .join(" "),
            confidence: None,
            is_partial: false,
        })
    }
}
#[async_trait::async_trait]
impl<R: Runtime> crate::audio::transcription::TranscriptionProvider for SpeechProvider<R> {
    async fn transcribe(
        &self,
        audio: Vec<f32>,
        language: Option<String>,
    ) -> Result<
        crate::audio::transcription::TranscriptResult,
        crate::audio::transcription::TranscriptionError,
    > {
        self.transcribe_with_cancellation(audio, language, &CancellationToken::new())
            .await
    }
    async fn is_model_loaded(&self) -> bool {
        if WORKER.lock().await.is_none() {
            return false;
        }
        match request(
            &self.app,
            json!({"operation":"health", "model":self.model}),
            &CancellationToken::new(),
        )
        .await
        {
            Ok(value) => {
                value.get("loaded_model").and_then(Value::as_str) == Some(self.model.as_str())
            }
            Err(_) => false,
        }
    }
    async fn get_current_model(&self) -> Option<String> {
        Some(self.model.clone())
    }
    fn provider_name(&self) -> &'static str {
        "Local speech worker"
    }
}

fn write_wav(path: &Path, samples: &[f32]) -> Result<(), String> {
    use std::io::Write;
    let bytes =
        u32::try_from(samples.len().saturating_mul(2)).map_err(|_| "Audio chunk too large")?;
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut file = std::io::BufWriter::new(file);
    let mut header = Vec::with_capacity(44);
    header.extend_from_slice(b"RIFF");
    header.extend_from_slice(&(bytes + 36).to_le_bytes());
    header.extend_from_slice(b"WAVEfmt ");
    header.extend_from_slice(&16u32.to_le_bytes());
    header.extend_from_slice(&1u16.to_le_bytes());
    header.extend_from_slice(&1u16.to_le_bytes());
    header.extend_from_slice(&16000u32.to_le_bytes());
    header.extend_from_slice(&32000u32.to_le_bytes());
    header.extend_from_slice(&2u16.to_le_bytes());
    header.extend_from_slice(&16u16.to_le_bytes());
    header.extend_from_slice(b"data");
    header.extend_from_slice(&bytes.to_le_bytes());
    file.write_all(&header).map_err(|e| e.to_string())?;
    for sample in samples {
        file.write_all(&((sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16).to_le_bytes())
            .map_err(|e| e.to_string())?;
    }
    file.flush().map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    #[tokio::test]
    async fn warm_worker_survives_old_idle_timer_and_expires_after_inactivity() {
        use std::os::unix::process::CommandExt;
        let mut command = Command::new("python3");
        command.args(["-u", "-c", "import sys,json\nfor line in sys.stdin:\n r=json.loads(line);print(json.dumps({'id':r['id'],'ok':True}),flush=True)"])
            .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
        command.as_std_mut().process_group(0);
        let mut child = command.spawn().unwrap();
        let pid = child.id();
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        let mut slot = Some(Worker {
            sandbox: overtalk_worker_sandbox::Sandbox::new(false).unwrap(),
            child,
            stdin,
            stdout,
            last_activity: tokio::time::Instant::now() - ASR_IDLE_TIMEOUT,
        });
        // A new job reuses the process and invalidates an earlier idle deadline.
        let token = CancellationToken::new();
        slot.as_mut()
            .unwrap()
            .request(json!({"operation":"load"}), &token)
            .await
            .unwrap();
        let loaded_at = slot.as_ref().unwrap().last_activity;
        slot.as_mut()
            .unwrap()
            .request(json!({"operation":"health"}), &token)
            .await
            .unwrap();
        assert_eq!(slot.as_ref().unwrap().last_activity, loaded_at);
        expire_idle_worker(&mut slot, tokio::time::Instant::now()).await;
        assert_eq!(slot.as_ref().unwrap().child.id(), pid);
        slot.as_mut()
            .unwrap()
            .request(json!({"operation":"analyze"}), &token)
            .await
            .unwrap();
        let last_activity = slot.as_ref().unwrap().last_activity;
        expire_idle_worker(
            &mut slot,
            last_activity + ASR_IDLE_TIMEOUT - Duration::from_millis(1),
        )
        .await;
        assert!(slot.is_some());
        expire_idle_worker(&mut slot, last_activity + ASR_IDLE_TIMEOUT).await;
        assert!(slot.is_none());
    }

    #[test]
    fn external_python_ignores_bundle_python_paths() {
        let mut command = Command::new("python3");
        command
            .env("PYTHONHOME", "/broken/bundle")
            .env("PYTHONPATH", "/broken/modules");
        configure_python_environment(&mut command, None);
        for (key, value) in command.as_std().get_envs() {
            if key == "PYTHONHOME" || key == "PYTHONPATH" {
                assert!(value.is_none());
            }
        }
    }
    #[cfg(unix)]
    #[test]
    fn interpreter_prefix_follows_links_except_for_virtual_environments() {
        use std::os::unix::fs::symlink;
        let root = tempfile::tempdir().unwrap();
        let install = root.path().join("opt/python");
        std::fs::create_dir_all(install.join("bin")).unwrap();
        std::fs::write(install.join("bin/python3.12"), "").unwrap();
        // A link in ~/.local/bin grants the real installation, not ~/.local.
        let local = root.path().join("home/.local");
        std::fs::create_dir_all(local.join("bin")).unwrap();
        symlink(install.join("bin/python3.12"), local.join("bin/python")).unwrap();
        assert_eq!(
            interpreter_prefix(&local.join("bin/python")).unwrap(),
            Some(install.canonicalize().unwrap())
        );
        // A virtual environment keeps its packages under its own prefix.
        let venv = root.path().join("venv");
        std::fs::create_dir_all(venv.join("bin")).unwrap();
        std::fs::write(venv.join("pyvenv.cfg"), "home = /usr/bin\n").unwrap();
        symlink(install.join("bin/python3.12"), venv.join("bin/python")).unwrap();
        assert_eq!(
            interpreter_prefix(&venv.join("bin/python")).unwrap(),
            Some(venv.clone())
        );
        assert!(interpreter_prefix(&root.path().join("missing/bin/python")).is_err());
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn startup_exit_is_not_reported_as_oversized_output() {
        let mut child = Command::new("/bin/sh")
            .args(["-c", "read request; exit 7"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        let mut worker = Worker {
            sandbox: overtalk_worker_sandbox::Sandbox::new(false).unwrap(),
            child,
            stdin,
            stdout,
            last_activity: tokio::time::Instant::now(),
        };
        let error = worker
            .request(json!({"operation":"health"}), &CancellationToken::new())
            .await
            .unwrap_err();
        assert!(error.contains("7"), "{error}");
        assert!(error.contains("Python executable"), "{error}");
        assert!(!error.contains("size limit"), "{error}");
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn worker_termination_allows_supervisor_cleanup() {
        use std::os::unix::process::CommandExt;
        let directory = tempfile::tempdir().unwrap();
        let marker = directory.path().join("cleaned");
        let mut command = Command::new("/bin/sh");
        command.args(["-c","trap 'printf cleaned > \"$SONEFOLD_TEST_CLEANUP\"; exit 0' TERM; printf ready; sleep 30 & wait"])
            .env("SONEFOLD_TEST_CLEANUP",&marker).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
        command.as_std_mut().process_group(0);
        let mut child = command.spawn().unwrap();
        let stdin = child.stdin.take().unwrap();
        let mut stdout = BufReader::new(child.stdout.take().unwrap());
        let mut ready = [0u8; 5];
        stdout.read_exact(&mut ready).await.unwrap();
        let mut worker = Worker {
            sandbox: overtalk_worker_sandbox::Sandbox::new(false).unwrap(),
            child,
            stdin,
            stdout,
            last_activity: tokio::time::Instant::now(),
        };
        worker.terminate().await;
        assert_eq!(std::fs::read_to_string(marker).unwrap(), "cleaned");
        assert!(worker.child.try_wait().unwrap().is_some());
    }
    async fn recovery_database() -> sqlx::SqlitePool {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        sqlx::raw_sql(
            "INSERT INTO meetings(id,title,created_at,updated_at) \
             VALUES('one','One','2026-09-30','2026-09-30'),('two','Two','2026-09-30','2026-09-30')\
             ; INSERT INTO transcripts(id,meeting_id,transcript,timestamp,summary,speaker,\
             speaker_id,audio_start_time,audio_end_time,overlap) \
             VALUES('original','one','Original 日本語','00:00:01','Per-turn summary','mic','A',1,2,\
             1),('unrelated','two','Other meeting','00:00:00',NULL,'system',NULL,0,1,0); \
             INSERT INTO meeting_speakers VALUES('one','A','Alice'); \
             INSERT INTO speaker_turns VALUES('one','A',1,2,0.8,'test');",
        )
        .execute(&pool)
        .await
        .unwrap();
        pool
    }
    #[tokio::test]
    async fn recovery_restores_all_fields_and_speaker_names_and_can_be_undone() {
        let pool = recovery_database().await;
        let mut transaction = pool.begin().await.unwrap();
        snapshot_transcript(&mut transaction, "one").await.unwrap();
        sqlx::raw_sql("UPDATE transcripts SET transcript='New version',summary=NULL, speaker_id=NULL,overlap=0 WHERE meeting_id='one'; DELETE FROM meeting_speakers WHERE meeting_id='one';").execute(&mut *transaction).await.unwrap();
        transaction.commit().await.unwrap();
        restore_transcript(&pool, "one").await.unwrap();
        let row: (String, String, String, String, bool) = sqlx::query_as("SELECT transcript,summary,speaker,speaker_id,overlap FROM transcripts WHERE id='original'").fetch_one(&pool).await.unwrap();
        assert_eq!(
            row,
            (
                "Original 日本語".into(),
                "Per-turn summary".into(),
                "mic".into(),
                "A".into(),
                true
            )
        );
        let name: String =
            sqlx::query_scalar("SELECT name FROM meeting_speakers WHERE meeting_id='one'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(name, "Alice");
        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM speaker_turns WHERE meeting_id='one'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(count, 1);
        assert!(restore_transcript(&pool, "two").await.is_err());
        let other: String =
            sqlx::query_scalar("SELECT transcript FROM transcripts WHERE id='unrelated'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(other, "Other meeting");
        restore_transcript(&pool, "one").await.unwrap();
        let restored: String =
            sqlx::query_scalar("SELECT transcript FROM transcripts WHERE id='original'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(restored, "New version");
    }
    #[tokio::test]
    async fn corrupt_recovery_rolls_back_every_change() {
        let pool = recovery_database().await;
        sqlx::query("INSERT INTO speech_transcript_backups(meeting_id,created_at,transcripts) VALUES('one','2026-09-30','[{\"id\":\"broken\"}]')").execute(&pool).await.unwrap();
        assert!(restore_transcript(&pool, "one").await.is_err());
        let text: String =
            sqlx::query_scalar("SELECT transcript FROM transcripts WHERE id='original'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(text, "Original 日本語");
        let name: String =
            sqlx::query_scalar("SELECT name FROM meeting_speakers WHERE meeting_id='one'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(name, "Alice");
    }
    #[test]
    fn reject_invalid_timestamps_without_mutating_data() {
        let mut segment = AnalyzedSegment {
            text: "Hello".into(),
            start: 1.0,
            end: 2.0,
            speaker: None,
            speaker_confidence: None,
            overlap: false,
        };
        assert!(validate_segments(std::slice::from_ref(&segment)).is_ok());
        for end in [0.0, f64::NAN, f64::INFINITY] {
            segment.end = end;
            assert!(validate_segments(std::slice::from_ref(&segment)).is_err());
        }
        assert!(validate_segments(&[]).is_err());
    }
    #[test]
    fn wav_header_and_sample_values_are_valid_pcm() {
        let file = tempfile::NamedTempFile::new().unwrap();
        write_wav(file.path(), &[0.0, 1.0, -1.0]).unwrap();
        let bytes = std::fs::read(file.path()).unwrap();
        assert_eq!(&bytes[..4], b"RIFF");
        assert_eq!(bytes.len(), 50);
        assert_eq!(
            i16::from_le_bytes(bytes[46..48].try_into().unwrap()),
            i16::MAX
        );
    }
}

/// Translation is a Whisper task, not a language code supported by local ASR.
pub(crate) fn recognition_language(language: Option<String>) -> Option<String> {
    language.filter(|value| !matches!(value.trim(), "" | "auto" | "auto-translate"))
}

#[cfg(test)]
mod native_language_tests {
    #[test]
    fn auto_translation_does_not_become_an_asr_language_name() {
        for value in ["", "auto", "auto-translate"] {
            assert_eq!(super::recognition_language(Some(value.into())), None);
        }
        assert_eq!(
            super::recognition_language(Some("pt".into())),
            Some("pt".into())
        );
    }
}
