//! Capture the engine's authoritative event stream, including the stop tail.
//! UI redraws, dropped transport events, and a closed window cannot lose a meeting.
use crate::api::TranscriptSegment;
use crate::database::repositories::transcript::TranscriptsRepository;
use crate::sync::LockRecovering;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeMap, io::Write, path::Path, sync::Mutex};
use tauri::{AppHandle, Emitter, Manager};

#[derive(Default)]
struct Session {
    id: String,
    segments: BTreeMap<u64, TranscriptSegment>,
}

impl Session {
    fn update(&mut self, value: &Value) {
        if self.id.is_empty() {
            return;
        }
        let Some(sequence) = value["sequence_id"].as_u64() else {
            return;
        };
        let Some(text) = value["text"].as_str() else {
            return;
        };
        let partial = value["is_partial"].as_bool().unwrap_or(false);
        if partial
            && self
                .segments
                .get(&sequence)
                .is_some_and(|old| !old.is_partial)
        {
            return;
        }
        self.segments.insert(
            sequence,
            TranscriptSegment {
                id: format!("seg_{sequence}"),
                text: text.into(),
                timestamp: value["timestamp"].as_str().unwrap_or("").into(),
                audio_start_time: value["audio_start_time"].as_f64(),
                audio_end_time: value["audio_end_time"].as_f64(),
                duration: value["duration"].as_f64(),
                speaker: value["speaker"].as_str().map(str::to_owned),
                speaker_id: value["speaker_id"].as_str().map(str::to_owned),
                speaker_name: value["speaker_name"].as_str().map(str::to_owned),
                speaker_confidence: value["speaker_confidence"].as_f64(),
                overlap: value["overlap"].as_bool().unwrap_or(false),
                is_partial: partial,
            },
        );
    }
}

#[derive(Serialize, Deserialize)]
struct SavedRecording {
    id: String,
    title: String,
    folder: Option<String>,
    segments: Vec<TranscriptSegment>,
}

#[derive(Default)]
pub(crate) struct Persistence {
    session: Mutex<Session>,
    jobs: Mutex<Vec<tauri::async_runtime::JoinHandle<Result<(), String>>>>,
    failed: Mutex<Vec<SavedRecording>>,
    completed: Mutex<Option<(String, bool)>>,
}

pub(crate) fn start(app: &AppHandle) {
    app.manage(Persistence::default());
}

pub(crate) fn recover_pending(app: &AppHandle) {
    let recovery_app = app.clone();
    let job = tauri::async_runtime::spawn(async move {
        let result = recover(&recovery_app).await;
        if result.is_err() {
            let _ = recovery_app.emit("recording-error", serde_json::json!({"message":"A previous recording still needs saving. Use Retry saving to recover it.","save_pending":true}));
        }
        result
    });
    app.state::<Persistence>().jobs.lock_recovering().push(job);
}

pub(crate) fn prepare(app: &AppHandle) -> bool {
    let persistence = app.state::<Persistence>();
    let mut session = persistence.session.lock_recovering();
    if !session.id.is_empty() {
        return false;
    }
    *persistence.completed.lock_recovering() = None;
    session.id = format!("meeting-{}", uuid::Uuid::new_v4());
    true
}
pub(crate) fn active_id(app: &AppHandle) -> Option<String> {
    let state = app.try_state::<Persistence>()?;
    let id = state.session.lock_recovering().id.clone();
    (!id.is_empty()).then_some(id)
}

pub(crate) fn abort_prepared(app: &AppHandle) {
    *app.state::<Persistence>().session.lock_recovering() = Session::default();
}

pub(crate) fn observe(app: &AppHandle, event: &str, value: &Value) {
    let persistence = app.state::<Persistence>();
    match event {
        "recording-started" => {
            prepare(app);
        }
        "transcript-update" => persistence.session.lock_recovering().update(value),
        "recording-stopped" => {
            let session = std::mem::take(&mut *persistence.session.lock_recovering());
            if session.id.is_empty() {
                return;
            }
            let recording = SavedRecording {
                id: session.id,
                title: value["meeting_name"]
                    .as_str()
                    .unwrap_or("Untitled meeting")
                    .into(),
                folder: value["folder_path"].as_str().map(str::to_owned),
                segments: session.segments.into_values().collect(),
            };
            let app = app.clone();
            let job = tauri::async_runtime::spawn(async move {
                let result = persist(&app, &recording).await;
                if result.is_err() {
                    app.state::<Persistence>()
                        .failed
                        .lock_recovering()
                        .push(recording);
                    let _ = app.emit("recording-error", serde_json::json!({"message":"The recording stopped, but saving failed. Keep Overtalk open and use Retry saving.","save_pending":true}));
                }
                result
            });
            let mut jobs = persistence.jobs.lock_recovering();
            jobs.retain(|job| !job.inner().is_finished());
            jobs.push(job);
        }
        _ => {}
    }
}

pub(crate) async fn retry(app: &AppHandle) -> Result<Value, String> {
    let failed = std::mem::take(&mut *app.state::<Persistence>().failed.lock_recovering());
    let mut error = None;
    for recording in failed {
        if let Err(e) = persist(app, &recording).await {
            app.state::<Persistence>()
                .failed
                .lock_recovering()
                .push(recording);
            error = Some(e);
        }
    }
    recover(app).await?;
    error.map_or(Ok(Value::Null), Err)
}

