use reqwest::header;
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::path::PathBuf;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

const REQUEST_TIMEOUT_DURATION: Duration = Duration::from_secs(300);
const MAX_SUMMARY_RESPONSE_BYTES: usize = 8 * 1024 * 1024;
const MAX_PROVIDER_ERROR_BYTES: usize = 16 * 1024;

fn provider_status_error(status: reqwest::StatusCode) -> String {
    let guidance = match status.as_u16() {
        401 | 403 => "Check the provider credential and account permissions.",
        404 => "Check the configured endpoint and model name.",
        429 => "The provider's rate or usage limit was reached. Try again later.",
        500..=599 => "The provider is temporarily unavailable. Try again later.",
        _ => "Check the provider settings and try again.",
    };
    format!("LLM API request failed with status {status}. {guidance}")
}

async fn await_or_cancel<T>(
    operation: impl Future<Output = T>,
    cancellation_token: Option<&CancellationToken>,
) -> Result<T, String> {
    let Some(token) = cancellation_token else {
        return Ok(operation.await);
    };

    tokio::select! {
        biased;
        _ = token.cancelled() => Err("Summary generation was cancelled".to_string()),
        result = operation => Ok(result),
    }
}

// Generic structure for OpenAI-compatible API chat messages
#[derive(Debug, Serialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

// Generic structure for OpenAI-compatible API chat requests
#[derive(Debug, Serialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<&'static str>,
}

/// Build OpenAI-compat JSON body.
pub fn build_openai_compat_chat_body(
    provider: &LLMProvider,
    model_name: &str,
    system_prompt: &str,
    user_prompt: &str,
    max_tokens: Option<u32>,
    temperature: Option<f32>,
    top_p: Option<f32>,
) -> serde_json::Value {
    let (max_tokens_val, temperature_val, top_p_val) = if *provider == LLMProvider::CustomOpenAI {
        (max_tokens, temperature, top_p)
    } else {
        (None, None, None)
    };

    let mut body = serde_json::json!(ChatRequest {
        model: model_name.to_string(),
        messages: vec![
            ChatMessage {
                role: "system".to_string(),
                content: system_prompt.to_string(),
            },
            ChatMessage {
                role: "user".to_string(),
                content: user_prompt.to_string(),
            }
        ],
        max_tokens: max_tokens_val,
        temperature: temperature_val,
        top_p: top_p_val,
        reasoning_effort: (*provider == LLMProvider::Ollama).then_some("none"),
    });
    if matches!(
        provider,
        LLMProvider::Privatemode | LLMProvider::OvertalkCloud
    ) {
        // The allowance includes private reasoning. Do not use `none`: GLM
        // maps it to maximum reasoning rather than disabling it.
        body["max_completion_tokens"] = max_tokens.unwrap_or(16384).into();
        body["reasoning_effort"] = "low".into();
        // Meeting notes need repeatable factual extraction rather than creative
        // sampling. Preserve explicit user choices and other models' defaults.
        let temperature = temperature
            .or_else(|| matches!(model_name, "glm-5.3-flash" | "glm-flash-latest").then_some(0.0));
        if let Some(value) = temperature {
            body["temperature"] = value.into();
        }
        if let Some(value) = top_p {
            body["top_p"] = value.into();
        }
    }
    body
}

// Generic structure for OpenAI-compatible API chat responses
#[derive(Deserialize, Debug)]
pub struct ChatResponse {
    pub choices: Vec<Choice>,
}

#[derive(Deserialize, Debug)]
pub struct Choice {
    pub message: MessageContent,
    #[serde(default)]
    pub finish_reason: Option<String>,
}

#[derive(Deserialize, Debug)]
pub struct MessageContent {
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub reasoning: Option<String>,
    #[serde(default)]
    pub reasoning_content: Option<String>,
}

// Claude-specific request structure
#[derive(Debug, Serialize)]
pub struct ClaudeRequest {
    pub model: String,
    pub max_tokens: u32,
    pub system: String,
    pub messages: Vec<ChatMessage>,
}

