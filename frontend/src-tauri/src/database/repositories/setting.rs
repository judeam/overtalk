use crate::database::models::{Setting, TranscriptSetting};
use crate::summary::CustomOpenAIConfig;
use sqlx::SqlitePool;

#[derive(serde::Deserialize, Debug)]
pub struct SaveModelConfigRequest {
    pub provider: String,
    pub model: String,
    #[serde(rename = "whisperModel")]
    pub whisper_model: String,
    #[serde(rename = "apiKey")]
    pub api_key: Option<String>,
    #[serde(rename = "ollamaEndpoint")]
    pub ollama_endpoint: Option<String>,
}

#[derive(serde::Deserialize, Debug)]
pub struct SaveTranscriptConfigRequest {
    pub provider: String,
    pub model: String,
    #[serde(rename = "apiKey")]
    pub api_key: Option<String>,
}

pub struct SettingsRepository;

// Transcript providers: localWhisper, parakeet, localSpeech, deepgram, elevenLabs, groq, openai
// Summary providers: openai, claude, ollama, groq, added openrouter
// NOTE: Handle data exclusion in the higher layer as this is database abstraction layer(using SELECT *)

impl SettingsRepository {
    pub async fn get_model_config(
        pool: &SqlitePool,
    ) -> std::result::Result<Option<Setting>, sqlx::Error> {
        let setting = sqlx::query_as::<_, Setting>("SELECT * FROM settings LIMIT 1")
            .fetch_optional(pool)
            .await?;
        Ok(setting)
    }

    pub async fn save_model_config(
        pool: &SqlitePool,
        provider: &str,
        model: &str,
        whisper_model: &str,
        ollama_endpoint: Option<&str>,
    ) -> std::result::Result<(), sqlx::Error> {
        // Using id '1' for backward compatibility
        sqlx::query(
            r#"
            INSERT INTO settings (id, provider, model, whisperModel, ollamaEndpoint)
            VALUES ('1', $1, $2, $3, $4)
            ON CONFLICT(id) DO UPDATE SET
                provider = excluded.provider,
                model = excluded.model,
                whisperModel = excluded.whisperModel,
                ollamaEndpoint = excluded.ollamaEndpoint
            "#,
        )
        .bind(provider)
        .bind(model)
        .bind(whisper_model)
        .bind(ollama_endpoint)
        .execute(pool)
        .await?;

        Ok(())
    }

    pub async fn save_api_key(
        pool: &SqlitePool,
        provider: &str,
        api_key: &str,
    ) -> std::result::Result<(), sqlx::Error> {
        if provider == "privatemode" {
            return crate::summary::privatemode::key(Some(api_key.into()))
                .await
                .map(|_| ())
                .map_err(sqlx::Error::Protocol);
        }
        if provider == "builtin-ai" {
            return Ok(());
        }
        crate::security::credentials::save(pool, "summary", provider, api_key).await
    }

    pub async fn get_api_key(
        pool: &SqlitePool,
        provider: &str,
    ) -> std::result::Result<Option<String>, sqlx::Error> {
        if provider == "privatemode" {
            return crate::summary::privatemode::key(None)
                .await
                .map_err(sqlx::Error::Protocol);
        }
        if provider == "builtin-ai" {
            return Ok(None);
        }
        crate::security::credentials::get(pool, "summary", provider).await
    }

    pub async fn get_transcript_config(
        pool: &SqlitePool,
    ) -> std::result::Result<Option<TranscriptSetting>, sqlx::Error> {
        let setting =
            sqlx::query_as::<_, TranscriptSetting>("SELECT * FROM transcript_settings LIMIT 1")
                .fetch_optional(pool)
                .await?;
        Ok(setting)
    }

