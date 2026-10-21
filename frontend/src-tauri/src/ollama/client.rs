use crate::ollama::metadata::ModelMetadataCache;
use futures_util::StreamExt;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::process::Command;
use tauri::{command, AppHandle, Emitter, Runtime};
use tokio::time::{sleep, timeout, Duration};

// Global cache for model metadata (5 minute TTL)
static METADATA_CACHE: Lazy<ModelMetadataCache> =
    Lazy::new(|| ModelMetadataCache::new(Duration::from_secs(300)));

// Error categorization for better error handling and user feedback
#[derive(Debug)]
pub enum OllamaError {
    Timeout,
    NetworkError(String),
    InvalidEndpoint(String),
    ServerError(String),
    NoModelsFound,
}

impl std::fmt::Display for OllamaError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            OllamaError::Timeout => write!(f, "Request timed out after 5 seconds. Please check if the Ollama server is running."),
            OllamaError::NetworkError(msg) => write!(f, "Network error: {}. Please check your connection and endpoint URL.", msg),
            OllamaError::InvalidEndpoint(msg) => write!(f, "Invalid endpoint: {}. Please check the URL format.", msg),
            OllamaError::ServerError(msg) => write!(f, "Ollama server error: {}", msg),
            OllamaError::NoModelsFound => write!(f, "No models found on the Ollama server. Please pull models using 'ollama pull <model>'."),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OllamaModel {
    pub name: String,
    pub id: String,
    pub size: String,
    pub modified: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct OllamaApiResponse {
    models: Vec<OllamaApiModel>,
}

#[derive(Debug, Serialize, Deserialize)]
struct OllamaApiModel {
    name: String,
    model: String,
    modified_at: String,
    size: i64,
}

// Helper function to check if endpoint is localhost
fn is_localhost_endpoint(endpoint: Option<&str>) -> bool {
    super::boundary::endpoint(endpoint)
        .is_ok_and(|url| crate::security::transport::is_loopback(&url))
}

fn validate_endpoint_url(url: &str) -> Result<(), OllamaError> {
    super::boundary::endpoint(Some(url))
        .map(|_| ())
        .map_err(OllamaError::InvalidEndpoint)
}

#[command]
pub async fn get_ollama_models(endpoint: Option<String>) -> Result<Vec<OllamaModel>, String> {
    // Validate endpoint format if provided
    if let Some(ref ep) = endpoint {
        if let Err(e) = validate_endpoint_url(ep) {
            return Err(e.to_string());
        }
    }

    // Add timeout wrapper (5 seconds max)
    match timeout(
        Duration::from_secs(5),
        get_models_via_http_with_retry(endpoint.as_deref()),
    )
    .await
    {
        Ok(Ok(models)) => {
            if models.is_empty() {
                Err(OllamaError::NoModelsFound.to_string())
            } else {
                Ok(models)
            }
        }
        Ok(Err(http_err)) => {
            // A CLI retry must not bypass rejected redirects or response limits.
            // Only our own connection-error classification allows this fallback.
            if is_localhost_endpoint(endpoint.as_deref()) && http_err.contains("Cannot connect to")
            {
                let endpoint = super::boundary::endpoint(endpoint.as_deref())?;
                tokio::task::spawn_blocking(move || get_models_via_cli(endpoint.as_str()))
                    .await
                    .map_err(|_| "Ollama discovery worker stopped".to_owned())?
                    .map_err(|cli_err| format!("{http_err}\n\nAlso tried CLI: {cli_err}"))
            } else {
                Err(http_err)
            }
        }
        Err(_) => Err(OllamaError::Timeout.to_string()),
    }
}

// HTTP request with retry logic and exponential backoff
async fn get_models_via_http_with_retry(
    endpoint: Option<&str>,
) -> Result<Vec<OllamaModel>, String> {
    const MAX_RETRIES: u32 = 2;
    const INITIAL_BACKOFF_MS: u64 = 300;

    let mut last_error = String::new();

    for attempt in 0..=MAX_RETRIES {
        match get_models_via_http_async(endpoint).await {
            Ok(models) => return Ok(models),
            Err(e) => {
                last_error = e.clone();

                // Don't retry on certain errors
                if e.contains("Invalid endpoint") || e.contains("404") {
                    return Err(e);
                }

                // If not the last attempt, wait with exponential backoff
                if attempt < MAX_RETRIES {
                    let backoff_duration = INITIAL_BACKOFF_MS * 2_u64.pow(attempt);
                    sleep(Duration::from_millis(backoff_duration)).await;
                }
            }
        }
    }

    Err(format!(
        "Failed after {} retries: {}",
        MAX_RETRIES, last_error
    ))
}

async fn get_models_via_http_async(endpoint: Option<&str>) -> Result<Vec<OllamaModel>, String> {
    let client = crate::security::transport::client()?;
    let base_url = super::boundary::endpoint(endpoint)?;
    let url = super::boundary::url(&base_url, "tags");

    let response = client
        .get(&url)
        .timeout(Duration::from_secs(3)) // Per-request timeout
        .send()
        .await
        .map_err(|e| {
            if e.is_timeout() {
                OllamaError::NetworkError("Connection timed out".to_string()).to_string()
            } else if e.is_connect() {
                OllamaError::NetworkError(format!(
                    "Cannot connect to {}. Please check if the server is running.",
                    base_url
                ))
                .to_string()
            } else {
                OllamaError::NetworkError(e.to_string()).to_string()
            }
        })?;

    if !response.status().is_success() {
        return Err(OllamaError::ServerError(format!(
            "HTTP {}: Server returned an error",
            response.status()
        ))
        .to_string());
    }

    let api_response: OllamaApiResponse = super::boundary::json(response).await?;
    if api_response.models.len() > 4096 {
        return Err("Ollama model catalogue exceeds its size limit".into());
    }

    Ok(api_response
        .models
        .into_iter()
        .map(|m| OllamaModel {
            name: m.name,
            id: m.model,
            size: format_size(m.size),
            modified: m.modified_at,
        })
        .collect())
}

fn get_models_via_cli(endpoint: &str) -> Result<Vec<OllamaModel>, String> {
    let mut command = Command::new("ollama");
    command.arg("list").env("OLLAMA_HOST", endpoint);
    let output = overtalk_worker_sandbox::run_bounded(command, 256 * 1024, Duration::from_secs(5))
        .map_err(|_| {
            "Ollama CLI unavailable, timed out or exceeded its response limit".to_owned()
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(OllamaError::ServerError(format!("Ollama CLI error: {}", stderr)).to_string());
    }

    let output_str = String::from_utf8_lossy(&output.stdout);
    let mut models = Vec::new();

    // Skip the header line
    for line in output_str.lines().skip(1) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 4 {
            models.push(OllamaModel {
                name: parts[0].to_string(),
                id: parts[1].to_string(),
                size: format!("{} {}", parts[2], parts[3]),
                modified: parts[4..].join(" "),
            });
        }
    }

    if models.is_empty() {
        return Err(OllamaError::NoModelsFound.to_string());
    }

    Ok(models)
}

fn format_size(size: i64) -> String {
    if size < 1024 {
        format!("{} B", size)
    } else if size < 1024 * 1024 {
        format!("{:.1} KB", size as f64 / 1024.0)
    } else if size < 1024 * 1024 * 1024 {
        format!("{:.1} MB", size as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1} GB", size as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

#[command]
pub async fn pull_ollama_model<R: Runtime>(
    app_handle: AppHandle<R>,
    model_name: String,
    endpoint: Option<String>,
) -> Result<(), String> {
    let endpoint = super::boundary::endpoint(endpoint.as_deref())?;
    let _job = super::boundary::ModelJob::acquire(&endpoint, &model_name)?;
    let outcome = pull_model(&endpoint, &model_name, |progress| {
        let _ = app_handle.emit(
            "ollama-model-download-progress",
            serde_json::json!({"modelName":model_name,"progress":progress}),
        );
    })
    .await;
    match &outcome {
        Ok(()) => {
            let _ = app_handle.emit(
                "ollama-model-download-complete",
                serde_json::json!({"modelName":model_name}),
            );
        }
        Err(error) => {
            let _ = app_handle.emit(
                "ollama-model-download-error",
                serde_json::json!({"modelName":model_name,"error":error}),
            );
        }
    }
    outcome
}

async fn pull_model(
    endpoint: &reqwest::Url,
    model_name: &str,
    mut emit: impl FnMut(u8),
) -> Result<(), String> {
    let response = crate::security::transport::client()?
        .post(super::boundary::url(endpoint, "pull"))
        .json(&serde_json::json!({"name":model_name,"stream":true}))
        .timeout(Duration::from_secs(600))
        .send()
        .await
        .map_err(|_| "Could not pull the Ollama model".to_owned())?;
    if !response.status().is_success() {
        return Err(format!(
            "Ollama returned HTTP {} while pulling the model",
            response.status()
        ));
    }
    let mut stream = response.bytes_stream();
    let mut progress = super::boundary::Progress::default();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| "Ollama download stream interrupted")?;
        progress.feed(&chunk, &mut emit)?;
    }
    progress.finish(emit)
}

#[command]
pub async fn delete_ollama_model(
    model_name: String,
    endpoint: Option<String>,
) -> Result<(), String> {
    let base_url = super::boundary::endpoint(endpoint.as_deref())?;
    let _job = super::boundary::ModelJob::acquire(&base_url, &model_name)?;
    let client = crate::security::transport::client()?;
    let url = super::boundary::url(&base_url, "delete");

    let payload = serde_json::json!({
        "name": model_name
    });

    log::info!("Deleting Ollama model: {}", model_name);

    let response = client
        .delete(&url)
        .json(&payload)
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| {
            if e.is_timeout() {
                format!("Delete request timed out for model: {}", model_name)
            } else if e.is_connect() {
                format!(
                    "Cannot connect to {}. Please check if the Ollama server is running.",
                    base_url
                )
            } else {
                format!("Failed to delete model: {}", e)
            }
        })?;

    if !response.status().is_success() {
        let status = response.status();
        return Err(format!("Failed to delete model (HTTP {status})"));
    }

    log::info!("Successfully deleted Ollama model: {}", model_name);

    Ok(())
}

