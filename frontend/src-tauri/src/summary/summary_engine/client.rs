// High-level client API for built-in AI summary generation
// Provides simple interface for generating text using the sidecar

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::sync::RwLock;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use super::models;
use super::sidecar::SidecarManager;

// ============================================================================
// Request/Response Types
// ============================================================================

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Request {
    Generate {
        prompt: String,
        max_tokens: Option<i32>,
        context_size: Option<u32>,
        model_path: Option<String>,
        // Sampling parameters
        temperature: Option<f32>,
        top_k: Option<i32>,
        top_p: Option<f32>,
        presence_penalty: Option<f32>,
        frequency_penalty: Option<f32>,
        repeat_penalty: Option<f32>,
        penalty_last_n: Option<i32>,
        stop_tokens: Option<Vec<String>>,
        stop_on_repeated_line: bool,
    },
    GenerateBatch {
        prompts: Vec<String>,
        max_tokens: Option<i32>,
        context_size: Option<u32>,
        model_path: Option<String>,
        temperature: Option<f32>,
        top_k: Option<i32>,
        top_p: Option<f32>,
        presence_penalty: Option<f32>,
        frequency_penalty: Option<f32>,
        repeat_penalty: Option<f32>,
        penalty_last_n: Option<i32>,
        stop_tokens: Option<Vec<String>>,
        stop_on_repeated_line: bool,
    },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Response {
    #[serde(rename = "response")]
    Completion {
        text: String,
        error: Option<String>,
    },
    #[serde(rename = "batch_response")]
    BatchCompletion {
        texts: Vec<String>,
        error: Option<String>,
    },
    Error {
        message: String,
    },
}

// ============================================================================
// Global Sidecar Manager
// ============================================================================

lazy_static::lazy_static! {
    static ref SIDECAR_MANAGER: Arc<Mutex<Option<Arc<SidecarManager>>>> = Arc::new(Mutex::new(None));
}

// Model path cache to avoid repeated filesystem I/O and model lookups
static MODEL_PATH_CACHE: Lazy<RwLock<HashMap<String, PathBuf>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

/// Get cached model path with read-through caching to avoid repeated filesystem I/O
fn get_cached_model_path(app_data_dir: &std::path::Path, model_name: &str) -> Result<PathBuf> {
    // Try read lock first (fast path for cache hits)
    {
        let cache = MODEL_PATH_CACHE.read().unwrap();
        if let Some(path) = cache.get(model_name) {
            // Verify file still exists before returning cached path
            if path.exists() {
                return Ok(path.clone());
            }
        }
    }

    // Cache miss or file deleted - acquire write lock and update cache
    let mut cache = MODEL_PATH_CACHE.write().unwrap();

    // Double-check after acquiring write lock (another thread may have updated it)
    if let Some(path) = cache.get(model_name) {
        if path.exists() {
            return Ok(path.clone());
        }
    }

    // Resolve model path (involves model lookup + filesystem operations)
    let model_path = models::get_model_path(app_data_dir, model_name)?;

    if !model_path.exists() {
        return Err(anyhow!(
            "Model file not found: {}. Please download the model '{}' first.",
            model_path.display(),
            model_name
        ));
    }

    // Cache the validated path
    cache.insert(model_name.to_string(), model_path.clone());
    Ok(model_path)
}

// ============================================================================
// Public API
// ============================================================================

/// Generate text using built-in AI
///
/// # Arguments
/// * `app_data_dir` - Application data directory (for model resolution)
/// * `model_name` - Model name (e.g., "gemma3:1b")
/// * `system_prompt` - System instructions for the model
/// * `user_prompt` - User message/task
/// * `cancellation_token` - Optional token for cancellation
///
/// # Returns
/// Generated text
pub async fn generate_with_builtin(
    app_data_dir: &std::path::Path,
    model_name: &str,
    system_prompt: &str,
    user_prompt: &str,
    cancellation_token: Option<&CancellationToken>,
) -> Result<String> {
    generate_with_builtin_options(
        app_data_dir,
        model_name,
        system_prompt,
        user_prompt,
        crate::summary::summary_engine::client::BuiltinOptions {
            sampling: None,
            max_tokens: None,
            stop_on_repeated_line: false,
        },
        cancellation_token,
    )
    .await
}

/// Sampling and output policy for one local generation or a batch.
#[derive(Clone, Copy, Default)]
pub struct BuiltinOptions<'a> {
    pub sampling: Option<&'a models::SamplingParams>,
    pub max_tokens: Option<i32>,
    pub stop_on_repeated_line: bool,
}

