//! Explicit read-only grants for stdio/CLI, and signed opt-in webhook delivery.
use once_cell::sync::Lazy;
use ring::hmac;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use std::time::Duration;
use tauri::{AppHandle, Manager, Runtime};

static CONFIG_PATH: Lazy<RwLock<Option<PathBuf>>> = Lazy::new(|| RwLock::new(None));

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct IntegrationSettings {
    pub api_enabled: bool,
    pub api_port: u16,
    pub api_token: String,
    pub cli_token: String,
    pub mcp_token: String,
    pub record_control: bool,
    pub write_notes: bool,
    pub cli_enabled: bool,
    pub mcp_enabled: bool,
    pub read_meetings: bool,
    pub read_transcripts: bool,
    pub read_summaries: bool,
    pub webhook_enabled: bool,
    pub webhook_url: String,
    pub webhook_secret: String,
    pub webhook_include_summary: bool,
}

#[derive(Serialize)]
pub struct IntegrationStatus {
    settings: IntegrationSettings,
    config_path: String,
    companion_path: String,
}

pub fn initialize<R: Runtime>(app: &AppHandle<R>) {
    if let Ok(dir) = app.path().app_data_dir() {
        if let Ok(mut path) = CONFIG_PATH.write() {
            *path = Some(dir.join("integrations.json"));
            if purge_legacy_webhook_outbox(&dir.join("webhook-outbox")).is_err() {
                log::warn!("Could not clear obsolete webhook payloads");
            }
        }
    }
}

// Older versions persisted bodies but had no crash-replay reader. Retry in
// memory only, so failed delivery cannot retain deleted meeting content on disk.
fn purge_legacy_webhook_outbox(directory: &Path) -> Result<(), String> {
    if !directory.exists() {
        return Ok(());
    }
    let meta =
        std::fs::symlink_metadata(directory).map_err(|_| "Could not inspect webhook outbox")?;
    if !meta.is_dir() || meta.file_type().is_symlink() {
        return Err("Invalid webhook outbox".into());
    }
    for entry in std::fs::read_dir(directory).map_err(|_| "Could not read webhook outbox")? {
        let entry = entry.map_err(|_| "Could not inspect webhook event")?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json")
            || path
                .file_stem()
                .and_then(|s| s.to_str())
                .and_then(|s| uuid::Uuid::parse_str(s).ok())
                .is_none()
            || !entry.file_type().is_ok_and(|t| t.is_file())
        {
            continue;
        }
        std::fs::remove_file(path).map_err(|_| "Could not clear obsolete webhook event")?;
    }
    Ok(())
}

async fn webhook_delivery_allowed(
    pool: &sqlx::SqlitePool,
    id: &str,
    destination: &str,
) -> Result<bool, String> {
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM meetings WHERE id=?)")
        .bind(id)
        .fetch_one(pool)
        .await
        .map_err(|_| "Meeting state unavailable")?;
    if !exists {
        return Ok(false);
    }
    let mut destination = validate_webhook_url(destination)?;
    destination.set_query(None);
    crate::security::privacy::guard_destination(pool, destination.as_str()).await?;
    Ok(true)
}

pub(crate) fn read_settings(path: &Path) -> Result<IntegrationSettings, String> {
    match crate::security::storage::read_limited(path, 1024 * 1024) {
        Ok(data) => {
            serde_json::from_slice(&data).map_err(|_| "Invalid integrations configuration".into())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(IntegrationSettings::default()),
        Err(e) => Err(format!("Cannot read integrations configuration: {e}")),
    }
}

pub(crate) fn configuration_path() -> Option<PathBuf> {
    CONFIG_PATH.read().ok().and_then(|path| path.clone())
}

pub fn write_private_json(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let parent = path.parent().ok_or("Configuration path has no parent")?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    crate::security::storage::write_json_atomic(path, value).map_err(|e| e.to_string())
}

fn companion_path<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, String> {
    let bundled = app
        .path()
        .resolve(
            "integrations/sonefold.py",
            tauri::path::BaseDirectory::Resource,
        )
        .map_err(|e| e.to_string())?;
    if bundled.is_file() {
        return Ok(bundled);
    }
    #[cfg(debug_assertions)]
    {
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../integrations/sonefold.py");
        if source.is_file() {
            return source.canonicalize().map_err(|e| e.to_string());
        }
    }
    Err("Integration companion is not bundled in this build".into())
}

#[tauri::command]
pub async fn integrations_get_settings<R: Runtime>(
    app: AppHandle<R>,
) -> Result<IntegrationStatus, String> {
    let path = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("integrations.json");
    Ok(IntegrationStatus {
        settings: read_settings(&path)?,
        config_path: path.to_string_lossy().into_owned(),
        companion_path: companion_path(&app)?.to_string_lossy().into_owned(),
    })
}

pub fn validate_webhook_url(input: &str) -> Result<url::Url, String> {
    let url =
        url::Url::parse(input).map_err(|_| "Webhook URL must be an absolute URL".to_string())?;
    let local = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
    if url.scheme() != "https" && !(url.scheme() == "http" && local) {
        return Err("Use HTTPS for webhooks (HTTP is allowed only on localhost)".into());
    }
    if url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err("Webhook URL cannot contain credentials or a fragment".into());
    }
    Ok(url)
}

