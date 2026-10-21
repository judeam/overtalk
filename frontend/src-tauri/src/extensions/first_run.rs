//! The first meeting: both model downloads start together, a meeting can be recorded
//! while they run, and its saved audio is transcribed when the speech model arrives.
//! A recording made meanwhile waits in the queue; it never fails for a missing model.
use crate::database::repositories::setting::SettingsRepository;
use serde_json::{json, Value};
use sqlx::SqlitePool;
use tauri::{AppHandle, Manager};
use tokio_util::sync::CancellationToken;

/// The notes model a first run starts with: about 1.2 GB, so the first notes arrive
/// soon on any supported computer. A larger model can be chosen later in Notes settings.
pub const STARTER_NOTES_MODEL: &str = "qwen3.5:2b";
/// Download size of the default speech model, as listed in Speech settings.
const SPEECH_MODEL_MB: u64 = 670;

/// A model's name as people read it, for the setup checklist.
pub fn label(model: &str) -> String {
    match model {
        crate::config::DEFAULT_PARAKEET_MODEL => "Parakeet TDT 0.6B v3".into(),
        "qwen3.5:2b" => "Qwen 3.5 2B".into(),
        "qwen3.5:4b" => "Qwen 3.5 4B".into(),
        "gemma3:1b" => "Gemma 3 1B".into(),
        other => other.into(),
    }
}

/// Whether the configured speech engine can transcribe a saved recording now.
#[derive(Clone, Debug, PartialEq)]
pub enum Speech {
    Ready,
    Downloading(u8),
    Missing,
    /// A cloud engine: it transcribes live calls but not saved recordings.
    Elsewhere(String),
}

/// The configured speech provider and model, and their state.
pub async fn speech(pool: &SqlitePool) -> (String, String, Speech) {
    let (provider, model) = SettingsRepository::get_transcript_config(pool)
        .await
        .ok()
        .flatten()
        .map(|c| (c.provider, c.model))
        .unwrap_or_else(|| {
            (
                "parakeet".into(),
                crate::config::DEFAULT_PARAKEET_MODEL.into(),
            )
        });
    let state = match provider.as_str() {
        "parakeet" => {
            use crate::parakeet_engine::{commands, ModelStatus};
            match commands::parakeet_init().await {
                Err(_) => Speech::Missing,
                Ok(()) => commands::parakeet_get_available_models()
                    .await
                    .unwrap_or_default()
                    .into_iter()
                    .find(|m| m.name == model)
                    .map_or(Speech::Missing, |m| match m.status {
                        ModelStatus::Available => Speech::Ready,
                        ModelStatus::Downloading { progress } => Speech::Downloading(progress),
                        _ => Speech::Missing,
                    }),
            }
        }
        "localWhisper" => {
            use crate::whisper_engine::{commands, ModelStatus};
            match commands::whisper_init().await {
                Err(_) => Speech::Missing,
                Ok(()) => commands::whisper_get_available_models()
                    .await
                    .unwrap_or_default()
                    .into_iter()
                    .find(|m| m.name == model)
                    .map_or(Speech::Missing, |m| match m.status {
                        ModelStatus::Available => Speech::Ready,
                        ModelStatus::Downloading { progress } => Speech::Downloading(progress),
                        _ => Speech::Missing,
                    }),
            }
        }
        // The local speech worker reports its own problems when it runs.
        "localSpeech" => Speech::Ready,
        other => Speech::Elsewhere(other.into()),
    };
    (provider, model, state)
}

/// What a queued transcription shows while it waits for the speech model.
pub fn waiting_detail(state: &Speech) -> Option<String> {
    match state {
        // Parakeet's model list reports 0% throughout; its progress events carry the figure.
        Speech::Downloading(0) => Some("Waiting for the speech model to finish downloading".into()),
        Speech::Downloading(progress) => Some(format!(
            "Waiting for the speech model to finish downloading ({progress}%)"
        )),
        Speech::Missing => {
            Some("Waiting for a speech model. Download one in Transcription settings.".into())
        }
        _ => None,
    }
}

/// Transcribe a saved recording with the configured speech engine.
pub async fn transcribe_saved(
    app: &AppHandle,
    pool: &SqlitePool,
    meeting: &str,
    job_id: &str,
    token: &CancellationToken,
) -> Result<(), String> {
    let (provider, model, state) = speech(pool).await;
    match state {
        Speech::Elsewhere(name) => Err(format!(
            "{name} transcribes live calls only. Choose a local speech engine in Transcription settings to transcribe this recording."
        )),
        Speech::Missing | Speech::Downloading(_) => {
            Err(waiting_detail(&state).unwrap_or_default())
        }
        Speech::Ready if provider == "localSpeech" => super::speech::analyze_with_token(
            app.clone(),
            app.state(),
            meeting.into(),
            job_id.into(),
            true,
            None,
            token.clone(),
        )
        .await
        .map(|_| ()),
        Speech::Ready => {
            let folder: Option<String> =
                sqlx::query_scalar("SELECT folder_path FROM meetings WHERE id=?")
                    .bind(meeting)
                    .fetch_optional(pool)
                    .await
                    .map_err(|e| e.to_string())?
                    .flatten();
            let folder = folder.ok_or("This meeting has no stored recording")?;
            // The job's cancel button stops the transcription it started.
            let cancel = token.clone();
            let watcher = tauri::async_runtime::spawn(async move {
                cancel.cancelled().await;
                crate::audio::retranscription::cancel_retranscription();
            });
            let result = crate::audio::retranscription::start_retranscription(
                app.clone(),
                meeting.into(),
                folder,
                None,
                Some(model),
                Some(provider),
            )
            .await;
            watcher.abort();
            result.map(|_| ()).map_err(|e| e.to_string())
        }
    }
}