/// Like `generate_with_builtin`, with sampling and an output cap chosen by the caller instead of
/// the model's defaults, e.g. near-greedy extraction. `stop_on_repeated_line` ends a looping
/// model's output at the first repeated line.
pub async fn generate_with_builtin_options(
    app_data_dir: &std::path::Path,
    model_name: &str,
    system_prompt: &str,
    user_prompt: &str,
    options: BuiltinOptions<'_>,
    cancellation_token: Option<&CancellationToken>,
) -> Result<String> {
    let BuiltinOptions {
        sampling,
        max_tokens,
        stop_on_repeated_line,
    } = options;
    let mut texts = run_builtin(
        app_data_dir,
        model_name,
        system_prompt,
        &[user_prompt],
        crate::summary::summary_engine::client::BuiltinOptions {
            sampling,
            max_tokens,
            stop_on_repeated_line,
        },
        cancellation_token,
    )
    .await?;
    texts
        .pop()
        .ok_or_else(|| anyhow!("The model returned no text"))
}

/// Several user prompts with the same system prompt, generated together as parallel sequences
/// in one context (much faster than one request each). The caller keeps their total within the
/// model's context: each prompt gets an equal share of it.
pub async fn generate_batch_with_builtin(
    app_data_dir: &std::path::Path,
    model_name: &str,
    system_prompt: &str,
    user_prompts: &[&str],
    options: BuiltinOptions<'_>,
    cancellation_token: Option<&CancellationToken>,
) -> Result<Vec<String>> {
    let BuiltinOptions {
        sampling,
        max_tokens,
        stop_on_repeated_line,
    } = options;
    run_builtin(
        app_data_dir,
        model_name,
        system_prompt,
        user_prompts,
        crate::summary::summary_engine::client::BuiltinOptions {
            sampling,
            max_tokens,
            stop_on_repeated_line,
        },
        cancellation_token,
    )
    .await
}