fn webhook_destination(input: &str) -> Result<(), String> {
    let mut url = validate_webhook_url(input)?;
    url.set_query(None);
    crate::security::privacy::guard_process_destination(url.as_str())
}

#[tauri::command]
pub async fn integrations_save_settings<R: Runtime>(
    app: AppHandle<R>,
    mut settings: IntegrationSettings,
) -> Result<IntegrationStatus, String> {
    if settings.api_port == 0 {
        settings.api_port = 8421;
    }
    if settings.api_port < 1024 {
        return Err("Use an unprivileged API port (1024 or higher)".into());
    }
    for token in [
        &mut settings.api_token,
        &mut settings.cli_token,
        &mut settings.mcp_token,
    ] {
        if token.is_empty() {
            *token = format!(
                "{}{}",
                uuid::Uuid::new_v4().simple(),
                uuid::Uuid::new_v4().simple()
            );
        }
        if token.len() < 32 || token.len() > 1024 {
            return Err("API tokens must contain 32 to 1024 bytes".into());
        }
    }
    if settings.webhook_enabled {
        validate_webhook_url(&settings.webhook_url)?;
        if settings.webhook_secret.is_empty() {
            settings.webhook_secret = format!(
                "{}{}",
                uuid::Uuid::new_v4().simple(),
                uuid::Uuid::new_v4().simple()
            );
        }
        if settings.webhook_secret.len() < 32 || settings.webhook_secret.len() > 1024 {
            return Err("Webhook signing secret must contain 32 to 1024 bytes".into());
        }
    }
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let mut value = serde_json::to_value(&settings).map_err(|e| e.to_string())?;
    value["database_path"] = serde_json::json!(dir.join("meeting_minutes.sqlite"));
    write_private_json(&dir.join("integrations.json"), &value)?;
    initialize(&app);
    crate::automation::restart(app.clone()).await?;
    integrations_get_settings(app).await
}

