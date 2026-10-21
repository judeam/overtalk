//! Privatemode's official local proxy performs attestation and encryption.
//! Never fall back to its proxyless API or send a prompt to a remote HTTP URL.
use reqwest::{Client, Url};
use serde::Serialize;
use std::{net::IpAddr, sync::OnceLock, time::Duration};

pub const DEFAULT_ENDPOINT: &str = "http://127.0.0.1:8080/v1";

pub fn endpoint(value: Option<&str>) -> Result<Url, String> {
    let value = value
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(DEFAULT_ENDPOINT);
    let mut url =
        Url::parse(value.trim()).map_err(|_| "Enter a valid local Privatemode proxy URL")?;
    let local = match url.host() {
        Some(url::Host::Ipv4(ip)) => IpAddr::V4(ip).is_loopback(),
        Some(url::Host::Ipv6(ip)) => IpAddr::V6(ip).is_loopback(),
        // Resolve no external names, and do not trust lookalike localhost domains.
        Some(url::Host::Domain("localhost")) => {
            url.set_host(Some("127.0.0.1"))
                .map_err(|_| "Invalid proxy host")?;
            true
        }
        _ => false,
    };
    if !local || !matches!(url.scheme(), "http" | "https") {
        return Err("Privatemode requires its official proxy on this device (127.0.0.1 or ::1). Remote and proxyless URLs are not accepted.".into());
    }
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(
            "Keep credentials, query parameters and fragments out of the proxy URL.".into(),
        );
    }
    match url.path().trim_end_matches('/') {
        "" | "/v1" => url.set_path("/v1/"),
        _ => return Err("The Privatemode proxy base URL must end in /v1.".into()),
    }
    Ok(url)
}

/// The proxy the user configured, or Privatemode's proxy run by Overtalk with
/// the user's own key when no address is set and nothing listens on the default.
pub async fn resolve(value: Option<&str>, api_key: &str) -> Result<Url, String> {
    let url = endpoint(value)?;
    let default = endpoint(None)?;
    if url != default || api_key.trim().is_empty() {
        return Ok(url);
    }
    let address = (
        url.host_str()
            .unwrap_or("127.0.0.1")
            .trim_matches(['[', ']'])
            .to_owned(),
        url.port_or_known_default().unwrap_or(8080),
    );
    let listening = tokio::time::timeout(
        Duration::from_millis(500),
        tokio::net::TcpStream::connect(address),
    )
    .await
    .is_ok_and(|r| r.is_ok());
    if listening {
        Ok(url)
    } else {
        super::privatemode_runtime::endpoint(api_key).await
    }
}

/// Dedicated pooled client: a system HTTP proxy or redirect must never receive
/// unencrypted transcripts or credentials intended for the local proxy.
pub fn client() -> Result<&'static Client, String> {
    static CLIENT: OnceLock<Result<Client, String>> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(5))
                .timeout(Duration::from_secs(300))
                .build()
                .map_err(|_| "Could not initialize the Privatemode connection".into())
        })
        .as_ref()
        .map_err(Clone::clone)
}

pub const DEFAULT_MAX_TOKENS: u32 = 16384;

/// Conservative limits from the provider's catalog; unknown future models
/// must not inherit GLM's large context without a verified limit.
pub fn context_size(model: &str) -> usize {
    match model {
        "glm-5.3" | "glm-latest" | "glm-5.3-flash" | "glm-flash-latest" => 1_000_000,
        "gpt-oss-120b" | "openai/gpt-oss-120b" | "gpt-oss-latest" => 128_000,
        _ => 32_768,
    }
}

pub async fn key(value: Option<String>) -> Result<Option<String>, String> {
    tokio::task::spawn_blocking(move || {
        let entry = keyring::Entry::new("Overtalk providers", "privatemode")
            .map_err(|_| "Cannot access the OS keychain")?;
        if let Some(value) = value {
            entry
                .set_password(&value)
                .map_err(|_| "Unlock the OS keychain to save the Privatemode API key")?;
            Ok(None)
        } else {
            match entry.get_password() {
                Ok(value) => Ok(Some(value)),
                Err(keyring::Error::NoEntry) => Ok(None),
                Err(_) => Err("Unlock the OS keychain to read the Privatemode API key".into()),
            }
        }
    })
    .await
    .map_err(|_| "The OS keychain request failed".to_string())?
}

pub async fn delete_key() -> Result<(), String> {
    tokio::task::spawn_blocking(|| {
        let entry = keyring::Entry::new("Overtalk providers", "privatemode")
            .map_err(|_| "Cannot access the OS keychain")?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err("Unlock the OS keychain to remove the Privatemode API key".into()),
        }
    })
    .await
    .map_err(|_| "The OS keychain request failed".to_string())?
}

#[derive(Debug, Serialize)]
pub struct Model {
    pub name: String,
    pub display_name: String,
    pub status: &'static str,
}

pub fn api_error(status: reqwest::StatusCode) -> String {
    match status.as_u16() {
        401 | 403 => "Privatemode rejected authentication. Check the API key in Overtalk or in your local proxy.".into(),
        402 => "Privatemode credits are exhausted. No fallback provider was used.".into(),
        429 => "Privatemode rate or credit limit reached. Try again later.".into(),
        300..=399 => "The Privatemode proxy attempted a redirect, which Overtalk blocked.".into(),
        _ => format!("Privatemode proxy returned HTTP {}. Check its connection and attestation status.", status.as_u16()),
    }
}

