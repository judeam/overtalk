mod download;

use crate::parakeet_engine::model::ParakeetModel;
use anyhow::{anyhow, Result};
use ring::digest;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::fs;
use tokio::io::{AsyncReadExt, BufReader};
use tokio::sync::{watch, Mutex, RwLock};
use tokio_util::sync::CancellationToken;

/// Quantization type for Parakeet models
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Default)]
pub enum QuantizationType {
    FP32, // Full precision
    #[default]
    Int8, // 8-bit integer quantization (faster)
}

/// Model status for Parakeet models
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ModelStatus {
    Available,
    Missing,
    Downloading {
        progress: u8,
    },
    Error(String),
    Corrupted {
        file_size: u64,
        expected_min_size: u64,
    },
}

/// Detailed download progress info (MB-based with speed)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadProgress {
    /// Bytes downloaded so far
    pub downloaded_bytes: u64,
    /// Total file size in bytes
    pub total_bytes: u64,
    /// Downloaded in MB (for display)
    pub downloaded_mb: f64,
    /// Total size in MB (for display)
    pub total_mb: f64,
    /// Download speed in MB/s
    pub speed_mbps: f64,
    /// Percentage complete (0-100)
    pub percent: u8,
}

impl DownloadProgress {
    pub fn new(downloaded: u64, total: u64, speed_mbps: f64) -> Self {
        let percent = if total > 0 {
            ((downloaded as f64 / total as f64) * 100.0).min(100.0) as u8
        } else {
            0
        };
        Self {
            downloaded_bytes: downloaded,
            total_bytes: total,
            downloaded_mb: downloaded as f64 / (1024.0 * 1024.0),
            total_mb: total as f64 / (1024.0 * 1024.0),
            speed_mbps,
            percent,
        }
    }
}

/// Information about a Parakeet model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub name: String,
    pub path: PathBuf,
    pub size_mb: u32,
    pub quantization: QuantizationType,
    pub speed: String, // Performance description
    pub status: ModelStatus,
    pub description: String,
}

#[derive(Clone, Copy)]
struct ArtifactSpec {
    filename: &'static str,
    exact_bytes: u64,
    /// Optional pinned content digest. Routine discovery remains size-only.
    sha256: Option<&'static str>,
}

struct ModelSpec {
    name: &'static str,
    size_mb: u32,
    quantization: QuantizationType,
    speed: &'static str,
    description: &'static str,
    source_base_url: &'static str,
    artifacts: &'static [ArtifactSpec],
}

impl ModelSpec {
    fn exact_bytes(&self) -> u64 {
        self.artifacts
            .iter()
            .map(|artifact| artifact.exact_bytes)
            .sum()
    }
}

const PARAKEET_V3_ARTIFACTS: &[ArtifactSpec] = &[
    ArtifactSpec {
        filename: "encoder-model.int8.onnx",
        exact_bytes: 652_183_999,
        sha256: Some("6139d2fa7e1b086097b277c7149725edbab89cc7c7ae64b23c741be4055aff09"),
    },
    ArtifactSpec {
        filename: "decoder_joint-model.int8.onnx",
        exact_bytes: 18_202_004,
        sha256: Some("eea7483ee3d1a30375daedc8ed83e3960c91b098812127a0d99d1c8977667a70"),
    },
    ArtifactSpec {
        filename: "nemo128.onnx",
        exact_bytes: 139_764,
        sha256: Some("a9fde1486ebfcc08f328d75ad4610c67835fea58c73ba57e3209a6f6cf019e9f"),
    },
    ArtifactSpec {
        filename: "vocab.txt",
        exact_bytes: 93_939,
        sha256: Some("d58544679ea4bc6ac563d1f545eb7d474bd6cfa467f0a6e2c1dc1c7d37e3c35d"),
    },
];

const PARAKEET_V2_ARTIFACTS: &[ArtifactSpec] = &[
    ArtifactSpec {
        filename: "encoder-model.int8.onnx",
        exact_bytes: 652_184_014,
        sha256: None,
    },
    ArtifactSpec {
        filename: "decoder_joint-model.int8.onnx",
        exact_bytes: 8_998_286,
        sha256: None,
    },
    ArtifactSpec {
        filename: "nemo128.onnx",
        exact_bytes: 139_764,
        sha256: None,
    },
    ArtifactSpec {
        filename: "vocab.txt",
        exact_bytes: 9_384,
        sha256: None,
    },
];

