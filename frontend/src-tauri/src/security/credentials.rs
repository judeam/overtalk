//! Provider secrets are never persisted in SQLite. Legacy values are cleared
//! only after a verified keychain write, so a locked keychain cannot lose a key.
use sqlx::SqlitePool;
use tokio::sync::Mutex;
static OPERATIONS: Mutex<()> = Mutex::const_new(());

async fn namespace(pool: &SqlitePool) -> Result<String, sqlx::Error> {
    sqlx::query("CREATE TABLE IF NOT EXISTS privacy_settings(id INTEGER PRIMARY KEY CHECK(id=1), credential_namespace TEXT NOT NULL DEFAULT (lower(hex(randomblob(16)))), policy TEXT NOT NULL DEFAULT '{}')").execute(pool).await?;
    sqlx::query("INSERT OR IGNORE INTO privacy_settings(id) VALUES(1)")
        .execute(pool)
        .await?;
    sqlx::query_scalar("SELECT credential_namespace FROM privacy_settings WHERE id=1")
        .fetch_one(pool)
        .await
}

enum Operation {
    Read,
    Write(String),
}

async fn operate(account: String, op: Operation) -> Result<Option<String>, sqlx::Error> {
    tokio::task::spawn_blocking(move || backend(account, op))
        .await
        .map_err(|_| sqlx::Error::Protocol("Credential worker stopped".into()))?
        .map_err(sqlx::Error::Protocol)
}

#[cfg(not(test))]
fn backend(account: String, op: Operation) -> Result<Option<String>, String> {
    let entry = keyring::Entry::new("Overtalk providers", &account)
        .map_err(|_| "Cannot access the OS keychain")?;
    match op {
        Operation::Read => match entry.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err("Unlock your OS keychain to access the provider credential".into()),
        },
        Operation::Write(value) => {
            entry
                .set_password(&value)
                .map_err(|_| "Unlock your OS keychain to save the provider credential")?;
            Ok(None)
        }
    }
}

#[cfg(test)]
static TEST_KEYS: std::sync::LazyLock<std::sync::Mutex<std::collections::HashMap<String, String>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));
#[cfg(test)]
fn backend(account: String, op: Operation) -> Result<Option<String>, String> {
    if account.starts_with("locked-test/") {
        return Err("Keychain locked".into());
    }
    let mut keys = TEST_KEYS.lock().unwrap();
    match op {
        Operation::Read => Ok(keys.get(&account).cloned()),
        Operation::Write(value) => {
            keys.insert(account, value);
            Ok(None)
        }
    }
}

fn location(domain: &str, provider: &str) -> Result<(&'static str, &'static str), sqlx::Error> {
    let column = match (domain, provider) {
        ("summary", "openai") => ("settings", "openaiApiKey"),
        ("summary", "claude") => ("settings", "anthropicApiKey"),
        ("summary", "groq") => ("settings", "groqApiKey"),
        ("summary", "ollama") => ("settings", "ollamaApiKey"),
        ("summary", "openrouter") => ("settings", "openRouterApiKey"),
        ("transcript", "openai") => ("transcript_settings", "openaiApiKey"),
        ("transcript", "groq") => ("transcript_settings", "groqApiKey"),
        ("transcript", "deepgram") => ("transcript_settings", "deepgramApiKey"),
        ("transcript", "elevenLabs") => ("transcript_settings", "elevenLabsApiKey"),
        ("transcript", "localWhisper") => ("transcript_settings", "whisperApiKey"),
        ("summary", "custom-openai") => ("settings", "customOpenAIConfig"),
        _ => {
            return Err(sqlx::Error::Protocol(
                "Unsupported provider credential".into(),
            ))
        }
    };
    Ok(column)
}

async fn legacy(
    pool: &SqlitePool,
    domain: &str,
    provider: &str,
) -> Result<Option<String>, sqlx::Error> {
    let (table, column) = location(domain, provider)?;
    let value: Option<String> =
        sqlx::query_scalar(&format!("SELECT {column} FROM {table} WHERE id='1'"))
            .fetch_optional(pool)
            .await?
            .flatten();
    if provider == "custom-openai" {
        return Ok(value
            .and_then(|v| serde_json::from_str::<serde_json::Value>(&v).ok())
            .and_then(|v| {
                v.get("api_key")
                    .or_else(|| v.get("apiKey"))
                    .and_then(|v| v.as_str())
                    .map(str::to_owned)
            }));
    }
    Ok(value.filter(|v| !v.is_empty()))
}

async fn clear_legacy(pool: &SqlitePool, domain: &str, provider: &str) -> Result<(), sqlx::Error> {
    let (table, column) = location(domain, provider)?;
    if provider == "custom-openai" {
        // Preserve the endpoint/model while removing either historical key spelling.
        sqlx::query("UPDATE settings SET customOpenAIConfig=json_remove(customOpenAIConfig,'$.api_key','$.apiKey') WHERE id='1' AND json_valid(customOpenAIConfig)")
            .execute(pool).await?;
    } else {
        sqlx::query(&format!("UPDATE {table} SET {column}=NULL WHERE id='1'"))
            .execute(pool)
            .await?;
    }
    Ok(())
}