async fn run_builtin(
    app_data_dir: &std::path::Path,
    model_name: &str,
    system_prompt: &str,
    user_prompts: &[&str],
    options: BuiltinOptions<'_>,
    cancellation_token: Option<&CancellationToken>,
) -> Result<Vec<String>> {
    let BuiltinOptions {
        sampling,
        max_tokens,
        stop_on_repeated_line,
    } = options;
    let local_token = CancellationToken::new();
    let token = cancellation_token.unwrap_or(&local_token);
    let _lifecycle = crate::audio::common::acquire_engine_lifecycle_lock().await;
    let _budget = crate::extensions::model_budget::acquire(token)
        .await
        .map_err(anyhow::Error::msg)?;
    if crate::audio::recording_commands::is_recording().await {
        return Err(anyhow!(
            "Stop recording before running local generation; recording has priority"
        ));
    }
    crate::speech::shutdown_knowledge().await;
    crate::speech::release_idle_asr().await;
    // Check cancellation at start
    if let Some(token) = cancellation_token {
        if token.is_cancelled() {
            return Err(anyhow!("Generation cancelled before starting"));
        }
    }

    log::info!("Built-in AI generation request");
    log::info!("Model: {}", model_name);

    // Get model definition
    let model_def = models::get_model_by_name(model_name)
        .ok_or_else(|| anyhow!("Unknown model: {}", model_name))?;

    // Resolve model path with caching (avoids repeated filesystem I/O)
    let model_path = get_cached_model_path(app_data_dir, model_name)?;

    // Apply model-specific chat template
    let formatted_prompts = user_prompts
        .iter()
        .map(|user_prompt| models::format_prompt(&model_def.template, system_prompt, user_prompt))
        .collect::<Result<Vec<_>>>()?;
    // Get or initialize sidecar manager
    let manager = {
        let mut global_manager = SIDECAR_MANAGER.lock().await;
        if global_manager.is_none() {
            log::info!("Initializing sidecar manager");
            let new_manager = SidecarManager::new(app_data_dir.to_path_buf())?;
            *global_manager = Some(Arc::new(new_manager));
        }
        global_manager.clone().unwrap()
    };

    // Ensure sidecar is running with this model
    manager.ensure_running(model_path.clone()).await?;

    // Check cancellation after sidecar startup
    if let Some(token) = cancellation_token {
        if token.is_cancelled() {
            return Err(anyhow!("Generation cancelled during sidecar startup"));
        }
    }

    // Prepare generation request with model-specific sampling parameters
    let mut sampling = sampling
        .unwrap_or(&model_def.sampling)
        .sanitize_for_llama_helper();
    if sampling.stop_tokens.is_empty() {
        sampling.stop_tokens = model_def.sampling.stop_tokens.clone();
    }
    let max_tokens = Some(max_tokens.unwrap_or(models::DEFAULT_MAX_TOKENS));
    let model_path_text = Some(model_path.to_string_lossy().to_string());
    let request = if formatted_prompts.len() == 1 {
        Request::Generate {
            prompt: formatted_prompts.into_iter().next().unwrap_or_default(),
            max_tokens,
            context_size: Some(model_def.context_size),
            model_path: model_path_text,
            temperature: Some(sampling.temperature),
            top_k: Some(sampling.top_k),
            top_p: Some(sampling.top_p),
            presence_penalty: Some(sampling.presence_penalty),
            frequency_penalty: Some(sampling.frequency_penalty),
            repeat_penalty: Some(sampling.repeat_penalty),
            penalty_last_n: Some(sampling.penalty_last_n),
            stop_tokens: Some(sampling.stop_tokens),
            stop_on_repeated_line,
        }
    } else {
        Request::GenerateBatch {
            prompts: formatted_prompts,
            max_tokens,
            context_size: Some(model_def.context_size),
            model_path: model_path_text,
            temperature: Some(sampling.temperature),
            top_k: Some(sampling.top_k),
            top_p: Some(sampling.top_p),
            presence_penalty: Some(sampling.presence_penalty),
            frequency_penalty: Some(sampling.frequency_penalty),
            repeat_penalty: Some(sampling.repeat_penalty),
            penalty_last_n: Some(sampling.penalty_last_n),
            stop_tokens: Some(sampling.stop_tokens),
            stop_on_repeated_line,
        }
    };

    let request_json = serde_json::to_string(&request)?;

    // Send request with timeout
    let timeout = Duration::from_secs(models::GENERATION_TIMEOUT_SECS);

    log::info!("Sending generation request to sidecar");

    // Race between send_request and cancellation token
    let response_json = if let Some(token) = cancellation_token {
        tokio::select! {
            result = manager.send_request(request_json, timeout) => {
                result?
            }
            _ = token.cancelled() => {
                log::warn!("Generation cancelled by user, shutting down sidecar");
                // Shutdown sidecar to stop generation immediately
                if let Err(e) = manager.shutdown().await {
                    log::error!("Failed to shutdown sidecar during cancellation: {}", e);
                }
                return Err(anyhow!("Generation cancelled by user"));
            }
        }
    } else {
        manager.send_request(request_json, timeout).await?
    };

    // Check cancellation before parsing response
    if let Some(token) = cancellation_token {
        if token.is_cancelled() {
            return Err(anyhow!("Generation cancelled"));
        }
    }

    // Parse response
    let response: Response = serde_json::from_str(&response_json)
        .with_context(|| format!("Failed to parse response: {}", response_json))?;

    match response {
        Response::Completion {
            error: Some(err_msg),
            ..
        }
        | Response::BatchCompletion {
            error: Some(err_msg),
            ..
        } => Err(anyhow!("Generation failed: {}", err_msg)),
        Response::Completion { text, error: None } => {
            log::info!("Generation completed: {} chars", text.len());
            Ok(vec![text])
        }
        Response::BatchCompletion { texts, error: None } => {
            log::info!("Batch generation completed: {} texts", texts.len());
            Ok(texts)
        }
        Response::Error { message } => Err(anyhow!("Sidecar error: {}", message)),
    }
}