/// Get the context size for a specific Ollama model
///
/// This command fetches model metadata and returns the context size.
/// Results are cached for 5 minutes to avoid repeated API calls.
///
/// # Arguments
/// * `model_name` - Name of the model (e.g., "llama3.2:1b")
/// * `endpoint` - Optional custom Ollama endpoint
///
/// # Returns
/// Context size in tokens, or error message
#[command]
pub async fn get_ollama_model_context(
    model_name: String,
    endpoint: Option<String>,
) -> Result<usize, String> {
    log::info!("Fetching context size for model: {}", model_name);

    match METADATA_CACHE
        .get_or_fetch(&model_name, endpoint.as_deref())
        .await
    {
        Ok(metadata) => {
            log::info!(
                "Model {} context size: {} tokens",
                model_name,
                metadata.context_size
            );
            Ok(metadata.context_size)
        }
        Err(e) => {
            log::warn!(
                "Failed to fetch context for {}: {}. Returning default 4000",
                model_name,
                e
            );
            // Return default instead of error for better UX
            Ok(4000)
        }
    }
}

#[cfg(test)]
mod security_tests {
    use super::*;

    #[test]
    fn endpoint_requires_secure_transport_and_exact_loopback() {
        for url in [
            "http://remote.example",
            "http://localhost.example",
            "https://user:secret@example.test",
            "https://example.test/?token=value",
        ] {
            assert!(
                validate_endpoint_url(url).is_err(),
                "Accepted unsafe endpoint"
            );
        }
        assert!(!is_localhost_endpoint(Some("https://localhost.example")));
        assert!(!is_localhost_endpoint(Some(
            "https://example.test/127.0.0.1"
        )));
        assert!(is_localhost_endpoint(Some("http://127.0.0.1:11434")));
        assert!(is_localhost_endpoint(Some("http://[::1]:11434")));
    }

