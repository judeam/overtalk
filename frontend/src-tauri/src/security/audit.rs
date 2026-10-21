//! Content-free, HMAC authenticated audit events. The signing key stays in the
//! OS credential store. This detects changed events, not deletion of the tail.
use ring::hmac;
#[cfg(test)]
use sha2::{Digest, Sha256};
use sqlx::{Row, SqlitePool};
static WRITES: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
#[cfg(not(test))]
static KEYS: std::sync::LazyLock<std::sync::Mutex<std::collections::HashMap<String, Vec<u8>>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

async fn key(pool: &SqlitePool) -> Result<Vec<u8>, String> {
    let id: String =
        sqlx::query_scalar("SELECT credential_namespace FROM privacy_settings WHERE id=1")
            .fetch_one(pool)
            .await
            .map_err(|_| "Privacy settings unavailable")?;
    let id = format!("audit/{id}");
    #[cfg(test)]
    {
        Ok(Sha256::digest(id.as_bytes()).to_vec())
    }
    #[cfg(not(test))]
    {
        if let Some(key) = KEYS
            .lock()
            .map_err(|_| "Audit key cache unavailable")?
            .get(&id)
        {
            return Ok(key.clone());
        }
        let cache_id = id.clone();
        let secret = tokio::task::spawn_blocking(move || {
            let entry = keyring::Entry::new("Overtalk audit", &id)
                .map_err(|_| "Audit keychain unavailable")?;
            match entry.get_password() {
                Ok(value) => Ok(value.into_bytes()),
                Err(keyring::Error::NoEntry) => {
                    let value = format!("{}{}", uuid::Uuid::new_v4(), uuid::Uuid::new_v4());
                    entry
                        .set_password(&value)
                        .map_err(|_| "Unlock the keychain to protect the audit log")?;
                    Ok(value.into_bytes())
                }
                Err(_) => Err("Unlock the keychain to protect the audit log"),
            }
        })
        .await
        .map_err(|_| "Audit worker stopped")?
        .map_err(str::to_owned)?;
        KEYS.lock()
            .map_err(|_| "Audit key cache unavailable")?
            .insert(cache_id, secret.clone());
        Ok(secret)
    }
}

/// Protected operation access must be recorded before sensitive data is read or
/// changed. A locked credential store or failed audit write fails closed.
pub async fn preflight(pool: &SqlitePool, command: &str, object: &str) -> Result<(), String> {
    if sensitive(command) && super::privacy::policy(pool).await?.protected_mode {
        append(pool, &format!("requested:{command}"), object, true).await?;
    }
    Ok(())
}

pub fn object_id(args: &serde_json::Value) -> String {
    // Never log arbitrary argument text: names, tokens and transcript content
    // are all valid command arguments. Only a UUID is retained as an object ID.
    args.get("meetingId")
        .or_else(|| args.get("meeting_id"))
        .and_then(|v| v.as_str())
        .and_then(|v| uuid::Uuid::parse_str(v).ok())
        .map(|id| id.to_string())
        .unwrap_or_default()
}

pub async fn record_outcome(
    pool: &SqlitePool,
    command: &str,
    object: &str,
    success: bool,
) -> Result<(), String> {
    // Privacy changes and erasures append their own events in every mode.
    // Per-access logging is an explicit protected-mode control, avoiding a
    // write for every ordinary UI read on a personal, unprotected library.
    if !super::privacy::policy(pool).await?.protected_mode {
        return Ok(());
    }
    match append(pool, command, object, success).await {
        Ok(()) => Ok(()),
        Err(error) if super::privacy::policy(pool).await?.protected_mode => Err(error),
        Err(_) => {
            log::warn!("Security event could not be written; check the OS keychain");
            Ok(())
        }
    }
}

fn signature(
    secret: &[u8],
    at: &str,
    actor: &str,
    operation: &str,
    object: &str,
    outcome: &str,
    previous: &str,
) -> String {
    let bytes = serde_json::to_vec(&(at, actor, operation, object, outcome, previous)).unwrap();
    hmac::sign(&hmac::Key::new(hmac::HMAC_SHA256, secret), &bytes)
        .as_ref()
        .iter()
        .map(|v| format!("{v:02x}"))
        .collect()
}