pub async fn get(
    pool: &SqlitePool,
    domain: &str,
    provider: &str,
) -> Result<Option<String>, sqlx::Error> {
    let _guard = OPERATIONS.lock().await;
    let account = format!("{}/{domain}/{provider}", namespace(pool).await?);
    let old = legacy(pool, domain, provider).await?;
    let current = operate(account.clone(), Operation::Read).await?;
    if let Some(value) = old {
        // An interrupted migration prefers the old DB value until verified and cleared.
        operate(account.clone(), Operation::Write(value.clone())).await?;
        if operate(account, Operation::Read).await?.as_deref() != Some(value.as_str()) {
            return Err(sqlx::Error::Protocol(
                "Credential verification failed; original retained".into(),
            ));
        }
        clear_legacy(pool, domain, provider).await?;
        Ok(Some(value))
    } else {
        Ok(current)
    }
}

pub async fn save(
    pool: &SqlitePool,
    domain: &str,
    provider: &str,
    value: &str,
) -> Result<(), sqlx::Error> {
    location(domain, provider)?;
    let _guard = OPERATIONS.lock().await;
    let account = format!("{}/{domain}/{provider}", namespace(pool).await?);
    operate(account.clone(), Operation::Write(value.into())).await?;
    if operate(account, Operation::Read).await?.as_deref() != Some(value) {
        return Err(sqlx::Error::Protocol(
            "Credential verification failed; original retained".into(),
        ));
    }
    clear_legacy(pool, domain, provider).await
}

pub async fn migrate(pool: &SqlitePool) -> Result<usize, sqlx::Error> {
    let mut count = 0;
    for (domain, providers) in [
        (
            "summary",
            &[
                "openai",
                "claude",
                "groq",
                "ollama",
                "openrouter",
                "custom-openai",
            ][..],
        ),
        (
            "transcript",
            &["openai", "groq", "deepgram", "elevenLabs", "localWhisper"][..],
        ),
    ] {
        for provider in providers {
            if legacy(pool, domain, provider).await?.is_some() {
                get(pool, domain, provider).await?;
                count += 1;
            }
        }
    }
    Ok(count)
}

pub async fn pending_count(pool: &SqlitePool) -> Result<i64, sqlx::Error> {
    let summary:i64=sqlx::query_scalar("SELECT (openaiApiKey IS NOT NULL)+(anthropicApiKey IS NOT NULL)+(groqApiKey IS NOT NULL)+(ollamaApiKey IS NOT NULL)+(openRouterApiKey IS NOT NULL) FROM settings WHERE id='1'").fetch_optional(pool).await?.unwrap_or(0);
    let transcript:i64=sqlx::query_scalar("SELECT (openaiApiKey IS NOT NULL)+(groqApiKey IS NOT NULL)+(deepgramApiKey IS NOT NULL)+(elevenLabsApiKey IS NOT NULL)+(whisperApiKey IS NOT NULL) FROM transcript_settings WHERE id='1'").fetch_optional(pool).await?.unwrap_or(0);
    Ok(summary + transcript + legacy(pool, "summary", "custom-openai").await?.is_some() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;
    async fn database() -> SqlitePool {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE settings(id TEXT PRIMARY KEY,openaiApiKey TEXT,customOpenAIConfig TEXT)",
        )
        .execute(&pool)
        .await
        .unwrap();
        pool
    }
    #[tokio::test]
    async fn migration_preserves_secret_until_verified_and_isolated_per_library() {
        let a = database().await;
        let b = database().await;
        sqlx::query("INSERT INTO settings VALUES('1','test-secret',NULL)")
            .execute(&a)
            .await
            .unwrap();
        assert_eq!(
            get(&a, "summary", "openai").await.unwrap().as_deref(),
            Some("test-secret")
        );
        let raw: Option<String> = sqlx::query_scalar("SELECT openaiApiKey FROM settings")
            .fetch_one(&a)
            .await
            .unwrap();
        assert!(raw.is_none());
        assert_eq!(get(&b, "summary", "openai").await.unwrap(), None);
    }
    #[tokio::test]
    async fn locked_keychain_does_not_clear_legacy_secret() {
        let pool = database().await;
        namespace(&pool).await.unwrap();
        sqlx::query("UPDATE privacy_settings SET credential_namespace='locked-test'")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO settings VALUES('1','survives-failure',NULL)")
            .execute(&pool)
            .await
            .unwrap();
        assert!(get(&pool, "summary", "openai").await.is_err());
        let raw: String = sqlx::query_scalar("SELECT openaiApiKey FROM settings")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(raw, "survives-failure");
    }
}