const PARAKEET_MODEL_SPECS: &[ModelSpec] = &[
    ModelSpec {
        name: "parakeet-tdt-0.6b-v3-int8",
        size_mb: 670,
        quantization: QuantizationType::Int8,
        speed: "Ultra Fast (v3)",
        description: "25 European languages with automatic language detection; CPU-optimized INT8",
        source_base_url: "https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx/resolve/8f23f0c03c8761650bdb5b40aaf3e40d2c15f1ce",
        artifacts: PARAKEET_V3_ARTIFACTS,
    },
    ModelSpec {
        name: "parakeet-tdt-0.6b-v2-int8",
        size_mb: 661,
        quantization: QuantizationType::Int8,
        speed: "Fast (v2)",
        description: "English-only Parakeet v2; CPU-optimized INT8",
        source_base_url: "https://huggingface.co/istupakov/parakeet-tdt-0.6b-v2-onnx/resolve/0bbb45a3365852604aef28b538a8f066f4ccaa85",
        artifacts: PARAKEET_V2_ARTIFACTS,
    },
];

fn find_model_spec(model_name: &str) -> Option<&'static ModelSpec> {
    PARAKEET_MODEL_SPECS
        .iter()
        .find(|spec| spec.name == model_name)
}

struct ActiveDownload {
    cancellation: CancellationToken,
    completion: watch::Sender<bool>,
}

#[derive(Default)]
struct ActiveDownloadState {
    downloads: HashMap<String, Arc<ActiveDownload>>,
    revision: u64,
}

#[derive(Debug, thiserror::Error)]
#[error("Download cancelled by user")]
pub(crate) struct DownloadCancelled;

pub(crate) fn is_download_cancelled(error: &anyhow::Error) -> bool {
    error.downcast_ref::<DownloadCancelled>().is_some()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CancelDownloadOutcome {
    Cancelled,
    Pending,
}

const CANCEL_DOWNLOAD_CLEANUP_TIMEOUT: Duration = Duration::from_secs(5);

#[cfg(test)]
struct DownloadStateTestHook {
    finalization_ready: tokio::sync::Notify,
    continue_finalization: tokio::sync::Notify,
    discovery_scanned: tokio::sync::Notify,
    continue_discovery: tokio::sync::Notify,
}

#[cfg(test)]
struct ModelLifecycleTestHook {
    load_started: tokio::sync::Notify,
    continue_load: tokio::sync::Notify,
    unload_attempted: tokio::sync::Notify,
}

enum ContentRange {
    Range { start: u64, end: u64, total: u64 },
    Unsatisfied { total: u64 },
}

fn parse_content_range(value: &reqwest::header::HeaderValue) -> Result<ContentRange> {
    let value = value
        .to_str()
        .map_err(|e| anyhow!("Invalid Content-Range header encoding: {}", e))?;
    let value = value
        .strip_prefix("bytes ")
        .ok_or_else(|| anyhow!("Content-Range must use bytes: {}", value))?;

    if let Some(total) = value.strip_prefix("*/") {
        return total
            .parse()
            .map(|total| ContentRange::Unsatisfied { total })
            .map_err(|e| anyhow!("Invalid unsatisfied Content-Range total: {}", e));
    }

    let (range, total) = value
        .split_once('/')
        .ok_or_else(|| anyhow!("Malformed Content-Range: {}", value))?;
    let (start, end) = range
        .split_once('-')
        .ok_or_else(|| anyhow!("Malformed Content-Range range: {}", value))?;
    let start = start
        .parse()
        .map_err(|e| anyhow!("Invalid Content-Range start: {}", e))?;
    let end = end
        .parse()
        .map_err(|e| anyhow!("Invalid Content-Range end: {}", e))?;
    let total = total
        .parse()
        .map_err(|e| anyhow!("Invalid Content-Range total: {}", e))?;
    if start > end {
        return Err(anyhow!("Content-Range start exceeds end: {}", value));
    }

    Ok(ContentRange::Range { start, end, total })
}

#[derive(Debug)]
pub enum ParakeetEngineError {
    ModelNotLoaded,
    ModelNotFound(String),
    TranscriptionFailed(String),
    DownloadFailed(String),
    IoError(std::io::Error),
    Other(String),
}

impl std::fmt::Display for ParakeetEngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParakeetEngineError::ModelNotLoaded => write!(f, "No Parakeet model loaded"),
            ParakeetEngineError::ModelNotFound(name) => write!(f, "Model '{}' not found", name),
            ParakeetEngineError::TranscriptionFailed(err) => {
                write!(f, "Transcription failed: {}", err)
            }
            ParakeetEngineError::DownloadFailed(err) => write!(f, "Download failed: {}", err),
            ParakeetEngineError::IoError(err) => write!(f, "IO error: {}", err),
            ParakeetEngineError::Other(err) => write!(f, "Error: {}", err),
        }
    }
}