    pub async fn save_transcript_config(
        pool: &SqlitePool,
        provider: &str,
        model: &str,
    ) -> std::result::Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            INSERT INTO transcript_settings (id, provider, model)
            VALUES ('1', $1, $2)
            ON CONFLICT(id) DO UPDATE SET
                provider = excluded.provider,
                model = excluded.model
            "#,
        )
        .bind(provider)
        .bind(model)
        .execute(pool)
        .await?;

        Ok(())
    }

    pub async fn save_transcript_api_key(
        pool: &SqlitePool,
        provider: &str,
        api_key: &str,
    ) -> std::result::Result<(), sqlx::Error> {
        if matches!(provider, "parakeet" | "localSpeech") {
            return Ok(());
        }
        crate::security::credentials::save(pool, "transcript", provider, api_key).await
    }

    pub async fn get_transcript_api_key(
        pool: &SqlitePool,
        provider: &str,
    ) -> std::result::Result<Option<String>, sqlx::Error> {
        if matches!(provider, "parakeet" | "localSpeech") {
            return Ok(None);
        }
        crate::security::credentials::get(pool, "transcript", provider).await
    }

    pub async fn delete_api_key(
        pool: &SqlitePool,
        provider: &str,
    ) -> std::result::Result<(), sqlx::Error> {
        if provider == "privatemode" {
            return crate::summary::privatemode::delete_key()
                .await
                .map_err(sqlx::Error::Protocol);
        }
        if provider == "builtin-ai" {
            return Ok(());
        }
        crate::security::credentials::delete(pool, "summary", provider).await
    }

    // ===== CUSTOM OPENAI CONFIG METHODS =====

    /// Gets the custom OpenAI configuration from JSON
    ///
    /// # Returns
    /// * `Ok(Some(CustomOpenAIConfig))` - Config exists and is valid JSON
    /// * `Ok(None)` - No config stored
    /// * `Err(sqlx::Error)` - Database error
    pub async fn get_custom_openai_config(
        pool: &SqlitePool,
    ) -> std::result::Result<Option<CustomOpenAIConfig>, sqlx::Error> {
        use sqlx::Row;

        let row = sqlx::query(
            r#"
            SELECT customOpenAIConfig
            FROM settings
            WHERE id = '1'
            LIMIT 1
            "#,
        )
        .fetch_optional(pool)
        .await?;

        match row {
            Some(record) => {
                let config_json: Option<String> = record.get("customOpenAIConfig");

                if let Some(json) = config_json {
                    // Parse JSON into CustomOpenAIConfig
                    let mut config: CustomOpenAIConfig =
                        serde_json::from_str(&json).map_err(|e| {
                            sqlx::Error::Protocol(format!(
                                "Invalid JSON in customOpenAIConfig: {}",
                                e
                            ))
                        })?;

                    config.api_key =
                        crate::security::credentials::get(pool, "summary", "custom-openai").await?;
                    Ok(Some(config))
                } else {
                    Ok(None)
                }
            }
            None => Ok(None),
        }
    }

    /// Saves the custom OpenAI configuration as JSON
    ///
    /// # Arguments
    /// * `pool` - Database connection pool
    /// * `config` - CustomOpenAIConfig to save (includes endpoint, apiKey, model, maxTokens, temperature, topP)
    ///
    /// # Returns
    /// * `Ok(())` - Config saved successfully
    /// * `Err(sqlx::Error)` - Database or JSON serialization error
    pub async fn save_custom_openai_config(
        pool: &SqlitePool,
        config: &CustomOpenAIConfig,
    ) -> std::result::Result<(), sqlx::Error> {
        crate::security::transport::endpoint(&config.endpoint).map_err(sqlx::Error::Protocol)?;
        if let Some(key) = config.api_key.as_ref().filter(|v| !v.is_empty()) {
            crate::security::credentials::save(pool, "summary", "custom-openai", key).await?;
        }
        let mut sanitized = config.clone();
        sanitized.api_key = None;
        let config_json = serde_json::to_string(&sanitized).map_err(|e| {
            sqlx::Error::Protocol(format!("Failed to serialize config to JSON: {}", e))
        })?;

        // Upsert into settings table
        sqlx::query(
            r#"
            INSERT INTO settings (id, provider, model, whisperModel, customOpenAIConfig)
            VALUES ('1', 'custom-openai', $1, 'large-v3', $2)
            ON CONFLICT(id) DO UPDATE SET
                customOpenAIConfig = excluded.customOpenAIConfig
            "#,
        )
        .bind(&config.model)
        .bind(config_json)
        .execute(pool)
        .await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::SettingsRepository;
    use sqlx::sqlite::SqlitePoolOptions;

    #[tokio::test]
    async fn local_speech_selection_round_trips_without_cloud_credentials() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::raw_sql(include_str!(
            "../../../migrations/20250916100000_initial_schema.sql"
        ))
        .execute(&pool)
        .await
        .unwrap();
        SettingsRepository::save_transcript_api_key(&pool, "groq", "fixture-cloud-key")
            .await
            .unwrap();
        for model in [
            "qwen3-asr-1.7b",
            "qwen3-asr-0.6b",
            "parakeet-tdt-0.6b-v3",
            "cohere-transcribe",
            "moss-transcribe-diarize",
        ] {
            SettingsRepository::save_transcript_config(&pool, "localSpeech", model)
                .await
                .unwrap();
            SettingsRepository::save_transcript_api_key(&pool, "localSpeech", "ignored")
                .await
                .unwrap();
            let saved = SettingsRepository::get_transcript_config(&pool)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(saved.provider, "localSpeech");
            assert_eq!(saved.model, model);
            // The API's reload path asks for a key for the persisted provider.
            assert!(
                SettingsRepository::get_transcript_api_key(&pool, &saved.provider)
                    .await
                    .unwrap()
                    .is_none()
            );
            assert!(
                saved.groq_api_key.is_none(),
                "credentials must not remain in SQLite"
            );
            assert_eq!(
                SettingsRepository::get_transcript_api_key(&pool, "groq")
                    .await
                    .unwrap()
                    .as_deref(),
                Some("fixture-cloud-key")
            );
        }
    }

    #[tokio::test]
    async fn local_provider_keys_need_no_settings_row_and_unknown_providers_fail() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        for provider in ["localSpeech", "parakeet"] {
            assert!(SettingsRepository::get_transcript_api_key(&pool, provider)
                .await
                .unwrap()
                .is_none());
            SettingsRepository::save_transcript_api_key(&pool, provider, "ignored")
                .await
                .unwrap();
        }
        assert!(
            SettingsRepository::get_transcript_api_key(&pool, "invalid-provider")
                .await
                .is_err()
        );
        assert!(
            SettingsRepository::save_transcript_api_key(&pool, "invalid-provider", "ignored")
                .await
                .is_err()
        );
    }
}