pub async fn models(base: Option<&str>, api_key: &str) -> Result<Vec<Model>, String> {
    let url = endpoint(base)?
        .join("models")
        .map_err(|_| "Invalid proxy URL")?;
    let mut response = client()?.get(url)
        .bearer_auth(if api_key.trim().is_empty() { "placeholder" } else { api_key })
        .timeout(Duration::from_secs(20)).send().await
        .map_err(|_| "Could not reach the Privatemode proxy. Start the official proxy on this device, then try again.")?;
    if !response.status().is_success() {
        return Err(api_error(response.status()));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "Could not read the model list")?
    {
        if bytes.len() + chunk.len() > 1024 * 1024 {
            return Err("Proxy model list exceeds the size limit".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    parse_models(&bytes)
}

fn parse_models(bytes: &[u8]) -> Result<Vec<Model>, String> {
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| "Proxy returned an invalid model list")?;
    let rows = value["data"]
        .as_array()
        .ok_or("Proxy returned no model list")?;
    let mut models = Vec::new();
    for row in rows {
        let Some(id) = row["id"]
            .as_str()
            .filter(|id| !id.is_empty() && id.len() <= 200)
        else {
            continue;
        };
        // Only chat models belong in the summary picker. Keep IDs verbatim.
        if let Some(tasks) = row["tasks"].as_array() {
            if !tasks.iter().any(|task| task.as_str() == Some("generate")) {
                continue;
            }
        }
        let lower = id.to_ascii_lowercase();
        if lower.contains("embedding")
            || lower.contains("whisper")
            || lower.contains("voxtral")
            || lower.contains("ocr")
        {
            continue;
        }
        if models.iter().any(|m: &Model| m.name == id) {
            continue;
        }
        models.push(Model {
            name: id.into(),
            display_name: id.into(),
            status: "Available",
        });
    }
    let ids: std::collections::HashSet<_> = models.iter().map(|m| m.name.clone()).collect();
    models.retain(|model| match model.name.as_str() {
        "glm-5.2" => false, // Deprecated alias for 5.3, not another model.
        "glm-latest" => !ids.contains("glm-5.3"),
        "glm-flash-latest" => !ids.contains("glm-5.3-flash"),
        "gpt-oss-latest" | "openai/gpt-oss-120b" => !ids.contains("gpt-oss-120b"),
        _ => true,
    });
    let rank = |id: &str| match id {
        "glm-5.3" => 0,
        "glm-5.3-flash" => 1,
        "gpt-oss-120b" => 2,
        _ => 3,
    };
    models.sort_by(|a, b| {
        rank(&a.name)
            .cmp(&rank(&b.name))
            .then_with(|| a.name.cmp(&b.name))
    });
    if models.is_empty() {
        return Err("Privatemode returned no chat models".into());
    }
    Ok(models)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accepts_only_unambiguous_local_proxy_urls() {
        assert_eq!(
            endpoint(None).unwrap().as_str(),
            "http://127.0.0.1:8080/v1/"
        );
        assert_eq!(
            endpoint(Some("http://localhost:9000/")).unwrap().as_str(),
            "http://127.0.0.1:9000/v1/"
        );
        assert_eq!(
            endpoint(Some("http://[::1]:8080/v1/")).unwrap().as_str(),
            "http://[::1]:8080/v1/"
        );
        for invalid in [
            "https://api.privatemode.ai/v1",
            "http://localhost.example/v1",
            "http://192.168.1.2:8080/v1",
            "http://user:secret@localhost:8080/v1",
            "http://localhost:8080/v1?key=secret",
            "http://localhost:8080/v1#secret",
            "file:///v1",
            "http://127.0.0.1:8080/other",
        ] {
            assert!(endpoint(Some(invalid)).is_err(), "{invalid}");
        }
    }
    #[test]
    fn model_list_retains_identifiers_without_speech_or_embedding_models() {
        let rows = parse_models(br#"{"data":[{"id":"zai-org/GLM-5.3"},{"id":"openai/gpt-oss-120b"},{"id":"zai-org/GLM-5.3"},{"id":"Whisper-large-v3"},{"id":"Qwen3-Embedding-4B"},{"id":"DeepSeek-OCR-2"},{"id":""}]}"#).unwrap();
        assert_eq!(
            rows.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(),
            ["openai/gpt-oss-120b", "zai-org/GLM-5.3"]
        );
        assert!(parse_models(br#"{"error":"no models"}"#).is_err());
    }

    #[test]
    fn live_catalog_shows_concrete_models_once_in_size_order() {
        let rows = parse_models(
            concat!(
                r#"{"data":["#,
                r#"{"id":"glm-latest","tasks":["generate"]},"#,
                r#"{"id":"glm-5.2","tasks":["generate"]},"#,
                r#"{"id":"glm-5.3-flash","tasks":["generate","vision"]},"#,
                r#"{"id":"openai/gpt-oss-120b","tasks":["generate"]},"#,
                r#"{"id":"gpt-oss-120b","tasks":["generate"]},"#,
                r#"{"id":"glm-5.3","tasks":["generate"]},"#,
                r#"{"id":"future-embedding","tasks":["embed"]}"#,
                r#"]}"#
            )
            .as_bytes(),
        )
        .unwrap();
        assert_eq!(
            rows.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(),
            ["glm-5.3", "glm-5.3-flash", "gpt-oss-120b"]
        );
    }
}