    async fn server(status: &str, header: &str, body: String) -> String {
        use tokio::{
            io::{AsyncReadExt, AsyncWriteExt},
            net::TcpListener,
        };
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\n{header}Connection: close\r\n\r\n{body}",
            body.len()
        );
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let _ = stream.read(&mut [0; 8192]).await;
            let _ = stream.write_all(response.as_bytes()).await;
        });
        url
    }

    #[tokio::test]
    async fn model_discovery_does_not_follow_redirects() {
        let target = server("200 OK", "", r#"{"models":[]}"#.into()).await;
        let origin = server(
            "302 Found",
            &format!("Location: {target}/api/tags\r\n"),
            String::new(),
        )
        .await;
        assert!(get_models_via_http_async(Some(&origin)).await.is_err());
    }

    #[tokio::test]
    async fn model_discovery_bounds_provider_response() {
        let body = serde_json::json!({"models":[{"name":"a".repeat(4*1024*1024),"model":"test","modified_at":"today","size":1}]}).to_string();
        let url = server("200 OK", "", body).await;
        assert!(get_models_via_http_async(Some(&url)).await.is_err());
    }

    #[tokio::test]
    async fn pull_requires_confirmation_and_accepts_final_unterminated_json() {
        for (body, success) in [
            ("{\"status\":\"success\"}", true),
            ("{\"completed\":90,\"total\":100}\n", false),
            ("{\"error\":\"synthetic rejection\"}\n", false),
        ] {
            let url = server("200 OK", "", body.into()).await;
            let endpoint = super::super::boundary::endpoint(Some(&url)).unwrap();
            assert_eq!(
                pull_model(&endpoint, "synthetic", |_| {}).await.is_ok(),
                success
            );
        }
    }
}