pub(crate) fn last_saved(app: &AppHandle) -> Value {
    app.state::<Persistence>().completed.lock_recovering().as_ref()
        .map(|(id, has_transcript)| serde_json::json!({"meeting_id":id,"has_transcript":has_transcript})).unwrap_or(Value::Null)
}

/// Called before a stop reply or process shutdown, never on the event-loop thread.
pub(crate) async fn finish(app: &AppHandle) -> Result<(), String> {
    let jobs = std::mem::take(&mut *app.state::<Persistence>().jobs.lock_recovering());
    let mut error = None;
    for job in jobs {
        match job.await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => error = Some(e),
            Err(e) => error = Some(e.to_string()),
        }
    }
    error.map_or(Ok(()), Err)
}

fn directory(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    let path = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("native-pending-recordings");
    std::fs::create_dir_all(&path).map_err(|e| e.to_string())?;
    Ok(path)
}

async fn save_database(pool: &sqlx::SqlitePool, recording: &SavedRecording) -> Result<(), String> {
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM meetings WHERE id=?)")
        .bind(&recording.id)
        .fetch_one(pool)
        .await
        .map_err(|e| e.to_string())?;
    if !exists {
        TranscriptsRepository::save_transcript_with_id(
            pool,
            &recording.id,
            &recording.title,
            &recording.segments,
            recording.folder.clone(),
        )
        .await
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

async fn persist(app: &AppHandle, recording: &SavedRecording) -> Result<(), String> {
    let folder = directory(app)?;
    let path = folder.join(format!("{}.json", recording.id));
    let mut file = tempfile::NamedTempFile::new_in(&folder).map_err(|e| e.to_string())?;
    serde_json::to_writer(&mut file, &recording).map_err(|e| e.to_string())?;
    file.flush().map_err(|e| e.to_string())?;
    file.as_file().sync_all().map_err(|e| e.to_string())?;
    file.persist(&path).map_err(|e| e.to_string())?;
    commit_journal(app, &path, recording).await
}

async fn commit_journal(
    app: &AppHandle,
    path: &Path,
    recording: &SavedRecording,
) -> Result<(), String> {
    save_database(
        app.state::<crate::state::AppState>().db_manager.pool(),
        recording,
    )
    .await?;
    crate::extensions::meeting_workflow::finalize(
        app.state::<crate::state::AppState>().db_manager.pool(),
        &recording.id,
    )
    .await?;
    std::fs::remove_file(path).map_err(|e| e.to_string())?;
    crate::extensions::usage::count_once(
        app.state::<crate::state::AppState>().db_manager.pool(),
        "recording_saved",
        &recording.id,
    )
    .await;
    // Queued transcription or notes run the edition's hook themselves.
    let ready = app.clone();
    let id = recording.id.clone();
    tauri::async_runtime::spawn(async move {
        let pool = ready
            .state::<crate::state::AppState>()
            .db_manager
            .pool()
            .clone();
        let queued: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workspace_jobs WHERE meeting_id=? AND kind IN ('transcribe','notes') AND state IN ('queued','running'))")
            .bind(&id).fetch_one(&pool).await.unwrap_or(true);
        if !queued {
            crate::addon::transcript_ready(&ready, &id).await;
        }
    });
    *app.state::<Persistence>().completed.lock_recovering() = Some((
        recording.id.clone(),
        recording
            .segments
            .iter()
            .any(|segment| !segment.is_partial && !segment.text.trim().is_empty()),
    ));
    let _ = app.emit(
        "meeting-updated",
        serde_json::json!({"meeting_id":recording.id}),
    );
    Ok(())
}

async fn recover(app: &AppHandle) -> Result<(), String> {
    for entry in std::fs::read_dir(directory(app)?).map_err(|e| e.to_string())? {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") || !path.is_file() {
            continue;
        }
        let file = std::fs::File::open(&path).map_err(|e| e.to_string())?;
        let recording: SavedRecording = serde_json::from_reader(file).map_err(|e| e.to_string())?;
        if recording
            .id
            .strip_prefix("meeting-")
            .is_none_or(|id| uuid::Uuid::parse_str(id).is_err())
        {
            return Err("Invalid pending recording ID".into());
        }
        commit_journal(app, &path, &recording).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn late_final_segments_replace_partials_and_recovery_is_idempotent() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let mut session = Session {
            id: format!("meeting-{}", uuid::Uuid::new_v4()),
            ..Default::default()
        };
        session.update(&serde_json::json!({"sequence_id":1,"text":"part", "is_partial":true}));
        session.update(&serde_json::json!({"sequence_id":1,"text":"Complete sentence.", "is_partial":false,"audio_start_time":2.0,"audio_end_time":4.0}));
        session.update(&serde_json::json!({"sequence_id":1,"text":"stale", "is_partial":true}));
        session.update(&serde_json::json!({"sequence_id":2,"text":"Final shutdown chunk.", "is_partial":false,"audio_start_time":4.0,"audio_end_time":5.0}));
        let recording = SavedRecording {
            id: session.id,
            title: "Test".into(),
            folder: None,
            segments: session.segments.into_values().collect(),
        };
        save_database(&pool, &recording).await.unwrap();
        save_database(&pool, &recording).await.unwrap();
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM meetings")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 1);
        let texts: Vec<String> =
            sqlx::query_scalar("SELECT transcript FROM transcripts ORDER BY audio_start_time")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(texts, ["Complete sentence.", "Final shutdown chunk."]);
    }
}