impl std::error::Error for ParakeetEngineError {}

impl From<std::io::Error> for ParakeetEngineError {
    fn from(err: std::io::Error) -> Self {
        ParakeetEngineError::IoError(err)
    }
}

pub struct ParakeetEngine {
    models_dir: PathBuf,
    isolated: crate::native_model_worker::Shared,
    current_model: Arc<RwLock<Option<ParakeetModel>>>,
    current_model_name: Arc<RwLock<Option<String>>>,
    model_lifecycle_lock: Mutex<()>,
    pub(crate) available_models: Arc<RwLock<HashMap<String, ModelInfo>>>,
    active_downloads: Arc<Mutex<ActiveDownloadState>>,
    #[cfg(test)]
    download_state_test_hook: Mutex<Option<Arc<DownloadStateTestHook>>>,
    #[cfg(test)]
    model_lifecycle_test_hook: Mutex<Option<Arc<ModelLifecycleTestHook>>>,
}

impl ParakeetEngine {
    /// Create a new Parakeet engine with optional custom models directory
    pub fn new_with_models_dir(models_dir: Option<PathBuf>) -> Result<Self> {
        let models_dir = if let Some(dir) = models_dir {
            dir.join("parakeet") // Parakeet models in subdirectory
        } else {
            // Fallback to default location
            let current_dir = std::env::current_dir()
                .map_err(|e| anyhow!("Failed to get current directory: {}", e))?;

            if cfg!(debug_assertions) {
                // Development mode
                current_dir.join("models").join("parakeet")
            } else {
                // Production mode
                dirs::data_dir()
                    .or_else(dirs::home_dir)
                    .ok_or_else(|| anyhow!("Could not find system data directory"))?
                    .join("Sonefold")
                    .join("models")
                    .join("parakeet")
            }
        };

        log::info!(
            "ParakeetEngine using models directory: {}",
            models_dir.display()
        );

        // Create directory if it doesn't exist
        if !models_dir.exists() {
            std::fs::create_dir_all(&models_dir)?;
        }

        Ok(Self {
            models_dir,
            current_model: Arc::new(RwLock::new(None)),
            isolated: crate::native_model_worker::shared(),
            current_model_name: Arc::new(RwLock::new(None)),
            model_lifecycle_lock: Mutex::new(()),
            available_models: Arc::new(RwLock::new(HashMap::new())),
            active_downloads: Arc::new(Mutex::new(ActiveDownloadState::default())),
            #[cfg(test)]
            download_state_test_hook: Mutex::new(None),
            #[cfg(test)]
            model_lifecycle_test_hook: Mutex::new(None),
        })
    }

    #[cfg(test)]
    async fn test_hook(&self) -> Option<Arc<DownloadStateTestHook>> {
        self.download_state_test_hook.lock().await.clone()
    }

    #[cfg(test)]
    async fn load_test_hook(&self) -> Option<Arc<ModelLifecycleTestHook>> {
        self.model_lifecycle_test_hook.lock().await.clone()
    }

    /// Discover available Parakeet models.
    pub async fn discover_models(&self) -> Result<Vec<ModelInfo>> {
        self.discover_models_from_specs(PARAKEET_MODEL_SPECS).await
    }