/// Start the downloads a first meeting needs: the configured local speech model, and a
/// notes model unless one is configured or already downloaded. Returns what started.
pub async fn start_downloads(app: &AppHandle, pool: &SqlitePool) -> Result<Value, String> {
    let (provider, model, state) = speech(pool).await;
    let mut started = Vec::new();
    let speech = if state == Speech::Missing
        && matches!(provider.as_str(), "parakeet" | "localWhisper")
    {
        if SettingsRepository::get_transcript_config(pool)
            .await
            .map_err(|e| e.to_string())?
            .is_none()
        {
            SettingsRepository::save_transcript_config(pool, &provider, &model)
                .await
                .map_err(|e| e.to_string())?;
        }
        let (download, app_handle, name) = (provider.clone(), app.clone(), model.clone());
        let pool_handle = pool.clone();
        tauri::async_runtime::spawn(async move {
            let result = if download == "parakeet" {
                crate::parakeet_engine::commands::parakeet_download_model(app_handle, name).await
            } else {
                crate::whisper_engine::commands::whisper_download_model(app_handle, name).await
            };
            match result {
                Ok(()) => super::usage::count(&pool_handle, "speech_model_ready").await,
                Err(error) => log::warn!("Speech model download did not finish: {error}"),
            }
        });
        started.push("speech");
        json!({"model":model,"label":label(&model),"size_mb":(provider == "parakeet").then_some(SPEECH_MODEL_MB),"state":"downloading"})
    } else {
        json!({"model":model,"state":match state {Speech::Ready=>"ready",Speech::Downloading(_)=>"downloading",Speech::Missing=>"missing",Speech::Elsewhere(_)=>"cloud"}})
    };
    let config = SettingsRepository::get_model_config(pool)
        .await
        .map_err(|e| e.to_string())?;
    let models =
        crate::summary::summary_engine::commands::builtin_ai_list_models(app.clone(), app.state())
            .await
            .unwrap_or_default();
    let status = |name: &str| {
        models
            .iter()
            .find(|m| m.name == name)
            .map(|m| m.status.clone())
    };
    use crate::summary::summary_engine::model_manager::ModelStatus;
    let local = |provider: &str| matches!(provider, "builtin-ai" | "local-llama" | "localllama");
    let notes = match &config {
        // A configured cloud or Ollama provider needs no download.
        Some(c) if !local(&c.provider) && !c.model.is_empty() => {
            json!({"model":c.model,"state":"configured"})
        }
        Some(c) if matches!(status(&c.model), Some(ModelStatus::Available)) => {
            json!({"model":c.model,"state":"ready"})
        }
        Some(c) if matches!(status(&c.model), Some(ModelStatus::Downloading { .. })) => {
            json!({"model":c.model,"state":"downloading"})
        }
        _ => {
            let whisper = config.as_ref().map_or("base", |c| c.whisper_model.as_str());
            SettingsRepository::save_model_config(
                pool,
                "builtin-ai",
                STARTER_NOTES_MODEL,
                whisper,
                None,
            )
            .await
            .map_err(|e| e.to_string())?;
            let (app_handle, pool_handle) = (app.clone(), pool.clone());
            tauri::async_runtime::spawn(async move {
                let state = app_handle
                    .state::<crate::summary::summary_engine::commands::ModelManagerState>();
                match crate::summary::summary_engine::commands::builtin_ai_download_model(
                    app_handle.clone(),
                    state,
                    STARTER_NOTES_MODEL.into(),
                )
                .await
                {
                    Ok(()) => super::usage::count(&pool_handle, "notes_model_ready").await,
                    Err(error) => log::warn!("Notes model download did not finish: {error}"),
                }
            });
            started.push("notes");
            let size = models
                .iter()
                .find(|m| m.name == STARTER_NOTES_MODEL)
                .map(|m| m.size_mb);
            json!({"model":STARTER_NOTES_MODEL,"label":label(STARTER_NOTES_MODEL),"size_mb":size,"state":"downloading"})
        }
    };
    if !started.is_empty() {
        super::usage::count(pool, "models_download_started").await;
    }
    Ok(json!({"speech":speech,"notes":notes,"started":started}))
}

/// The notice for a recording that starts before the speech model is downloaded: the audio
/// is kept and transcribed when the model is ready, so it is not an error.
pub fn missing_model_notice(error: &str) -> Option<super::gpu::SpeechNotice> {
    let lower = error.to_ascii_lowercase();
    (lower.contains("not downloaded")
        || lower.contains("currently downloading")
        || lower.contains("no parakeet models are available"))
    .then(|| super::gpu::SpeechNotice {
        reason: "model-pending".into(),
        message: "The speech model is still downloading. Overtalk keeps the audio and transcribes it as soon as the model is ready.".into(),
        detail: String::new(),
        can_free: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transcription_waits_only_while_the_model_is_missing_or_downloading() {
        assert!(waiting_detail(&Speech::Downloading(40))
            .unwrap()
            .contains("40%"));
        assert!(!waiting_detail(&Speech::Downloading(0))
            .unwrap()
            .contains('%'));
        assert!(waiting_detail(&Speech::Missing).is_some());
        assert!(waiting_detail(&Speech::Ready).is_none());
        assert!(waiting_detail(&Speech::Elsewhere("deepgram".into())).is_none());
    }

    #[test]
    fn a_missing_model_is_explained_not_reported_as_a_failure() {
        let notice = missing_model_notice("Model 'parakeet-tdt-0.6b-v3-int8' is not downloaded. Please download it first from the settings.").unwrap();
        assert_eq!(notice.reason, "model-pending");
        assert!(missing_model_notice("CUDA error: out of memory").is_none());
    }
}