/// Shutdown the global sidecar (graceful cleanup)
/// Detaches the current manager and spawns a background task to drain active requests
pub async fn shutdown_sidecar_gracefully() -> Result<()> {
    let manager_opt = {
        let mut global_manager = SIDECAR_MANAGER.lock().await;
        global_manager.take()
    };

    if let Some(manager) = manager_opt {
        log::info!("Detaching sidecar manager for graceful shutdown");

        // Spawn background task to wait for active requests and then kill
        tokio::spawn(async move {
            if let Err(e) = manager.shutdown_gracefully().await {
                log::error!("Error during graceful shutdown: {}", e);
            }
        });
    }

    Ok(())
}

/// Force shutdown the global sidecar (for app exit)
/// Directly kills the process without waiting for active requests to complete.
/// This is synchronous and blocks until the sidecar is terminated.
pub async fn force_shutdown_sidecar() -> Result<()> {
    let manager_opt = {
        let mut global_manager = SIDECAR_MANAGER.lock().await;
        global_manager.take()
    };

    if let Some(manager) = manager_opt {
        log::info!("Force shutting down sidecar for app exit");
        // Call shutdown() directly - sends shutdown command and force kills after 3s
        manager.shutdown().await?;
    }

    Ok(())
}

// ============================================================================
// Tests
// ============================================================================

/// Called while holding the shared inference lease, so no generation can be killed.
pub(crate) async fn release_idle_model() -> Result<()> {
    if let Some(manager) = SIDECAR_MANAGER.lock().await.as_ref() {
        manager.shutdown().await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_serialization() {
        let request = Request::Generate {
            prompt: "test prompt".to_string(),
            max_tokens: Some(512),
            context_size: Some(2048),
            model_path: Some("/path/to/model.gguf".to_string()),
            temperature: Some(1.0),
            top_k: Some(64),
            top_p: Some(0.95),
            presence_penalty: Some(0.3),
            frequency_penalty: Some(0.0),
            repeat_penalty: Some(1.05),
            penalty_last_n: Some(256),
            stop_tokens: Some(vec!["<end_of_turn>".to_string()]),
            stop_on_repeated_line: false,
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("\"type\":\"generate\""));
        assert!(json.contains("\"prompt\":\"test prompt\""));
        assert!(json.contains("\"max_tokens\":512"));
        assert!(json.contains("\"temperature\":1.0"));
        assert!(json.contains("\"presence_penalty\":0.3"));
        assert!(json.contains("\"frequency_penalty\":0.0"));
        assert!(json.contains("\"repeat_penalty\":1.05"));
        assert!(json.contains("\"penalty_last_n\":256"));
    }

    #[test]
    fn test_response_deserialization() {
        let json = r#"{"type":"response","text":"generated text","error":null}"#;
        let response: Response = serde_json::from_str(json).unwrap();

        match response {
            Response::Completion { text, error } => {
                assert_eq!(text, "generated text");
                assert!(error.is_none());
            }
            _ => panic!("Wrong response type"),
        }
    }

    #[test]
    fn test_error_response_deserialization() {
        let json = r#"{"type":"error","message":"something went wrong"}"#;
        let response: Response = serde_json::from_str(json).unwrap();

        match response {
            Response::Error { message } => {
                assert_eq!(message, "something went wrong");
            }
            _ => panic!("Wrong response type"),
        }
    }
}