    async fn discover_models_from_specs(&self, specs: &[ModelSpec]) -> Result<Vec<ModelInfo>> {
        loop {
            let revision = self.active_downloads.lock().await.revision;
            let mut models = Vec::with_capacity(specs.len());
            let mut validation_errors = Vec::new();

            for spec in specs {
                let model_path = self.models_dir.join(spec.name);
                let status = if model_path.exists() {
                    match Self::validate_model_directory(&model_path, spec.artifacts) {
                        Ok(()) => ModelStatus::Available,
                        Err(error) => {
                            let file_size = spec
                                .artifacts
                                .iter()
                                .filter_map(|artifact| {
                                    std::fs::metadata(model_path.join(artifact.filename)).ok()
                                })
                                .map(|metadata| metadata.len())
                                .sum();
                            validation_errors.push((spec.name, error));
                            ModelStatus::Corrupted {
                                file_size,
                                expected_min_size: spec.exact_bytes(),
                            }
                        }
                    }
                } else {
                    ModelStatus::Missing
                };

                models.push(ModelInfo {
                    name: spec.name.to_string(),
                    path: model_path,
                    size_mb: spec.size_mb,
                    quantization: spec.quantization,
                    speed: spec.speed.to_string(),
                    status,
                    description: spec.description.to_string(),
                });
            }

            #[cfg(test)]
            if let Some(hook) = self.test_hook().await {
                hook.discovery_scanned.notify_one();
                hook.continue_discovery.notified().await;
            }

            let active_downloads = self.active_downloads.lock().await;
            if active_downloads.revision != revision {
                continue;
            }

            validation_errors
                .retain(|(model_name, _)| !active_downloads.downloads.contains_key(*model_name));
            for model in &mut models {
                if active_downloads.downloads.contains_key(&model.name) {
                    model.status = ModelStatus::Downloading { progress: 0 };
                }
            }

            let mut available_models = self.available_models.write().await;
            available_models.clear();
            for model in &models {
                available_models.insert(model.name.clone(), model.clone());
            }
            drop(available_models);
            drop(active_downloads);

            for (model_name, error) in validation_errors {
                log::warn!(
                    "Model directory {} appears corrupted: {}",
                    model_name,
                    error
                );
            }
            return Ok(models);
        }
    }

    fn validate_model_directory(model_dir: &Path, artifacts: &[ArtifactSpec]) -> Result<()> {
        for artifact in artifacts {
            let path = model_dir.join(artifact.filename);
            let metadata = std::fs::metadata(&path).map_err(|error| {
                anyhow!("Failed to read {} metadata: {}", artifact.filename, error)
            })?;
            if metadata.len() != artifact.exact_bytes {
                return Err(anyhow!(
                    "{} has {} bytes, expected exactly {} bytes",
                    artifact.filename,
                    metadata.len(),
                    artifact.exact_bytes
                ));
            }
        }

        Ok(())
    }

    async fn hash_local_artifact(
        file_path: &Path,
        expected_bytes: u64,
        context: &mut digest::Context,
        cancellation: &CancellationToken,
    ) -> Result<()> {
        let file = fs::File::open(file_path).await.map_err(|error| {
            anyhow!(
                "Failed to open artifact for integrity verification: {}",
                error
            )
        })?;
        let mut reader = BufReader::with_capacity(256 * 1024, file);
        let mut buffer = vec![0u8; 256 * 1024];
        let mut bytes = 0u64;
        loop {
            let count = tokio::select! {
                biased;
                _ = cancellation.cancelled() => return Err(DownloadCancelled.into()),
                count = reader.read(&mut buffer) => count
                    .map_err(|error| anyhow!("Failed to read artifact for integrity verification: {}", error))?,
            };
            if count == 0 {
                break;
            }
            bytes = bytes
                .checked_add(count as u64)
                .ok_or_else(|| anyhow!("Artifact size overflow during integrity verification"))?;
            if bytes > expected_bytes {
                return Err(anyhow!(
                    "Artifact changed size during integrity verification"
                ));
            }
            context.update(&buffer[..count]);
        }
        if bytes != expected_bytes {
            return Err(anyhow!(
                "Artifact changed size during integrity verification"
            ));
        }
        Ok(())
    }

