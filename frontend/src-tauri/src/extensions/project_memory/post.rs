//! Filing a meeting right after it ends, before notes are written, so notes
//! and project learning get the project's context.
use super::matcher::{
    decide, score_event, score_title, score_transcript, Decision, Knowledge, Scores,
};
use super::signals::{Event, Own};
use serde_json::json;
use sqlx::SqlitePool;
use tauri::AppHandle;

/// Enough text to recognise a project without scanning very long meetings.
const TRANSCRIPT_LIMIT: usize = 200_000;

async fn undecided(pool: &SqlitePool, meeting: &str) -> bool {
    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM meetings WHERE id=?)
        AND NOT EXISTS(SELECT 1 FROM project_meetings WHERE meeting_id=?)
        AND NOT EXISTS(SELECT 1 FROM project_suggestions WHERE meeting_id=?)
        AND NOT EXISTS(SELECT 1 FROM filing_log WHERE meeting_id=?)
        AND NOT EXISTS(SELECT 1 FROM meeting_workspace_state WHERE meeting_id=? AND trashed_at IS NOT NULL)
        AND NOT EXISTS(SELECT 1 FROM workspace_capture_workflows WHERE meeting_id=? AND project_origin='manual' AND project_id IS NULL)
        AND NOT EXISTS(SELECT 1 FROM workspace_jobs WHERE meeting_id=? AND kind='transcribe' AND state IN ('queued','running'))")
        .bind(meeting).bind(meeting).bind(meeting).bind(meeting).bind(meeting).bind(meeting).bind(meeting)
        .fetch_one(pool).await.unwrap_or(false)
}

pub async fn transcript_ready(app: &AppHandle, pool: &SqlitePool, meeting: &str) {
    let settings = super::settings(app);
    match decide_meeting(pool, meeting, &settings.auto_assign).await {
        Some(("filed", _)) => {
            // Remembered voices are scoped to projects: name speakers now that
            // the project is known, if the person turned automatic recognition on.
            // Voice memory may not be set up at all.
            let _ =
                crate::extensions::voice_memory::recognize_if_automatic(app, pool, meeting).await;
            crate::addon::emit_projects_changed(app);
        }
        Some(("suggested", _)) => crate::addon::emit_projects_changed(app),
        _ => {}
    }
}

/// File the meetings already recorded, so the first brief has history to draw on.
/// Newest first, each decided once; a person's own choice is never replaced. Runs at
/// start and after a person files a meeting (their choice may teach the matcher a new
/// project).
pub async fn backfill(app: &AppHandle, pool: &SqlitePool) -> usize {
    static RUNNING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let Ok(_running) = RUNNING.try_lock() else {
        return 0;
    };
    let settings = super::settings(app);
    let meetings: Vec<String> = sqlx::query_scalar("SELECT id FROM meetings m WHERE NOT EXISTS(SELECT 1 FROM project_meetings p WHERE p.meeting_id=m.id) AND NOT EXISTS(SELECT 1 FROM filing_log l WHERE l.meeting_id=m.id) ORDER BY created_at DESC LIMIT 500")
        .fetch_all(pool).await.unwrap_or_default();
    let mut changed = 0;
    for meeting in meetings {
        if decide_meeting(pool, &meeting, &settings.auto_assign)
            .await
            .is_some_and(|(outcome, _)| outcome != "unsorted")
        {
            changed += 1;
        }
        tokio::task::yield_now().await;
    }
    if changed > 0 {
        crate::addon::emit_projects_changed(app);
    }
    changed
}

/// File or suggest a project for a finished meeting once. Returns the outcome
/// and project, or `None` when the meeting is not for automatic filing to decide.
pub async fn decide_meeting(
    pool: &SqlitePool,
    meeting: &str,
    mode: &str,
) -> Option<(&'static str, Option<String>)> {
    static DECIDING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let _deciding = DECIDING.lock().await;
    if mode == "off" || !undecided(pool, meeting).await {
        return None;
    }
    let mut transcript = String::new();
    let rows: Vec<String> = sqlx::query_scalar("SELECT transcript FROM transcripts WHERE meeting_id=? AND coalesce(is_partial,0)=0 ORDER BY audio_start_time,id")
        .bind(meeting).fetch_all(pool).await.unwrap_or_default();
    for row in rows {
        if transcript.len() + row.len() > TRANSCRIPT_LIMIT {
            break;
        }
        transcript.push_str(&row);
        transcript.push('\n');
    }
    if transcript.trim().is_empty() {
        return None;
    }
    let knowledge = Knowledge::load(pool).await.ok()?;
    if knowledge.is_empty() {
        return None;
    }
    let speakers: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM meeting_speakers WHERE meeting_id=? AND name NOT LIKE 'Speaker %'",
    )
    .bind(meeting)
    .fetch_all(pool)
    .await
    .unwrap_or_default();
    let title: String = sqlx::query_scalar("SELECT title FROM meetings WHERE id=?")
        .bind(meeting)
        .fetch_one(pool)
        .await
        .unwrap_or_default();
    let event: Option<(String, String, String, String, String, String, String)> = sqlx::query_as("SELECT event_id,source,series_id,title,starts_at,organizer,participants FROM meeting_events WHERE meeting_id=?")
        .bind(meeting).fetch_optional(pool).await.unwrap_or(None);
    let own = Own::load(pool).await;
    let mut scores = Scores::default();
    match event {
        Some((id, source, series_id, event_title, starts_at, organizer, participants)) => {
            let row = json!({"id":id,"source":source,"series_id":series_id,"title":event_title,"starts_at":starts_at,"organizer":organizer,"participants":participants});
            score_event(&knowledge, &Event::from_row(&row, &own), &own, &mut scores);
        }
        None => score_title(&knowledge, &title, &mut scores),
    }
    score_transcript(&knowledge, &transcript, &speakers, &mut scores);
    let (outcome, project, reason) = match decide(&scores, mode) {
        Decision::Confident { project, reason } => {
            let filed = sqlx::query("INSERT OR IGNORE INTO project_meetings(meeting_id,project_id,origin,reason) VALUES(?,?,'automatic',?)")
                .bind(meeting).bind(&project).bind(&reason).execute(pool).await;
            if !filed.is_ok_and(|r| r.rows_affected() == 1) {
                return None;
            }
            ("filed", Some(project), reason)
        }
        Decision::Suggest { project, reason } => {
            let _ = sqlx::query("INSERT OR IGNORE INTO project_suggestions(meeting_id,project_id,reason,created_at) VALUES(?,?,?,?)")
                .bind(meeting).bind(&project).bind(&reason).bind(super::now()).execute(pool).await;
            ("suggested", Some(project), reason)
        }
        Decision::Nothing => ("unsorted", None, String::new()),
    };
    let _ = sqlx::query("INSERT OR IGNORE INTO filing_log(meeting_id,outcome,project_id,reason,decided_at) VALUES(?,?,?,?,?)")
        .bind(meeting).bind(outcome).bind(&project).bind(&reason).bind(super::now()).execute(pool).await;
    match outcome {
        "filed" => crate::extensions::usage::count(pool, "filed_automatically").await,
        "suggested" => crate::extensions::usage::count(pool, "suggested").await,
        _ => {}
    }
    Some((outcome, project))
}