pub async fn append(
    pool: &SqlitePool,
    operation: &str,
    object: &str,
    success: bool,
) -> Result<(), String> {
    if operation.len() > 80 || object.len() > 200 {
        return Err("Invalid audit event".into());
    }
    let _guard = WRITES.lock().await;
    let key = key(pool).await?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|_| "Audit storage unavailable")?;
    let previous: Option<String> =
        sqlx::query_scalar("SELECT event_hash FROM privacy_events ORDER BY sequence DESC LIMIT 1")
            .fetch_optional(&mut *tx)
            .await
            .map_err(|_| "Audit storage unavailable")?;
    let previous = previous.unwrap_or_default();
    let at = chrono::Utc::now().to_rfc3339();
    let actor = "local-os-session";
    let outcome = if success { "completed" } else { "failed" };
    let signature = signature(&key, &at, actor, operation, object, outcome, &previous);
    sqlx::query("INSERT INTO privacy_events(occurred_at,actor,operation,object_id,outcome,previous_hash,event_hash) VALUES(?,?,?,?,?,?,?)")
        .bind(at).bind(actor).bind(operation).bind(object).bind(outcome).bind(previous).bind(signature)
        .execute(&mut *tx).await.map_err(|_|"Could not write audit event")?;
    tx.commit()
        .await
        .map_err(|_| "Could not save audit event")?;
    Ok(())
}

pub async fn verify(pool: &SqlitePool) -> Result<bool, String> {
    let _guard = WRITES.lock().await;
    let key = key(pool).await?;
    let mut rows=sqlx::query("SELECT occurred_at,actor,operation,object_id,outcome,previous_hash,event_hash FROM privacy_events ORDER BY sequence")
        .fetch_all(pool).await.map_err(|_|"Could not read audit events")?;
    let mut previous = None;
    for row in rows.drain(..) {
        let string = |name| row.get::<String, _>(name);
        let prior = string("previous_hash");
        if previous.as_ref().is_some_and(|p| p != &prior) {
            return Ok(false);
        }
        let expected = signature(
            &key,
            &string("occurred_at"),
            &string("actor"),
            &string("operation"),
            &string("object_id"),
            &string("outcome"),
            &prior,
        );
        let actual = string("event_hash");
        use subtle::ConstantTimeEq;
        if !bool::from(expected.as_bytes().ct_eq(actual.as_bytes())) {
            return Ok(false);
        }
        previous = Some(actual);
    }
    Ok(true)
}

pub fn sensitive(command: &str) -> bool {
    // Audit new commands by default. Only content-free high-frequency health,
    // timing and device queries are exempt; do not exempt whole namespaces.
    !matches!(
        command,
        "native_audio"
            | "is_recording"
            | "get_transcription_status"
            | "is_retranscription_in_progress_command"
            | "is_import_in_progress_command"
            | "workspace_recording_health"
            | "workspace_release_audio"
            | "get_parallel_processing_status"
            | "get_system_resources"
            | "check_resource_constraints"
            | "calculate_optimal_workers"
            | "ui_log_diagnostics"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn access_is_audited_by_default_without_recording_arbitrary_arguments() {
        for command in [
            "api_get_summary",
            "workspace_project",
            "workspace_privacy",
            "future_command",
            "read_audio_file",
        ] {
            assert!(sensitive(command));
        }
        assert!(!sensitive("get_transcription_status"));
        assert_eq!(
            object_id(&serde_json::json!({"meetingId":"private text or token"})),
            ""
        );
    }
    #[tokio::test]
    async fn signed_events_detect_mutation_without_saving_content() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        append(&pool, "export", "meeting-id", true).await.unwrap();
        assert!(verify(&pool).await.unwrap());
        sqlx::query("UPDATE privacy_events SET outcome='changed'")
            .execute(&pool)
            .await
            .unwrap();
        assert!(!verify(&pool).await.unwrap());
    }
}