pub fn webhook_signature(secret: &str, timestamp: i64, body: &[u8]) -> String {
    let key = hmac::Key::new(hmac::HMAC_SHA256, secret.as_bytes());
    let mut context = hmac::Context::with_key(&key);
    context.update(timestamp.to_string().as_bytes());
    context.update(b".");
    context.update(body);
    context
        .sign()
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Summary completion never waits for external receivers; delivery is bounded and retryable.
pub async fn summary_completed(pool: &sqlx::SqlitePool, meeting_id: &str, markdown: &str) {
    let path = CONFIG_PATH.read().ok().and_then(|p| p.clone());
    let Some(path) = path else {
        return;
    };
    let Ok(settings) = read_settings(&path) else {
        return;
    };
    if !settings.webhook_enabled || validate_webhook_url(&settings.webhook_url).is_err() {
        return;
    }
    if webhook_destination(&settings.webhook_url).is_err() {
        return;
    }
    let timestamp = chrono::Utc::now().timestamp();
    let event_id = uuid::Uuid::new_v4().to_string();
    let mut payload = serde_json::json!({
        "id": event_id, "type": "summary.completed", "created_at": timestamp,
        "meeting_id": meeting_id,
    });
    if settings.webhook_include_summary {
        payload["summary_markdown"] = serde_json::json!(markdown);
    }
    let Ok(body) = serde_json::to_vec(&payload) else {
        return;
    };
    let pool = pool.clone();
    let meeting_id = meeting_id.to_owned();
    tokio::spawn(async move {
        let client = match reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .build()
        {
            Ok(client) => client,
            Err(_) => return,
        };
        for attempt in 0..3 {
            // Revocation takes effect before every delivery, including scheduled retries.
            let Ok(current) = read_settings(&path) else {
                return;
            };
            if !current.webhook_enabled
                || current.webhook_url != settings.webhook_url
                || current.webhook_secret != settings.webhook_secret
                || (settings.webhook_include_summary && !current.webhook_include_summary)
            {
                return;
            }
            // Privacy settings may change while this event waits for a retry.
            // Queries are allowed in webhook URLs, but cannot bypass the host gate.
            if !matches!(
                webhook_delivery_allowed(&pool, &meeting_id, &current.webhook_url).await,
                Ok(true)
            ) {
                return;
            }
            let signed_at = chrono::Utc::now().timestamp();
            let signature = webhook_signature(&current.webhook_secret, signed_at, &body);
            match client
                .post(&current.webhook_url)
                .header("content-type", "application/json")
                .header("x-sonefold-event-id", &event_id)
                .header("x-sonefold-timestamp", signed_at.to_string())
                .header("x-sonefold-signature", format!("v1={signature}"))
                .body(body.clone())
                .send()
                .await
            {
                Ok(response) if response.status().is_success() => {
                    return;
                }
                _ => {
                    if attempt < 2 {
                        tokio::time::sleep(Duration::from_secs(2u64.pow(attempt + 1))).await;
                    }
                }
            }
        }
        log::warn!("Webhook delivery failed after three attempts; payload discarded");
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn webhook_retries_check_current_privacy_and_meeting_existence() {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        sqlx::query("INSERT INTO meetings(id,title,created_at,updated_at) VALUES('m','Synthetic','now','now')").execute(&pool).await.unwrap();
        let url = "https://example.org/events?route=one";
        assert!(webhook_delivery_allowed(&pool, "m", url).await.unwrap());
        sqlx::query("UPDATE privacy_settings SET policy=? WHERE id=1")
            .bind(r#"{"local_only":true}"#)
            .execute(&pool)
            .await
            .unwrap();
        assert!(webhook_delivery_allowed(&pool, "m", url).await.is_err());
        assert!(
            webhook_delivery_allowed(&pool, "m", "http://127.0.0.1:7777/events")
                .await
                .unwrap()
        );
        sqlx::query("DELETE FROM meetings WHERE id='m'")
            .execute(&pool)
            .await
            .unwrap();
        assert!(
            !webhook_delivery_allowed(&pool, "m", "http://127.0.0.1:7777/events")
                .await
                .unwrap()
        );
    }
    #[test]
    fn obsolete_webhook_bodies_are_removed_without_following_links() {
        let root = tempfile::tempdir().unwrap();
        let outbox = root.path().join("webhook-outbox");
        std::fs::create_dir(&outbox).unwrap();
        let event = outbox.join(format!("{}.json", uuid::Uuid::new_v4()));
        std::fs::write(&event, b"synthetic old payload").unwrap();
        let other = outbox.join("keep.json");
        std::fs::write(&other, b"unrelated").unwrap();
        purge_legacy_webhook_outbox(&outbox).unwrap();
        assert!(!event.exists());
        assert!(other.exists());
        #[cfg(unix)]
        {
            let link = root.path().join("link");
            std::os::unix::fs::symlink(&outbox, &link).unwrap();
            assert!(purge_legacy_webhook_outbox(&link).is_err());
        }
    }
    #[test]
    fn webhooks_require_secure_transport_and_no_redirect_credentials() {
        assert!(validate_webhook_url("https://example.com/events").is_ok());
        assert!(validate_webhook_url("http://127.0.0.1:7777/events").is_ok());
        for url in [
            "http://example.com",
            "https://user:pass@example.com",
            "file:///tmp/event",
            "https://example.com/#secret",
        ] {
            assert!(validate_webhook_url(url).is_err());
        }
    }
    #[test]
    fn signatures_authenticate_timestamp_and_exact_body() {
        let first = webhook_signature("secret", 123, b"{\"id\":1}");
        assert_eq!(first.len(), 64);
        assert_ne!(first, webhook_signature("secret", 124, b"{\"id\":1}"));
        assert_ne!(first, webhook_signature("secret", 123, b"{\"id\":2}"));
        assert_ne!(first, webhook_signature("another", 123, b"{\"id\":1}"));
    }
}