    async fn verify_artifact_digest(
        file_path: &Path,
        artifact: &ArtifactSpec,
        context: Option<digest::Context>,
    ) -> Result<()> {
        let Some(expected) = artifact.sha256 else {
            return Ok(());
        };
        let context =
            context.ok_or_else(|| anyhow!("Missing integrity digest for {}", artifact.filename))?;
        let actual: String = context
            .finish()
            .as_ref()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        if actual != expected {
            if let Err(error) = fs::remove_file(file_path).await {
                if error.kind() != std::io::ErrorKind::NotFound {
                    return Err(anyhow!(
                        "SHA-256 mismatch for {}; failed to remove invalid artifact: {}",
                        artifact.filename,
                        error
                    ));
                }
            }
            return Err(anyhow!(
                "SHA-256 mismatch for {}: expected {}, received {}; invalid artifact removed",
                artifact.filename,
                expected,
                actual
            ));
        }
        Ok(())
    }

    /// Load a Parakeet model
    pub async fn load_model(&self, model_name: &str) -> Result<()> {
        let model_info = {
            let models = self.available_models.read().await;
            models
                .get(model_name)
                .cloned()
                .ok_or_else(|| anyhow!("Model {} not found", model_name))?
        };

        match &model_info.status {
            ModelStatus::Available => {
                let _lifecycle_guard = self.model_lifecycle_lock.lock().await;
                let current_model = self.current_model_name.read().await.clone();
                if current_model.as_deref() == Some(model_name)
                    && (crate::native_model_worker::is_worker()
                        || cfg!(test)
                        || crate::native_model_worker::loaded(&self.isolated))
                {
                    log::info!(
                        "Parakeet model {} is already loaded, skipping reload",
                        model_name
                    );
                    return Ok(());
                }

                if let Some(current_model) = current_model {
                    log::info!(
                        "Unloading current Parakeet model '{}' before loading '{}'",
                        current_model,
                        model_name
                    );
                }
                self.unload_model_locked().await;

                if !crate::native_model_worker::is_worker() && !cfg!(test) {
                    crate::native_model_worker::load_shared(
                        self.isolated.clone(),
                        "parakeet",
                        self.models_dir.clone(),
                        model_name.to_owned(),
                        false,
                    )
                    .await?;
                    *self.current_model_name.write().await = Some(model_name.to_owned());
                    return Ok(());
                }
                log::info!("Loading Parakeet model: {}", model_name);

                let quantized = model_info.quantization == QuantizationType::Int8;
                let model_path = model_info.path.clone();
                #[cfg(test)]
                let model_lifecycle_test_hook = self.load_test_hook().await;
                #[cfg(test)]
                let runtime_handle = tokio::runtime::Handle::current();
                let model = tokio::task::spawn_blocking(move || {
                    #[cfg(test)]
                    if let Some(hook) = model_lifecycle_test_hook {
                        hook.load_started.notify_one();
                        runtime_handle.block_on(hook.continue_load.notified());
                    }
                    ParakeetModel::new(&model_path, quantized).map_err(|error| error.to_string())
                })
                .await
                .map_err(|error| {
                    anyhow!(
                        "Parakeet model load task failed for {}: {}",
                        model_name,
                        error
                    )
                })?
                .map_err(|error| {
                    anyhow!("Failed to load Parakeet model {}: {}", model_name, error)
                })?;

                *self.current_model.write().await = Some(model);
                *self.current_model_name.write().await = Some(model_name.to_string());

                log::info!(
                    "Successfully loaded Parakeet model: {} ({})",
                    model_name,
                    if quantized { "Int8 quantized" } else { "FP32" }
                );
                Ok(())
            }
            ModelStatus::Missing => Err(anyhow!("Parakeet model {} is not downloaded", model_name)),
            ModelStatus::Downloading { .. } => Err(anyhow!(
                "Parakeet model {} is currently downloading",
                model_name
            )),
            ModelStatus::Error(err) => {
                Err(anyhow!("Parakeet model {} has error: {}", model_name, err))
            }
            ModelStatus::Corrupted { .. } => Err(anyhow!(
                "Parakeet model {} is corrupted and cannot be loaded",
                model_name
            )),
        }
    }

