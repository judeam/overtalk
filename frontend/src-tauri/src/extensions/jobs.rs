//! A durable, bounded work queue. Only one queued heavy job runs; active
//! recording defers the next job. Interrupted work requires an explicit retry.
use crate::state::AppState;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{FromRow, SqlitePool};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    LazyLock,
};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio_util::sync::CancellationToken;
static STARTED: AtomicBool = AtomicBool::new(false);
static CANCEL: LazyLock<tokio::sync::Mutex<Option<(String, CancellationToken)>>> =
    LazyLock::new(|| tokio::sync::Mutex::new(None));
#[derive(Serialize, FromRow)]
pub struct Job {
    pub id: String,
    pub meeting_id: Option<String>,
    pub kind: String,
    pub state: String,
    pub progress: f64,
    pub detail: String,
    pub payload: String,
    pub created_at: String,
    pub updated_at: String,
}
#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum JobAction {
    List,
    Submit {
        kind: String,
        meeting_id: String,
        language: Option<String>,
    },
    Cancel {
        id: String,
    },
    Retry {
        id: String,
    },
    Pause {
        paused: bool,
    },
    Resources {
        low_resource: bool,
    },
}
async fn update(pool: &SqlitePool, id: &str, state: &str, detail: &str) -> Result<(), String> {
    sqlx::query("UPDATE workspace_jobs SET state=?,detail=?,progress=?,updated_at=? WHERE id=?")
        .bind(state)
        .bind(detail)
        .bind(if state == "completed" { 1. } else { 0. })
        .bind(chrono::Utc::now().to_rfc3339())
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}
static PAUSED: AtomicBool = AtomicBool::new(false);
static LOW_RESOURCE: AtomicBool = AtomicBool::new(false);
pub(crate) fn low_resource() -> bool {
    LOW_RESOURCE.load(Ordering::Relaxed)
}
pub fn start(app: AppHandle) {
    if STARTED.swap(true, Ordering::AcqRel) {
        return;
    }
    if let Ok(path) = app.path().app_data_dir() {
        if let Ok(bytes) =
            crate::security::storage::read_limited(&path.join("resource-preferences.json"), 8192)
        {
            if let Ok(value) = serde_json::from_slice::<Value>(&bytes) {
                LOW_RESOURCE.store(value["low_resource"] == true, Ordering::Relaxed);
            }
        }
    }
    tauri::async_runtime::spawn(async move {
        let pool = app.state::<AppState>().db_manager.pool().clone();
        let _ = sqlx::query(
            "UPDATE workspace_jobs SET state='interrupted',\
             detail='App closed before completion. Retry when ready.' WHERE state='running'",
        )
        .execute(&pool)
        .await;
        // Safe automatic jobs resume from their durable input. Explicit manual
        // jobs retain the existing interrupted-and-retry policy.
        let _ = sqlx::query(
            "UPDATE workspace_jobs SET state='queued',detail='Resuming automatic notes' \
             WHERE state='interrupted' AND json_extract(payload,'$.automatic')=1",
        )
        .execute(&pool)
        .await;
        // A killed summary process must not block the queue forever.
        let _ = sqlx::query(
            "UPDATE summary_processes SET status='failed',error='App closed during generation' \
             WHERE LOWER(status) IN ('pending','running','processing')",
        )
        .execute(&pool)
        .await;
        // The speech model's state, rechecked at most every ten seconds while work waits on it.
        let mut speech_checked: Option<(std::time::Instant, super::first_run::Speech)> = None;
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            if PAUSED.load(Ordering::Relaxed)
                || crate::audio::recording_commands::is_recording().await
                || super::speech::is_processing().await
            {
                continue;
            }
            // The empty queue is the normal idle state. Use its existing state
            // index before scanning summary history or issuing a write query.
            // Recheck on every tick, so work inserted concurrently is picked up
            // on the next tick without a lost notification.
            let queued: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM workspace_jobs WHERE state='queued')",
            )
            .fetch_one(&pool)
            .await
            .unwrap_or(false);
            if !queued {
                continue;
            }
            let summaries: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM summary_processes WHERE LOWER(status) IN \
                 ('processing','pending','running')",
            )
            .fetch_one(&pool)
            .await
            .unwrap_or(0);
            if summaries > 0 {
                continue;
            }
            let _ = sqlx::query(
                "UPDATE workspace_jobs SET \
                 state='failed',detail='Transcription did not finish. Retry to create notes.' \
                 WHERE state='queued' AND json_extract(payload,'$.depends_on') IN (SELECT id \
                 FROM workspace_jobs WHERE state IN ('failed','cancelled','interrupted'))",
            )
            .execute(&pool)
            .await;
            let candidates = sqlx::query_as::<_, Job>(
                "SELECT * FROM workspace_jobs WHERE state='queued' \
                 AND (json_extract(payload,'$.depends_on') IS NULL \
                 OR json_extract(payload,'$.depends_on') IN (SELECT id FROM workspace_jobs \
                 WHERE state='completed')) ORDER BY created_at,id LIMIT 20",
            )
            .fetch_all(&pool)
            .await
            .unwrap_or_default();
            // A transcription waits, queued, while the speech model downloads; other work
            // goes ahead. It starts on the first tick after the model is ready.
            let mut speech = None;
            let mut next = None;
            for job in candidates {
                if job.kind == "transcribe" {
                    if speech.is_none() {
                        let recent = speech_checked
                            .as_ref()
                            .filter(|(at, _)| at.elapsed() < std::time::Duration::from_secs(10))
                            .map(|(_, state)| state.clone());
                        let state = match recent {
                            Some(state) => state,
                            None => {
                                let state = super::first_run::speech(&pool).await.2;
                                speech_checked = Some((std::time::Instant::now(), state.clone()));
                                state
                            }
                        };
                        speech = Some(state);
                    }
                    if let Some(waiting) =
                        speech.as_ref().and_then(super::first_run::waiting_detail)
                    {
                        if job.detail != waiting {
                            let _ = update(&pool, &job.id, "queued", &waiting).await;
                            let _ = app.emit("workspace-jobs-changed", ());
                        }
                        continue;
                    }
                }
                next = Some(job);
                break;
            }
            let Some(job) = next else {
                continue;
            };
            let token = CancellationToken::new();
            *CANCEL.lock().await = Some((job.id.clone(), token.clone()));
            // Claim with a compare-and-set so cancellation cannot resurrect it.
            let claimed = sqlx::query(
                "UPDATE workspace_jobs SET state='running',detail='Processing',updated_at=? \
                 WHERE id=? AND state='queued'",
            )
            .bind(chrono::Utc::now().to_rfc3339())
            .bind(&job.id)
            .execute(&pool)
            .await;
            if !claimed.is_ok_and(|r| r.rows_affected() == 1) {
                *CANCEL.lock().await = None;
                continue;
            }
            let detail = match job.kind.as_str() {
                "translate" => "Translating the transcript in aligned batches",
                "learn" => "Checking source passages and proposing project facts",
                "transcribe" => "Transcribing audio and identifying speakers",
                "notes" => "Generating notes from the meeting transcript",
                _ => "Processing",
            };
            let _ = update(&pool, &job.id, "running", detail).await;
            let _ = app.emit("workspace-jobs-changed", ());
            let result = run(&app, &pool, &job, &token).await;
            let (state, detail) = if token.is_cancelled() {
                ("cancelled", "Cancelled".to_owned())
            } else {
                match result {
                    Ok(()) => ("completed", "Completed".into()),
                    Err(e) => ("failed", e),
                }
            };
            let _ = update(&pool, &job.id, state, &detail).await;
            *CANCEL.lock().await = None;
            let _ = app.emit(
                "workspace-jobs-changed",
                json!({"meeting_id":job.meeting_id,"kind":job.kind,"state":state}),
            );
        }
    });
}
async fn run(
    app: &AppHandle,
    pool: &SqlitePool,
    job: &Job,
    token: &CancellationToken,
) -> Result<(), String> {
    let meeting = job.meeting_id.as_deref().ok_or("Missing meeting")?;
    match job.kind.as_str() {
        "translate" => {
            let payload: Value = serde_json::from_str(&job.payload).map_err(|e| e.to_string())?;
            super::reading::translate(
                app,
                pool,
                meeting,
                payload["language"].as_str().unwrap_or("English"),
                token,
                &job.id,
            )
            .await?;
        }
        "learn" => {
            super::projects::learn(app, pool, meeting, token).await?;
        }
        "transcribe" => {
            super::first_run::transcribe_saved(app, pool, meeting, &job.id, token).await?;
            crate::addon::transcript_ready(app, meeting).await;
        }
        "notes" => {
            // An edition may file the meeting first, so notes get its context.
            crate::addon::transcript_ready(app, meeting).await;
            let payload: Value = serde_json::from_str(&job.payload).map_err(|e| e.to_string())?;
            if payload["automatic"] == true {
                let edited:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM summary_processes WHERE meeting_id=? AND json_extract(result,'$.manual')=1)").bind(meeting).fetch_one(pool).await.map_err(|e|e.to_string())?;
                if edited {
                    return Err(
                        "Your edited notes were preserved. Use Regenerate notes when ready.".into(),
                    );
                }
            }
            let started = crate::native_service::generate(
                app,
                json!({"meetingId":meeting,"templateId":payload["template_id"].as_str().unwrap_or("standard_meeting"),"summaryLanguage":payload["language"].as_str().unwrap_or("auto")}),
            )
            .await?;
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                let status: Option<(String, Option<String>)> =
                    sqlx::query_as("SELECT status,error FROM summary_processes WHERE meeting_id=?")
                        .bind(meeting)
                        .fetch_optional(pool)
                        .await
                        .map_err(|e| e.to_string())?;
                match status.as_ref().map(|s| s.0.as_str()) {
                    Some("completed") => break,
                    Some("failed" | "error" | "cancelled") => {
                        return Err(status
                            .and_then(|s| s.1)
                            .unwrap_or("Notes were not completed".into()))
                    }
                    _ => {}
                }
                if token.is_cancelled() {
                    crate::summary::commands::api_cancel_summary(
                        app.clone(),
                        app.state(),
                        meeting.into(),
                        started["process_id"].as_str().unwrap_or("").into(),
                    )
                    .await?;
                    return Err("Cancelled".into());
                }
            }
        }
        _ => return Err("Unsupported queued work".into()),
    }
    Ok(())
}
#[tauri::command]
pub async fn workspace_jobs(
    app: AppHandle,
    state: State<'_, AppState>,
    action: JobAction,
) -> Result<Value, String> {
    start(app.clone());
    let pool = state.db_manager.pool();
    match action {
        JobAction::List => Ok(json!(sqlx::query_as::<_, Job>(
            "SELECT * FROM workspace_jobs ORDER BY state IN ('running','queued') DESC,created_at \
             DESC LIMIT 200"
        )
        .fetch_all(pool)
        .await
        .map_err(|e| e.to_string())?)),
        JobAction::Submit {
            kind,
            meeting_id,
            language,
        } => {
            if !matches!(
                kind.as_str(),
                "translate" | "learn" | "transcribe" | "notes"
            ) {
                return Err("Unsupported job kind".into());
            }
            if language
                .as_ref()
                .is_some_and(|l| l.is_empty() || l.len() > 80)
            {
                return Err("Invalid language".into());
            }
            let count: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM workspace_jobs WHERE state IN ('running','queued')",
            )
            .fetch_one(pool)
            .await
            .map_err(|e| e.to_string())?;
            if count >= 30 {
                return Err("The queue holds up to 30 jobs. Let some finish first.".into());
            }
            let exists: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM workspace_jobs WHERE meeting_id=? AND kind=? \
                 AND state IN ('queued','running'))",
            )
            .bind(&meeting_id)
            .bind(&kind)
            .fetch_one(pool)
            .await
            .map_err(|e| e.to_string())?;
            if exists {
                return Err("This work is already queued for the meeting".into());
            }
            let id = uuid::Uuid::new_v4().to_string();
            let now = chrono::Utc::now().to_rfc3339();
            sqlx::query(
                "INSERT INTO workspace_jobs(id,meeting_id,kind,state,detail,payload,created_at,\
                 updated_at) VALUES(?,?,?,'queued','Waiting for a free processing slot',?,?,?)",
            )
            .bind(&id)
            .bind(meeting_id)
            .bind(kind)
            .bind(json!({"language": language}).to_string())
            .bind(&now)
            .bind(&now)
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;
            let _ = app.emit("workspace-jobs-changed", ());
            Ok(json!({"id":id}))
        }
        JobAction::Cancel { id } => {
            if let Some((active, token)) = CANCEL.lock().await.as_ref() {
                if active == &id {
                    token.cancel();
                    super::speech::speech_cancel(id.clone()).await;
                }
            }
            sqlx::query(
                "UPDATE workspace_jobs SET state='cancelled',detail='Cancelled' WHERE id=? \
                 AND state='queued'",
            )
            .bind(id)
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;
            Ok(Value::Null)
        }
        JobAction::Retry { id } => {
            let result = sqlx::query(
                "UPDATE workspace_jobs SET state='queued',detail='Retry queued',updated_at=? \
                 WHERE id=? AND state IN ('failed','interrupted','cancelled')",
            )
            .bind(chrono::Utc::now().to_rfc3339())
            .bind(id)
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;
            if result.rows_affected() == 0 {
                return Err("Only interrupted, cancelled or failed work can be retried".into());
            }
            Ok(Value::Null)
        }
        JobAction::Resources { low_resource } => {
            LOW_RESOURCE.store(low_resource, Ordering::Relaxed);
            let path = app
                .path()
                .app_data_dir()
                .map_err(|e| e.to_string())?
                .join("resource-preferences.json");
            super::integrations::write_private_json(&path, &json!({"low_resource":low_resource}))?;
            let _lifecycle = crate::audio::common::acquire_engine_lifecycle_lock().await;
            if !crate::audio::recording_commands::is_recording().await
                && !super::speech::is_processing().await
            {
                super::speech::shutdown().await;
            }
            Ok(json!({"low_resource":low_resource}))
        }
        JobAction::Pause { paused } => {
            PAUSED.store(paused, Ordering::Relaxed);
            Ok(json!({"paused": paused}))
        }
    }
}
