//! Reversible organisation and human transcript corrections.
use crate::state::AppState;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};
use tauri::{Emitter, State};

#[derive(Debug, Serialize, FromRow)]
pub struct MeetingState {
    pub meeting_id: String,
    pub pinned: bool,
    pub archived: bool,
    pub trashed_at: Option<String>,
    pub title: String,
}

#[tauri::command]
pub async fn workspace_meeting_states(
    state: State<'_, AppState>,
) -> Result<Vec<MeetingState>, String> {
    sqlx::query_as(
        "SELECT s.*, m.title FROM meeting_workspace_state s JOIN meetings m ON m.id=s.meeting_id",
    )
    .fetch_all(state.db_manager.pool())
    .await
    .map_err(|e| e.to_string())
}

#[derive(Debug, Deserialize)]
pub struct OrganisationChange {
    pub meeting_id: String,
    pub pinned: Option<bool>,
    pub archived: Option<bool>,
    pub trashed: Option<bool>,
}

pub async fn organise(pool: &SqlitePool, change: OrganisationChange) -> Result<(), String> {
    let mut tx = crate::database::begin_write(pool)
        .await
        .map_err(|e| e.to_string())?;
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM meetings WHERE id=?)")
        .bind(&change.meeting_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    if !exists {
        return Err("This meeting no longer exists".into());
    }
    sqlx::query("INSERT INTO meeting_workspace_state(meeting_id) VALUES(?) ON CONFLICT DO NOTHING")
        .bind(&change.meeting_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    sqlx::query("UPDATE meeting_workspace_state SET pinned=COALESCE(?,pinned), archived=COALESCE(?,archived), trashed_at=CASE WHEN ? IS NULL THEN trashed_at WHEN ? THEN ? ELSE NULL END WHERE meeting_id=?")
        .bind(change.pinned).bind(change.archived).bind(change.trashed).bind(change.trashed)
        .bind(chrono::Utc::now().to_rfc3339()).bind(change.meeting_id)
        .execute(&mut *tx).await.map_err(|e| e.to_string())?;
    tx.commit().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn workspace_organise_meeting(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    change: OrganisationChange,
) -> Result<(), String> {
    organise(state.db_manager.pool(), change).await?;
    let _ = app.emit("workspace-meetings-changed", ());
    Ok(())
}

#[derive(Debug, Deserialize)]
pub struct TranscriptCorrection {
    pub meeting_id: String,
    pub segment_id: String,
    pub text: String,
    pub speaker_id: Option<String>,
    pub expected_text: String,
    pub expected_speaker_id: Option<String>,
}

pub async fn correct_transcript(
    pool: &SqlitePool,
    change: TranscriptCorrection,
) -> Result<(), String> {
    if change.text.trim().is_empty() || change.text.chars().count() > 20_000 {
        return Err("A transcript line must contain between 1 and 20,000 characters".into());
    }
    let mut tx = crate::database::begin_write(pool)
        .await
        .map_err(|e| e.to_string())?;
    type StoredTranscriptRow = (String, Option<String>, Option<f64>, Option<f64>);
    let old: Option<StoredTranscriptRow> = sqlx::query_as(
        "SELECT transcript,speaker_id,audio_start_time,audio_end_time FROM transcripts WHERE meeting_id=? AND id=?")
        .bind(&change.meeting_id).bind(&change.segment_id).fetch_optional(&mut *tx).await.map_err(|e| e.to_string())?;
    let (text, speaker, start, end) =
        old.ok_or("This transcript line no longer exists. Reopen the meeting.")?;
    if text != change.expected_text || speaker != change.expected_speaker_id {
        return Err("This line changed while you were editing it. Reopen the editor to review the latest version.".into());
    }
    if let Some(id) = &change.speaker_id {
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM meeting_speakers WHERE meeting_id=? AND id=?)",
        )
        .bind(&change.meeting_id)
        .bind(id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
        if !exists {
            return Err("Choose a speaker from this meeting".into());
        }
    }
    super::speech::snapshot_transcript(&mut tx, &change.meeting_id).await?;
    sqlx::query("UPDATE transcripts SET transcript=?,speaker_id=?,speaker_confidence=CASE WHEN speaker_id IS ? THEN speaker_confidence ELSE NULL END WHERE meeting_id=? AND id=?")
        .bind(change.text.trim()).bind(&change.speaker_id).bind(&change.speaker_id).bind(&change.meeting_id).bind(&change.segment_id)
        .execute(&mut *tx).await.map_err(|e| e.to_string())?;
    // Update only the affected voice interval, retaining overlapping voices and surrounding turns.
    if speaker != change.speaker_id {
        if let (Some(start), Some(end)) = (start, end) {
            if end > start {
                let turns: Vec<(i64, String, f64, f64, Option<f64>, String)> = sqlx::query_as(
                    "SELECT rowid,speaker_id,start_time,end_time,confidence,model FROM speaker_turns WHERE meeting_id=? AND speaker_id IS ? AND start_time<? AND end_time>?")
                    .bind(&change.meeting_id).bind(&speaker).bind(end).bind(start).fetch_all(&mut *tx).await.map_err(|e| e.to_string())?;
                for (id, voice, left, right, confidence, model) in turns {
                    sqlx::query("DELETE FROM speaker_turns WHERE rowid=?")
                        .bind(id)
                        .execute(&mut *tx)
                        .await
                        .map_err(|e| e.to_string())?;
                    for (a, b) in [(left, start.min(right)), (end.max(left), right)] {
                        if b > a {
                            sqlx::query("INSERT INTO speaker_turns(meeting_id,speaker_id,start_time,end_time,confidence,model) VALUES(?,?,?,?,?,?)")
                                .bind(&change.meeting_id).bind(&voice).bind(a).bind(b).bind(confidence).bind(&model).execute(&mut *tx).await.map_err(|e| e.to_string())?;
                        }
                    }
                }
                if let Some(voice) = &change.speaker_id {
                    sqlx::query("INSERT INTO speaker_turns(meeting_id,speaker_id,start_time,end_time,model) VALUES(?,?,?,?,?)")
                        .bind(&change.meeting_id).bind(voice).bind(start).bind(end).bind("human-correction").execute(&mut *tx).await.map_err(|e| e.to_string())?;
                }
            }
        }
    }
    sqlx::query("UPDATE meetings SET updated_at=? WHERE id=?")
        .bind(chrono::Utc::now().to_rfc3339())
        .bind(&change.meeting_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    tx.commit().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn workspace_correct_transcript(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    change: TranscriptCorrection,
) -> Result<(), String> {
    if super::speech::is_processing().await {
        return Err("Wait for speech processing to finish before correcting the transcript".into());
    }
    let meeting_id = change.meeting_id.clone();
    correct_transcript(state.db_manager.pool(), change).await?;
    let _ = app.emit("meeting-speakers-updated", &meeting_id);
    Ok(())
}

#[tauri::command]
pub async fn workspace_source_at(
    state: State<'_, AppState>,
    meeting_id: String,
    at: f64,
) -> Result<Option<super::knowledge::Source>, String> {
    if !at.is_finite() || at < 0.0 {
        return Err("Invalid source time".into());
    }
    let mut source: Option<super::knowledge::Source> = sqlx::query_as("SELECT id,meeting_id,source_kind,source_id,title,body,start_time,end_time,recorded_at FROM workspace_documents WHERE meeting_id=? AND source_kind='transcript' AND start_time<=? AND end_time>=? ORDER BY start_time DESC LIMIT 1")
        .bind(meeting_id).bind(at + 0.1).bind(at - 0.1).fetch_optional(state.db_manager.pool()).await.map_err(|e| e.to_string())?;
    if let Some(item) = &mut source {
        item.fingerprint = super::knowledge::fingerprint(item);
    }
    Ok(source)
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
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        sqlx::query("INSERT INTO meetings(id,title,created_at,updated_at) VALUES('one','Review','2026-10-15','2026-10-15')").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO transcripts(id,meeting_id,transcript,timestamp,audio_start_time,audio_end_time) VALUES('line','one','Wrong number','00:00',0,10)").execute(&pool).await.unwrap();
        pool
    }
    fn correction(expected: &str) -> TranscriptCorrection {
        TranscriptCorrection {
            meeting_id: "one".into(),
            segment_id: "line".into(),
            text: "Correct number".into(),
            speaker_id: None,
            expected_text: expected.into(),
            expected_speaker_id: None,
        }
    }
    #[tokio::test]
    async fn correction_preserves_original_and_rejects_stale_writes() {
        let pool = database().await;
        correct_transcript(&pool, correction("Wrong number"))
            .await
            .unwrap();
        let text: String = sqlx::query_scalar("SELECT transcript FROM transcripts WHERE id='line'")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(text, "Correct number");
        let original: String = sqlx::query_scalar("SELECT json_extract(transcripts,'$[0].text') FROM speech_transcript_backups WHERE meeting_id='one'").fetch_one(&pool).await.unwrap();
        assert_eq!(original, "Wrong number");
        assert!(correct_transcript(&pool, correction("Wrong number"))
            .await
            .is_err());
        let indexed: String =
            sqlx::query_scalar("SELECT body FROM workspace_documents WHERE id='transcript:line'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(indexed, "Correct number");
    }
    #[tokio::test]
    async fn trash_is_reversible_and_excluded_from_search() {
        let pool = database().await;
        let query = super::super::knowledge::SearchRequest {
            query: "Wrong".into(),
            ..Default::default()
        };
        assert!(!super::super::knowledge::search(&pool, &query)
            .await
            .unwrap()
            .sources
            .is_empty());
        for (trashed, expected) in [(true, 0), (false, 1)] {
            organise(
                &pool,
                OrganisationChange {
                    meeting_id: "one".into(),
                    pinned: Some(true),
                    archived: None,
                    trashed: Some(trashed),
                },
            )
            .await
            .unwrap();
            assert_eq!(
                super::super::knowledge::search(&pool, &query)
                    .await
                    .unwrap()
                    .sources
                    .len(),
                expected
            );
        }
        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM transcripts WHERE meeting_id='one'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(count, 1);
    }
}
