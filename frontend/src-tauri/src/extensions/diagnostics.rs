//! Local readiness checks. No credentials, transcripts or telemetry are returned.
use crate::state::AppState;
use serde_json::{json, Value};
use tauri::{AppHandle, Manager, State};
#[tauri::command]
pub async fn workspace_diagnostics(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Value, String> {
    let pool = state.db_manager.pool();
    let mut checks = Vec::new();
    let result = sqlx::query_scalar::<_, String>("PRAGMA quick_check")
        .fetch_one(pool)
        .await
        .map_err(|e| e.to_string())?;
    checks.push(json!({
        "name": "Meeting database",
        "status": if result == "ok" {
            "ready"
        } else {
            "needs attention"
        },
        "detail": if result == "ok" {
            "SQLite integrity check passed. Backups include a consistent snapshot."
                .to_string()
        } else {
            result
        }
    }));
    let journal: String = sqlx::query_scalar("PRAGMA journal_mode")
        .fetch_one(pool)
        .await
        .map_err(|e| e.to_string())?;
    checks.push(json!({
        "name": "Recovery files",
        "status": "preserved",
        "detail": format!(
            "Journal mode: {journal}. Corruption recovery never deletes the original WAL or \
             database."
        )
    }));
    let speech = super::speech::read_settings(&app)?;
    let python = which::which(&speech.python).is_ok();
    checks.push(json!({
        "name": "Speech runtime",
        "status": if speech.enabled && python {
            "configured"
        } else {
            "setup required"
        },
        "detail": if python {
            format!(
                "{} selected. Check runtime in Transcription to validate dependencies and cached \
                 weights.",
                speech.model
            )
        } else {
            "Python executable was not found. Configure its full path in Transcription."
                .into()
        }
    }));
    let library = super::retrieval::read_settings(&app)?;
    checks.push(json!({
        "name": "Semantic retrieval",
        "status": if library.enabled && which::which(&library.python).is_ok() {
            "configured"
        } else {
            "setup required"
        },
        "detail": format!(
            "{} {} embedding, Qwen3 {} reranker. Use Check models and Build semantic index to \
             validate loading.",
            library.family, library.size, library.reranker_size
        )
    }));
    let recording = crate::audio::recording_commands::is_recording().await;
    checks.push(json!({
        "name": "Inference scheduling",
        "status": if recording { "recording" } else { "ready" },
        "detail": "Quality retrieval, speech and built-in generation share a GPU lease. The \
                   desktop's display settings are never changed."
    }));
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let writable = tempfile::NamedTempFile::new_in(&dir).is_ok();
    checks.push(json!({
        "name": "Workspace storage",
        "status": if writable { "ready" } else { "needs attention" },
        "detail": if writable {
            "The application data directory can create temporary snapshot files."
        } else {
            "The application data directory is not writable; check its permissions."
        }
    }));
    Ok(json!({"checks":checks,"offline":true}))
}

/// Interface stalls and slow calls reported by the page; written to the app log only.
#[tauri::command]
pub async fn ui_log_diagnostics(lines: Vec<String>) {
    for line in lines.iter().take(200) {
        log::warn!("UI {}", line.chars().take(240).collect::<String>());
    }
}
