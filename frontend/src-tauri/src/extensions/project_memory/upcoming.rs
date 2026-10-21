//! Projects for upcoming calendar events, decided before they start so that a
//! recording begins with the right vocabulary, voices and context.
use super::matcher::{decide, score_event, Decision, Knowledge, Scores};
use super::signals::{Event, Own};
use crate::extensions::calendar::CalendarEvent;
use serde_json::{json, Value};
use sqlx::SqlitePool;
use std::collections::HashSet;
use tauri::AppHandle;

/// Events worth filing: not declined, and with someone else invited or a call link.
fn eligible(event: &Event) -> bool {
    !event.declined_by_me() && (event.guests().next().is_some() || event.has_link)
}

pub async fn refresh(app: &AppHandle, pool: &SqlitePool) {
    refresh_with(pool, &super::settings(app).auto_assign).await;
}

/// `mode` is the automatic filing setting: `confident`, `suggest` or `off`.
pub async fn refresh_with(pool: &SqlitePool, mode: &str) {
    static REFRESHING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let _refreshing = REFRESHING.lock().await;
    if mode == "off" {
        let _ = sqlx::query("DELETE FROM event_projects WHERE origin IN ('automatic','suggested')")
            .execute(pool)
            .await;
        return;
    }
    let Ok(knowledge) = Knowledge::load(pool).await else {
        return;
    };
    let own = Own::load(pool).await;
    forget_own(pool, &own).await;
    let events: Vec<CalendarEvent> = sqlx::query_as("SELECT * FROM workspace_calendar WHERE all_day=0 AND julianday(ends_at)>=julianday('now') AND julianday(starts_at)<=julianday('now','+14 days') ORDER BY julianday(starts_at) LIMIT 500")
        .fetch_all(pool).await.unwrap_or_default();
    let chosen: HashSet<String> =
        sqlx::query_scalar("SELECT event_id FROM event_projects WHERE origin='manual'")
            .fetch_all(pool)
            .await
            .unwrap_or_default()
            .into_iter()
            .collect();
    let Ok(mut tx) = pool.begin().await else {
        return;
    };
    for row in events {
        if chosen.contains(&row.id) {
            continue;
        }
        let event = Event::from_row(&json!(row), &own);
        let decision = if eligible(&event) && !knowledge.is_empty() {
            let mut scores = Scores::default();
            score_event(&knowledge, &event, &own, &mut scores);
            decide(&scores, mode)
        } else {
            Decision::Nothing
        };
        let result = match decision {
            Decision::Confident { project, reason } => upsert(&mut tx, &event.id, &project, "automatic", &reason).await,
            Decision::Suggest { project, reason } => upsert(&mut tx, &event.id, &project, "suggested", &reason).await,
            Decision::Nothing => sqlx::query("DELETE FROM event_projects WHERE event_id=? AND origin IN ('automatic','suggested')")
                .bind(&event.id).execute(&mut *tx).await.map(|_| ()),
        };
        if result.is_err() {
            return;
        }
    }
    let _ = tx.commit().await;
}

/// A meeting filed before the person's own address was known may have taught
/// it as a signal. It never counts when scoring; remove it from view as well.
async fn forget_own(pool: &SqlitePool, own: &Own) {
    for (kind, values) in [("email", &own.emails), ("domain", &own.domains)] {
        for value in values {
            let _ = sqlx::query(
                "DELETE FROM project_match_rules WHERE origin='learned' AND kind=? AND value=?",
            )
            .bind(kind)
            .bind(value)
            .execute(pool)
            .await;
        }
    }
}

async fn upsert(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    event: &str,
    project: &str,
    origin: &str,
    reason: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO event_projects(event_id,project_id,origin,reason,updated_at) VALUES(?,?,?,?,?) ON CONFLICT(event_id) DO UPDATE SET project_id=excluded.project_id,origin=excluded.origin,reason=excluded.reason,updated_at=excluded.updated_at WHERE event_projects.origin<>'manual'")
        .bind(event).bind(project).bind(origin).bind(reason).bind(super::now())
        .execute(&mut **tx).await.map(|_| ())
}

/// A recording started for an event nobody filed yet, for example one created
/// minutes ago. Only a confident decision files it; anything less waits for
/// the transcript.
pub async fn project_for_event(
    app: &AppHandle,
    pool: &SqlitePool,
    event: &Value,
) -> Option<(String, String)> {
    let settings = super::settings(app);
    if settings.auto_assign != "confident" {
        return None;
    }
    let knowledge = Knowledge::load(pool).await.ok()?;
    let own = Own::load(pool).await;
    let event = Event::from_row(event, &own);
    let mut scores = Scores::default();
    score_event(&knowledge, &event, &own, &mut scores);
    match decide(&scores, "confident") {
        Decision::Confident { project, reason } => Some((project, reason)),
        _ => None,
    }
}