    /// Unload the current model
    pub async fn unload_model(&self) -> bool {
        #[cfg(test)]
        if let Some(hook) = self.load_test_hook().await {
            hook.unload_attempted.notify_one();
        }
        let _lifecycle_guard = self.model_lifecycle_lock.lock().await;
        self.unload_model_locked().await
    }

    async fn unload_model_locked(&self) -> bool {
        let isolated = crate::native_model_worker::unload_shared(self.isolated.clone()).await;
        let unloaded = self.current_model.write().await.take().is_some();
        if unloaded {
            log::info!("Parakeet model unloaded");
        }
        self.current_model_name.write().await.take();
        unloaded || isolated
    }

    /// Get the currently loaded model name
    pub async fn get_current_model(&self) -> Option<String> {
        self.current_model_name.read().await.clone()
    }

    /// Check if a model is loaded
    pub async fn is_model_loaded(&self) -> bool {
        crate::native_model_worker::loaded(&self.isolated)
            || self.current_model.read().await.is_some()
    }

    /// Transcribe audio samples using the loaded Parakeet model
    pub async fn transcribe_audio(&self, audio_data: Vec<f32>) -> Result<String> {
        Ok(self.transcribe_audio_timestamped(audio_data).await?.text)
    }

    /// Preserve native TDT token times; perform CPU inference away from async IO/capture tasks.
    pub async fn transcribe_audio_timestamped(
        &self,
        audio_data: Vec<f32>,
    ) -> Result<crate::parakeet_engine::model::TimestampedResult> {
        if !crate::native_model_worker::is_worker() && !cfg!(test) {
            return crate::native_model_worker::call_shared(
                self.isolated.clone(),
                "transcribe",
                audio_data,
                None,
            )
            .await;
        }
        let current_model = self.current_model.clone();
        tokio::task::spawn_blocking(move || {
            let mut guard = current_model.blocking_write();
            let model = guard
                .as_mut()
                .ok_or_else(|| anyhow!("No Parakeet model loaded. Please load a model first."))?;
            model
                .transcribe_samples(audio_data)
                .map_err(|e| anyhow!("Parakeet transcription failed: {e}"))
        })
        .await
        .map_err(|e| anyhow!("Parakeet inference task failed: {e}"))?
    }

    /// Get the models directory path
    pub async fn get_models_directory(&self) -> PathBuf {
        self.models_dir.clone()
    }

    /// Delete a corrupted model
    pub async fn delete_model(&self, model_name: &str) -> Result<String> {
        log::info!("Attempting to delete Parakeet model: {}", model_name);

        // Get model info to find the directory path
        let model_info = {
            let models = self.available_models.read().await;
            models.get(model_name).cloned()
        };

        let model_info =
            model_info.ok_or_else(|| anyhow!("Parakeet model '{}' not found", model_name))?;

        log::info!(
            "Parakeet model '{}' has status: {:?}",
            model_name,
            model_info.status
        );

        // Allow deletion of corrupted or available models
        match &model_info.status {
            ModelStatus::Corrupted { .. } | ModelStatus::Available => {
                // Delete the entire model directory
                if model_info.path.exists() {
                    fs::remove_dir_all(&model_info.path).await
                        .map_err(|e| anyhow!("Failed to delete directory '{}': {}", model_info.path.display(), e))?;
                    log::info!("Successfully deleted Parakeet model directory: {}", model_info.path.display());
                } else {
                    log::warn!("Directory '{}' does not exist, nothing to delete", model_info.path.display());
                }

                // Update model status to Missing
                {
                    let mut models = self.available_models.write().await;
                    if let Some(model) = models.get_mut(model_name) {
                        model.status = ModelStatus::Missing;
                    }
                }

                Ok(format!("Successfully deleted Parakeet model '{}'", model_name))
            }
            _ => {
                Err(anyhow!(
                    "Can only delete corrupted or available Parakeet models. Model '{}' has status: {:?}",
                    model_name,
                    model_info.status
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests;