// Claude-specific response structure
#[derive(Deserialize, Debug)]
pub struct ClaudeChatResponse {
    pub content: Vec<ClaudeChatContent>,
}

#[derive(Deserialize, Debug)]
pub struct ClaudeChatContent {
    #[serde(rename = "type")]
    pub block_type: String,
    pub text: Option<String>,
}

impl ClaudeChatResponse {
    fn completion(&self) -> Option<LlmCompletion> {
        let content = self
            .content
            .iter()
            .find(|block| block.block_type == "text")
            .and_then(|block| block.text.as_deref())?
            .trim()
            .to_string();
        Some(LlmCompletion {
            content,
            reasoning_stripped: self
                .content
                .iter()
                .any(|block| matches!(block.block_type.as_str(), "thinking" | "redacted_thinking")),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LlmCompletion {
    pub content: String,
    pub reasoning_stripped: bool,
}

impl ChatResponse {
    fn completion(&self) -> Result<LlmCompletion, String> {
        let message = &self
            .choices
            .first()
            .ok_or("No content in LLM response")?
            .message;
        Ok(LlmCompletion {
            content: message
                .content
                .as_deref()
                .unwrap_or_default()
                .trim()
                .to_string(),
            reasoning_stripped: [
                message.reasoning.as_deref(),
                message.reasoning_content.as_deref(),
            ]
            .into_iter()
            .flatten()
            .any(|reasoning| !reasoning.trim().is_empty()),
        })
    }
}

pub(crate) fn ollama_rejects_reasoning_effort(status: reqwest::StatusCode, body: &str) -> bool {
    if !matches!(status.as_u16(), 400 | 422) {
        return false;
    }

    let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else {
        return false;
    };
    let matches_field = |value: Option<&serde_json::Value>| {
        value
            .and_then(serde_json::Value::as_str)
            .is_some_and(|field| field.eq_ignore_ascii_case("reasoning_effort"))
    };
    if matches_field(value.get("param"))
        || matches_field(value.get("field"))
        || matches_field(value.get("error").and_then(|error| error.get("param")))
        || matches_field(value.get("error").and_then(|error| error.get("field")))
    {
        return true;
    }

    let matches_message = [
        value.as_str(),
        value.get("error").and_then(serde_json::Value::as_str),
        value.get("message").and_then(serde_json::Value::as_str),
        value
            .get("error")
            .and_then(|error| error.get("message"))
            .and_then(serde_json::Value::as_str),
    ]
    .into_iter()
    .flatten()
    .any(|message| {
        let message = message.to_ascii_lowercase();
        message.contains("reasoning_effort")
            || (message.contains("think value")
                && message.contains("none")
                && (message.contains("not supported") || message.contains("invalid")))
    });
    matches_message
}
/// LLM Provider enumeration for multi-provider support
#[derive(Debug, Clone, PartialEq)]
pub enum LLMProvider {
    OpenAI,
    Claude,
    Groq,
    Ollama,
    OpenRouter,
    BuiltInAI,
    CustomOpenAI,
    Privatemode,
    OvertalkCloud,
}

impl std::str::FromStr for LLMProvider {
    type Err = String;

    /// Parse provider from string (case-insensitive).
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "openai" => Ok(Self::OpenAI),
            "claude" => Ok(Self::Claude),
            "groq" => Ok(Self::Groq),
            "ollama" => Ok(Self::Ollama),
            "openrouter" => Ok(Self::OpenRouter),
            "builtin-ai" | "local-llama" | "localllama" => Ok(Self::BuiltInAI),
            "custom-openai" => Ok(Self::CustomOpenAI),
            "privatemode" => Ok(Self::Privatemode),
            "overtalk-cloud" => Ok(Self::OvertalkCloud),
            _ => Err(format!("Unsupported LLM provider: {}", s)),
        }
    }
}

/// Complete inputs for one provider call. Both generation paths use the same transport policy.
pub(crate) struct SummaryRequest<'a> {
    pub provider: &'a LLMProvider,
    pub model_name: &'a str,
    pub api_key: &'a str,
    pub system_prompt: &'a str,
    pub user_prompt: &'a str,
    pub ollama_endpoint: Option<&'a str>,
    pub custom_openai_endpoint: Option<&'a str>,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub app_data_dir: Option<&'a PathBuf>,
    pub cancellation_token: Option<&'a CancellationToken>,
}

/// Generate visible notes with the normal reasoning policy.
pub(crate) async fn generate_summary(request: SummaryRequest<'_>) -> Result<LlmCompletion, String> {
    generate_summary_with_effort(request, None).await
}

/// A bounded, more deliberate source check; normal generation keeps its policy.
pub(crate) async fn generate_source_audit(
    request: SummaryRequest<'_>,
) -> Result<LlmCompletion, String> {
    generate_summary_with_effort(request, Some("high")).await
}

async fn generate_summary_with_effort(
    request: SummaryRequest<'_>,
    reasoning_override: Option<&str>,
) -> Result<LlmCompletion, String> {
    let SummaryRequest {
        provider,
        model_name,
        api_key,
        system_prompt,
        user_prompt,
        ollama_endpoint,
        custom_openai_endpoint,
        max_tokens,
        temperature,
        top_p,
        app_data_dir,
        cancellation_token,
    } = request;
    // Check if cancelled before starting
    if let Some(token) = cancellation_token {
        if token.is_cancelled() {
            return Err("Summary generation was cancelled".to_string());
        }
    }

    // Handle BuiltInAI provider separately (uses local sidecar, no HTTP API)
    if provider == &LLMProvider::BuiltInAI {
        let app_data_dir = app_data_dir
            .ok_or_else(|| "app_data_dir is required for BuiltInAI provider".to_string())?;

        return crate::summary::summary_engine::generate_with_builtin(
            app_data_dir,
            model_name,
            system_prompt,
            user_prompt,
            cancellation_token,
        )
        .await
        .map(|content| LlmCompletion {
            content,
            reasoning_stripped: false,
        })
        .map_err(|e| e.to_string());
    }

    let (api_url, mut headers) = match provider {
        LLMProvider::OpenAI => (
            "https://api.openai.com/v1/chat/completions".to_string(),
            header::HeaderMap::new(),
        ),
        LLMProvider::Groq => (
            "https://api.groq.com/openai/v1/chat/completions".to_string(),
            header::HeaderMap::new(),
        ),
        LLMProvider::OpenRouter => (
            "https://openrouter.ai/api/v1/chat/completions".to_string(),
            header::HeaderMap::new(),
        ),
        LLMProvider::Ollama => {
            let host = ollama_endpoint
                .map(|s| s.to_string())
                .unwrap_or_else(|| "http://localhost:11434".to_string());
            (
                format!("{}/v1/chat/completions", host),
                header::HeaderMap::new(),
            )
        }
        LLMProvider::CustomOpenAI => {
            let endpoint = custom_openai_endpoint
                .ok_or_else(|| "Custom OpenAI endpoint not configured".to_string())?;
            (
                format!("{}/chat/completions", endpoint.trim_end_matches('/')),
                header::HeaderMap::new(),
            )
        }
        LLMProvider::Privatemode => (
            crate::summary::privatemode::resolve(ollama_endpoint, api_key)
                .await?
                .join("chat/completions")
                .map_err(|_| "Invalid Privatemode proxy URL")?
                .to_string(),
            header::HeaderMap::new(),
        ),
        // Private summaries use the subscriber's own Privatemode key through
        // Privatemode's proxy on this computer; Overtalk's service never sees them.
        LLMProvider::OvertalkCloud => (
            crate::summary::privatemode_runtime::endpoint(&crate::cloud::summaries_key().await?)
                .await?
                .join("chat/completions")
                .map_err(|_| "Invalid Privatemode proxy URL")?
                .to_string(),
            header::HeaderMap::new(),
        ),
        LLMProvider::Claude => {
            let mut header_map = header::HeaderMap::new();
            header_map.insert(
                "x-api-key",
                api_key
                    .parse()
                    .map_err(|_| "Invalid API key format".to_string())?,
            );
            header_map.insert(
                "anthropic-version",
                "2023-06-01"
                    .parse()
                    .map_err(|_| "Invalid anthropic version".to_string())?,
            );
            (
                "https://api.anthropic.com/v1/messages".to_string(),
                header_map,
            )
        }
        LLMProvider::BuiltInAI => {
            // This case is handled earlier with early returns
            unreachable!("BuiltInAI is handled before this match statement")
        }
    };

    let api_url = crate::security::transport::endpoint(&api_url)?.to_string();
    crate::security::privacy::guard_process_destination(
        if matches!(
            provider,
            LLMProvider::Privatemode | LLMProvider::OvertalkCloud
        ) {
            "https://api.privatemode.ai"
        } else {
            &api_url
        },
    )?;
    let client = crate::security::transport::client()?;
    // Add authorization header for non-Claude providers
    if provider != &LLMProvider::Claude {
        headers.insert(
            header::AUTHORIZATION,
            format!(
                "Bearer {}",
                // The proxy run for private summaries holds the subscriber's key itself.
                if *provider == LLMProvider::OvertalkCloud
                    || (*provider == LLMProvider::Privatemode && api_key.trim().is_empty())
                {
                    "placeholder"
                } else {
                    api_key
                }
            )
            .parse()
            .map_err(|_| "Invalid authorization header".to_string())?,
        );
    }
    headers.insert(
        header::CONTENT_TYPE,
        "application/json"
            .parse()
            .map_err(|_| "Invalid content type".to_string())?,
    );

    // Build request body based on provider
    let mut request_body = if provider != &LLMProvider::Claude {
        build_openai_compat_chat_body(
            provider,
            model_name,
            system_prompt,
            user_prompt,
            max_tokens,
            temperature,
            top_p,
        )
    } else {
        serde_json::json!(ClaudeRequest {
            system: system_prompt.to_string(),
            model: model_name.to_string(),
            // Shared budget: on models with thinking enabled by default this
            // covers thinking tokens as well as the answer.
            max_tokens: 8192,
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: user_prompt.to_string(),
            }]
        })
    };

    if matches!(
        provider,
        LLMProvider::Privatemode | LLMProvider::OvertalkCloud
    ) {
        if let Some(effort) = reasoning_override {
            request_body["reasoning_effort"] = serde_json::json!(effort);
        }
    }

    info!(
        "🐞 LLM Request to {}: model={}",
        provider_name(provider),
        model_name
    );

    // Send request with timeout and cancellation support
    let request_future = client
        .post(api_url.clone())
        .headers(headers.clone())
        .json(&request_body)
        .timeout(REQUEST_TIMEOUT_DURATION)
        .send();

    // Use tokio::select to race between cancellation and request completion
    let response = if let Some(token) = cancellation_token {
        tokio::select! {
            result = request_future => {
                result.map_err(|e| {
                    if e.is_timeout() {
                        format!(
                            "LLM request timed out after {} seconds",
                            REQUEST_TIMEOUT_DURATION.as_secs()
                        )
                    } else {
                        "Failed to connect to the LLM provider".to_string()
                    }
                })?
            }
            _ = token.cancelled() => {
                return Err("Summary generation was cancelled".to_string());
            }
        }
    } else {
        request_future.await.map_err(|e| {
            if e.is_timeout() {
                format!(
                    "LLM request timed out after {} seconds",
                    REQUEST_TIMEOUT_DURATION.as_secs()
                )
            } else {
                "Failed to connect to the LLM provider".to_string()
            }
        })?
    };

    let response = if response.status().is_success() {
        response
    } else {
        let status = response.status();
        if *provider == LLMProvider::OvertalkCloud {
            if matches!(status.as_u16(), 401 | 403) {
                // A rotated or revoked key: fetch the current one next time.
                crate::cloud::forget_summaries_key().await;
            }
            return Err(match status.as_u16() {
                401 | 403 => "Your private summaries key was not accepted. Try again; if it repeats, check Private AI summaries in Settings".into(),
                402 | 429 => "This month's private AI summaries are used up. Local processing remains available".into(),
                _ => crate::summary::privatemode::api_error(status),
            });
        }
        if matches!(
            provider,
            LLMProvider::Privatemode | LLMProvider::OvertalkCloud
        ) {
            // Do not echo a proxy's response body: it may contain a prompt/key.
            return Err(crate::summary::privatemode::api_error(status));
        }
        // Inspect only a small error body for the legacy Ollama retry. Never
        // expose it: proxies/providers can echo credentials or meeting content.
        let error_body = await_or_cancel(
            crate::security::transport::response_text(response, MAX_PROVIDER_ERROR_BYTES),
            cancellation_token,
        )
        .await?
        .unwrap_or_default();
        if provider != &LLMProvider::Ollama || !ollama_rejects_reasoning_effort(status, &error_body)
        {
            return Err(provider_status_error(status));
        }

        warn!("Ollama rejected reasoning_effort; retrying once without it");
        let mut retry_body = request_body;
        retry_body
            .as_object_mut()
            .ok_or_else(|| "Failed to prepare Ollama compatibility retry".to_string())?
            .remove("reasoning_effort");
        let retry_future = client
            .post(api_url)
            .headers(headers)
            .json(&retry_body)
            .timeout(REQUEST_TIMEOUT_DURATION)
            .send();
        await_or_cancel(retry_future, cancellation_token)
            .await?
            .map_err(|e| {
                if e.is_timeout() {
                    format!(
                        "LLM retry request timed out after {} seconds",
                        REQUEST_TIMEOUT_DURATION.as_secs()
                    )
                } else {
                    "Failed to reconnect to the LLM provider".to_string()
                }
            })?
    };

    if !response.status().is_success() {
        let status = response.status();
        let _ = await_or_cancel(
            crate::security::transport::response_text(response, MAX_PROVIDER_ERROR_BYTES),
            cancellation_token,
        )
        .await?;
        return Err(provider_status_error(status));
    }

    // The byte cap also covers unknown JSON fields and chunked bodies. Parsing
    // errors must not include serde's offending value (which can be private).
    let body = await_or_cancel(
        crate::security::transport::response_text(response, MAX_SUMMARY_RESPONSE_BYTES),
        cancellation_token,
    )
    .await??;
    if provider == &LLMProvider::Claude {
        let chat_response: ClaudeChatResponse = serde_json::from_str(&body)
            .map_err(|_| "The LLM provider returned an invalid summary response")?;

        info!("🐞 LLM Response received from Claude");

        let completion = chat_response
            .completion()
            .ok_or("No text content in LLM response")?;
        Ok(completion)
    } else {
        let chat_response: ChatResponse = serde_json::from_str(&body)
            .map_err(|_| "The LLM provider returned an invalid summary response")?;

        info!("🐞 LLM Response received from {}", provider_name(provider));
        if matches!(
            provider,
            LLMProvider::Privatemode | LLMProvider::OvertalkCloud
        ) && chat_response
            .choices
            .first()
            .and_then(|choice| choice.finish_reason.as_deref())
            == Some("length")
        {
            return Err("Privatemode reached its output limit before finishing the notes. No partial notes were saved.".into());
        }
        chat_response.completion()
    }
}

/// Helper function to get provider name for logging
fn provider_name(provider: &LLMProvider) -> &str {
    match provider {
        LLMProvider::OpenAI => "OpenAI",
        LLMProvider::Claude => "Claude",
        LLMProvider::Groq => "Groq",
        LLMProvider::Ollama => "Ollama",
        LLMProvider::BuiltInAI => "Built-in AI",
        LLMProvider::OpenRouter => "OpenRouter",
        LLMProvider::CustomOpenAI => "Custom OpenAI",
        LLMProvider::Privatemode => "Privatemode",
        LLMProvider::OvertalkCloud => "Overtalk cloud",
    }
}

#[cfg(test)]
mod tests;
