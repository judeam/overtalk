//! Deterministic review signals, reversible notes and local task reminders.
use crate::state::AppState;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{FromRow, SqlitePool};
use tauri::{AppHandle, Emitter, State};
fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}
#[derive(Serialize)]
pub struct Issue {
    pub id: String,
    pub fingerprint: String,
    pub meeting_id: String,
    pub title: String,
    pub segment_id: String,
    pub text: String,
    pub kind: String,
    pub start: f64,
    pub speaker_id: Option<String>,
}
fn repetitions(text: &str) -> bool {
    let words: Vec<_> = text.split_whitespace().map(|w| w.to_lowercase()).collect();
    (2..=8).any(|n| {
        words
            .windows(n * 3)
            .any(|w| w[..n] == w[n..2 * n] && w[..n] == w[2 * n..])
    })
}
pub fn start_reminders(app: AppHandle) {
    use tauri::Manager;
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            let Some(state) = app.try_state::<AppState>() else {
                continue;
            };
            let pool = state.db_manager.pool();
            let rows:Vec<(String,String)>=sqlx::query_as("SELECT t.id,t.title FROM task_reminders r JOIN workspace_tasks t ON t.id=r.task_id WHERE t.status='open' AND r.notified_at IS NULL AND r.remind_at<=? LIMIT 50").bind(chrono::Utc::now().to_rfc3339()).fetch_all(pool).await.unwrap_or_default();
            for (id, title) in rows {
                let claimed=sqlx::query("UPDATE task_reminders SET notified_at=? WHERE task_id=? AND notified_at IS NULL").bind(chrono::Utc::now().to_rfc3339()).bind(&id).execute(pool).await;
                if !claimed.is_ok_and(|v| v.rows_affected() == 1) {
                    continue;
                }
                let _ = app.emit("workspace-reminder", json!({"id":id,"title":title}));
                if let Some(manager)=app.try_state::<crate::notifications::commands::NotificationManagerState<tauri::Wry>>() {
                if let Some(manager)=manager.read().await.as_ref(){let note=crate::notifications::Notification::new("Action reminder",title,crate::notifications::NotificationType::MeetingReminder(0));let _=manager.show_notification(note).await;}
            }
            }
        }
    });
}
pub async fn issues(
    pool: &SqlitePool,
    project: Option<&str>,
    meeting: Option<&str>,
) -> Result<Vec<Issue>, String> {
    type ReviewTranscriptRow = (String, String, String, String, Option<String>, f64, i64);
    let rows: Vec<ReviewTranscriptRow> = sqlx::query_as(
        "SELECT t.meeting_id,m.title,t.id,t.transcript,t.speaker_id,COALESCE(t.audio_start_time,0),\
         COALESCE(t.overlap,0) FROM transcripts t JOIN meetings m ON m.id=t.meeting_id \
         LEFT JOIN project_meetings p ON p.meeting_id=m.id LEFT JOIN meeting_workspace_state s \
         ON s.meeting_id=m.id WHERE s.trashed_at IS NULL AND (? IS NULL OR p.project_id=?) \
         AND (? IS NULL OR m.id=?) ORDER BY m.created_at DESC,t.audio_start_time,t.id LIMIT 10000",
    )
    .bind(project)
    .bind(project)
    .bind(meeting)
    .bind(meeting)
    .fetch_all(pool)
    .await
    .map_err(err)?;
    let resolved: std::collections::HashMap<String, String> =
        sqlx::query_as::<_, (String, String)>(
            "SELECT issue_id,fingerprint FROM review_resolutions",
        )
        .fetch_all(pool)
        .await
        .map_err(err)?
        .into_iter()
        .collect();
    let mut output = Vec::new();
    let notes: Vec<(String, String, String, i64)> = sqlx::query_as(
        "SELECT m.id,m.title,s.result,COALESCE(r.revision,0) FROM summary_processes s \
         JOIN meetings m ON m.id=s.meeting_id LEFT JOIN meeting_revisions r ON r.meeting_id=m.id \
         LEFT JOIN project_meetings p ON p.meeting_id=m.id LEFT JOIN meeting_workspace_state w \
         ON w.meeting_id=m.id WHERE s.result IS NOT NULL AND w.trashed_at IS NULL AND (? IS NULL \
         OR p.project_id=?) AND (? IS NULL OR m.id=?) ORDER BY m.created_at DESC LIMIT 250",
    )
    .bind(project)
    .bind(project)
    .bind(meeting)
    .bind(meeting)
    .fetch_all(pool)
    .await
    .map_err(err)?;
    let citations = regex::Regex::new(r"\[S\d+\]|#source-\d+").map_err(err)?;
    for (meeting_id, title, result, revision) in notes {
        let Ok(value) = serde_json::from_str::<Value>(&result) else {
            continue;
        };
        if value["manual"] == true {
            continue;
        }
        let markdown = overtalk_documents::notes_markdown(&value)?;
        let stale = value["provenance"]["source_revision"]
            .as_i64()
            .is_some_and(|old| old != revision);
        let missing = markdown.len() > 80 && !citations.is_match(&markdown);
        for (kind, present, reason) in [
            (
                "notes_stale",
                stale,
                "The transcript or speaker names changed after these notes were generated.",
            ),
            (
                "notes_sources",
                missing,
                "These generated notes have no linked source passages. Check them against the \
                 recording.",
            ),
        ] {
            if !present {
                continue;
            }
            let id = format!("{kind}:{meeting_id}");
            let fingerprint = super::projects::fingerprint(&format!("{result}\0{revision}"));
            if resolved.get(&id) == Some(&fingerprint) {
                continue;
            }
            output.push(Issue {
                id,
                fingerprint,
                meeting_id: meeting_id.clone(),
                title: title.clone(),
                segment_id: String::new(),
                text: reason.into(),
                kind: kind.into(),
                start: 0.,
                speaker_id: None,
            });
        }
    }
    for (meeting_id, title, segment_id, text, speaker_id, start, overlap) in rows {
        for (kind, present) in [
            ("unassigned", speaker_id.is_none()),
            ("overlap", overlap != 0),
            ("repetition", repetitions(&text)),
        ] {
            if !present {
                continue;
            }
            let id = format!("{kind}:{segment_id}");
            let fingerprint =
                super::projects::fingerprint(&format!("{text}\0{speaker_id:?}\0{overlap}"));
            if resolved.get(&id) == Some(&fingerprint) {
                continue;
            }
            output.push(Issue {
                id,
                fingerprint,
                meeting_id: meeting_id.clone(),
                title: title.clone(),
                segment_id: segment_id.clone(),
                text: text.clone(),
                kind: kind.into(),
                start,
                speaker_id: speaker_id.clone(),
            });
            if output.len() >= 500 {
                return Ok(output);
            }
        }
    }
    Ok(output)
}
#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum ReviewAction {
    List {
        project_id: Option<String>,
        meeting_id: Option<String>,
    },
    Resolve {
        id: String,
        fingerprint: String,
        resolution: String,
    },
    Versions {
        meeting_id: String,
    },
    Restore {
        meeting_id: String,
        version_id: i64,
        expected_hash: String,
    },
    Remind {
        task_id: String,
        at: Option<String>,
    },
    Due,
}
#[derive(Serialize, FromRow)]
struct Version {
    id: i64,
    meeting_id: String,
    content: String,
    created_at: String,
    origin: String,
}
pub async fn apply(pool: &SqlitePool, action: ReviewAction) -> Result<Value, String> {
    match action {
        ReviewAction::List {
            project_id,
            meeting_id,
        } => Ok(json!(
            issues(pool, project_id.as_deref(), meeting_id.as_deref()).await?
        )),
        ReviewAction::Resolve {
            id,
            fingerprint,
            resolution,
        } => {
            if !matches!(resolution.as_str(), "reviewed" | "dismissed") {
                return Err("Invalid review resolution".into());
            }
            if id.len() > 240 || fingerprint.len() != 64 {
                return Err("Invalid review item".into());
            }
            // Only a specific content revision is dismissed. A later correction
            // or re-transcription automatically makes the issue visible again.
            sqlx::query("INSERT INTO review_resolutions VALUES(?,?,?,?) ON CONFLICT(issue_id) DO UPDATE SET fingerprint=excluded.fingerprint,resolution=excluded.resolution,updated_at=excluded.updated_at")
                .bind(id).bind(fingerprint).bind(resolution).bind(chrono::Utc::now().to_rfc3339()).execute(pool).await.map_err(err)?;
            Ok(Value::Null)
        }
        ReviewAction::Versions { meeting_id } => {
            let mut tx = pool.begin().await.map_err(err)?;
            let current: Option<String> =
                sqlx::query_scalar("SELECT result FROM summary_processes WHERE meeting_id=?")
                    .bind(&meeting_id)
                    .fetch_optional(&mut *tx)
                    .await
                    .map_err(err)?
                    .flatten();
            let rows: Vec<Version> = sqlx::query_as(
                "SELECT * FROM note_versions WHERE meeting_id=? ORDER BY id DESC LIMIT 100",
            )
            .bind(meeting_id)
            .fetch_all(&mut *tx)
            .await
            .map_err(err)?;
            tx.commit().await.map_err(err)?;
            Ok(
                json!({"versions":rows,"current_hash":super::projects::fingerprint(current.as_deref().unwrap_or(""))}),
            )
        }
        ReviewAction::Restore {
            meeting_id,
            version_id,
            expected_hash,
        } => {
            let mut tx = pool.begin().await.map_err(err)?;
            let running:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM summary_processes WHERE meeting_id=? AND status IN ('processing','pending','running'))").bind(&meeting_id).fetch_one(&mut *tx).await.map_err(err)?;
            if running {
                return Err("Finish or cancel notes generation before restoring a version".into());
            }
            let current: Option<String> =
                sqlx::query_scalar("SELECT result FROM summary_processes WHERE meeting_id=?")
                    .bind(&meeting_id)
                    .fetch_optional(&mut *tx)
                    .await
                    .map_err(err)?
                    .flatten();
            if super::projects::fingerprint(current.as_deref().unwrap_or("")) != expected_hash {
                return Err(
                    "Notes changed since you opened history. Refresh before restoring.".into(),
                );
            }
            let old: String =
                sqlx::query_scalar("SELECT content FROM note_versions WHERE meeting_id=? AND id=?")
                    .bind(&meeting_id)
                    .bind(version_id)
                    .fetch_optional(&mut *tx)
                    .await
                    .map_err(err)?
                    .ok_or("Version no longer exists")?;
            let mut restored: Value = serde_json::from_str(&old).map_err(err)?;
            restored["manual"] = json!(true);
            restored["restored_from"] = json!(version_id);
            sqlx::query("UPDATE summary_processes SET result=?,updated_at=? WHERE meeting_id=?")
                .bind(restored.to_string())
                .bind(chrono::Utc::now().to_rfc3339())
                .bind(&meeting_id)
                .execute(&mut *tx)
                .await
                .map_err(err)?;
            sqlx::query("UPDATE meetings SET updated_at=? WHERE id=?")
                .bind(chrono::Utc::now().to_rfc3339())
                .bind(&meeting_id)
                .execute(&mut *tx)
                .await
                .map_err(err)?;
            tx.commit().await.map_err(err)?;
            Ok(json!({"restored":version_id}))
        }
        ReviewAction::Remind { task_id, at } => {
            if let Some(at) = at {
                let date = chrono::DateTime::parse_from_rfc3339(&at)
                    .map_err(|_| "Choose a valid reminder time")?;
                sqlx::query("INSERT INTO task_reminders(task_id,remind_at) VALUES(?,?) ON CONFLICT(task_id) DO UPDATE SET remind_at=excluded.remind_at,notified_at=NULL")
                    .bind(task_id).bind(date.with_timezone(&chrono::Utc).to_rfc3339()).execute(pool).await.map_err(err)?;
            } else {
                sqlx::query("DELETE FROM task_reminders WHERE task_id=?")
                    .bind(task_id)
                    .execute(pool)
                    .await
                    .map_err(err)?;
            }
            Ok(Value::Null)
        }
        ReviewAction::Due => {
            let rows:Vec<super::knowledge::Task>=sqlx::query_as("SELECT t.* FROM workspace_tasks t LEFT JOIN task_reminders r ON r.task_id=t.id WHERE t.status='open' AND ((r.remind_at<=? AND r.notified_at IS NULL) OR (t.due_date IS NOT NULL AND t.due_date<=?)) ORDER BY t.due_date LIMIT 100")
                .bind(chrono::Utc::now().to_rfc3339()).bind(chrono::Local::now().format("%Y-%m-%d").to_string()).fetch_all(pool).await.map_err(err)?;
            Ok(json!(rows))
        }
    }
}
#[tauri::command]
pub async fn workspace_note_revision(
    state: State<'_, AppState>,
    meeting_id: String,
) -> Result<String, String> {
    let current: Option<String> =
        sqlx::query_scalar("SELECT result FROM summary_processes WHERE meeting_id=?")
            .bind(meeting_id)
            .fetch_optional(state.db_manager.pool())
            .await
            .map_err(err)?
            .flatten();
    Ok(super::projects::fingerprint(
        current.as_deref().unwrap_or(""),
    ))
}
#[tauri::command]
pub async fn workspace_review(
    app: AppHandle,
    state: State<'_, AppState>,
    action: ReviewAction,
) -> Result<Value, String> {
    let write = matches!(
        action,
        ReviewAction::Resolve { .. } | ReviewAction::Restore { .. } | ReviewAction::Remind { .. }
    );
    let result = apply(state.db_manager.pool(), action).await?;
    if write {
        let _ = app.emit("workspace-review-changed", ());
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repeated_phrases_are_specific() {
        assert!(repetitions(
            "not the new key not the new key not the new key"
        ));
        assert!(!repetitions("We agreed to keep the old key until Friday."));
    }
}
