//! The person's own usage report: what they did in Overtalk, counted on this computer.
//! There is no telemetry. Nothing is sent; the person can read the report in Privacy
//! settings and save it as a file, then decide whether to share it. Counts never
//! carry titles, names, text or audio.
use serde_json::{json, Map, Value};
use sqlx::SqlitePool;
use tauri::{AppHandle, Runtime, State};
use tauri_plugin_dialog::DialogExt;

/// Everything that can be counted. Anything else is ignored.
const EVENTS: &[&str] = &[
    // The first run and its downloads.
    "app_opened",
    "models_download_started",
    "speech_model_ready",
    "notes_model_ready",
    // Meetings.
    "recording_started",
    "recording_saved",
    "transcript_ready",
    "notes_ready",
    "export_saved",
    "meeting_shared",
    "meeting_imported",
    // Projects and the edition that files and prepares (counted by it).
    "filed_automatically",
    "suggested",
    "suggestion_accepted",
    "filing_changed",
    "brief_prepared",
    "brief_with_source",
    // Recorded by the desktop interface.
    "source_played",
    "brief_opened",
    "brief_source_played",
];

/// The events the desktop interface may record through `usage_record`.
const INTERFACE_EVENTS: &[&str] = &["source_played", "brief_opened", "brief_source_played"];

fn week(at: chrono::DateTime<chrono::Utc>) -> String {
    use chrono::Datelike;
    let week = at.iso_week();
    format!("{}-W{:02}", week.year(), week.week())
}

/// Count one occurrence. Counting never fails the work it describes.
pub async fn count(pool: &SqlitePool, event: &str) {
    if !EVENTS.contains(&event) {
        return;
    }
    let now = chrono::Utc::now();
    let at = now.to_rfc3339();
    let result = sqlx::query("INSERT INTO usage_counts(event,count,first_at,last_at) VALUES(?,1,?,?) ON CONFLICT(event) DO UPDATE SET count=count+1,last_at=excluded.last_at")
        .bind(event).bind(&at).bind(&at).execute(pool).await;
    let weekly = sqlx::query("INSERT INTO usage_weeks(week,event,count) VALUES(?,?,1) ON CONFLICT(week,event) DO UPDATE SET count=count+1")
        .bind(week(now)).bind(event).execute(pool).await;
    if let Err(error) = result.and(weekly) {
        log::debug!("Usage count skipped: {error}");
    }
}

/// Count an event once for its meeting or calendar event, however often the work runs.
pub async fn count_once(pool: &SqlitePool, event: &str, subject: &str) {
    let first = sqlx::query("INSERT OR IGNORE INTO usage_once(event,subject) VALUES(?,?)")
        .bind(event)
        .bind(subject)
        .execute(pool)
        .await
        .is_ok_and(|r| r.rows_affected() == 1);
    if first {
        count(pool, event).await;
    }
}

fn parse(at: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    chrono::DateTime::parse_from_rfc3339(at)
        .ok()
        .map(|d| d.with_timezone(&chrono::Utc))
}

/// The report: totals, the last 26 weeks, and how long the first run took.
pub async fn report(pool: &SqlitePool, edition: &str) -> Result<Value, String> {
    let rows: Vec<(String, i64, String, String)> =
        sqlx::query_as("SELECT event,count,first_at,last_at FROM usage_counts ORDER BY event")
            .fetch_all(pool)
            .await
            .map_err(|_| "Could not read the usage report")?;
    let weeks: Vec<(String, String, i64)> = sqlx::query_as("SELECT week,event,count FROM usage_weeks WHERE week IN (SELECT DISTINCT week FROM usage_weeks ORDER BY week DESC LIMIT 26) ORDER BY week,event")
        .fetch_all(pool).await.map_err(|_| "Could not read the usage report")?;
    let first = |event: &str| rows.iter().find(|r| r.0 == event).and_then(|r| parse(&r.2));
    let minutes = |from: &str, to: &str| match (first(from), first(to)) {
        (Some(a), Some(b)) if b >= a => json!((b - a).num_minutes()),
        _ => Value::Null,
    };
    let download = match (
        first("models_download_started"),
        [first("speech_model_ready"), first("notes_model_ready")]
            .into_iter()
            .flatten()
            .max(),
    ) {
        (Some(a), Some(b)) if b >= a => json!((b - a).num_minutes()),
        _ => Value::Null,
    };
    let days_to_brief = match (first("app_opened"), first("brief_with_source")) {
        (Some(a), Some(b)) if b >= a => json!((b - a).num_days()),
        _ => Value::Null,
    };
    let mut totals = Map::new();
    for (event, count, _, _) in &rows {
        totals.insert(event.clone(), json!(count));
    }
    let mut by_week: Vec<(String, Map<String, Value>)> = Vec::new();
    for (week, event, count) in weeks {
        match by_week.last_mut() {
            Some((last, counts)) if *last == week => {
                counts.insert(event, json!(count));
            }
            _ => {
                let mut counts = Map::new();
                counts.insert(event, json!(count));
                by_week.push((week, counts));
            }
        }
    }
    let weeks_with_meetings = by_week
        .iter()
        .filter(|(_, c)| c.get("recording_started").is_some())
        .count();
    Ok(json!({
        "format": "overtalk-usage-v1",
        "generated_at": chrono::Utc::now().to_rfc3339(),
        "app_version": env!("CARGO_PKG_VERSION"),
        "edition": edition,
        "first_opened": first("app_opened").map(|d| d.date_naive().to_string()),
        "first_run": {
            "minutes_from_first_open_to_first_recording": minutes("app_opened", "recording_started"),
            "model_download_minutes": download,
            "minutes_from_first_recording_to_first_transcript": minutes("recording_started", "transcript_ready"),
            "minutes_from_first_recording_to_first_notes": minutes("recording_started", "notes_ready"),
            "days_from_first_open_to_first_brief_with_a_source": days_to_brief,
        },
        "weeks_with_meetings": weeks_with_meetings,
        "totals": totals,
        "weeks": by_week.into_iter().map(|(week, counts)| json!({"week": week, "counts": counts})).collect::<Vec<_>>(),
        "contains": "Counts and dates only: no meeting titles, names, text or audio. Kept on this computer; it leaves only if you send it.",
    }))
}

fn edition() -> String {
    crate::addon::installed()
        .and_then(|addon| addon.edition()["name"].as_str().map(str::to_owned))
        .unwrap_or_else(|| "open".into())
}

#[tauri::command]
pub async fn usage_report(state: State<'_, crate::state::AppState>) -> Result<Value, String> {
    report(state.db_manager.pool(), &edition()).await
}

/// Count something the person did in the desktop interface, such as playing a source.
#[tauri::command]
pub async fn usage_record(
    state: State<'_, crate::state::AppState>,
    event: String,
) -> Result<(), String> {
    if !INTERFACE_EVENTS.contains(&event.as_str()) {
        return Err("This is not something the usage report counts".into());
    }
    count(state.db_manager.pool(), &event).await;
    Ok(())
}

/// Save the report to a file the person chooses.
#[tauri::command]
pub async fn usage_export<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, crate::state::AppState>,
) -> Result<Value, String> {
    let report = report(state.db_manager.pool(), &edition()).await?;
    let body = serde_json::to_vec_pretty(&report).map_err(|_| "Could not create the report")?;
    let name = format!(
        "overtalk-usage-{}.json",
        chrono::Local::now().format("%Y-%m-%d")
    );
    let chosen = tokio::task::spawn_blocking(move || {
        app.dialog().file().set_file_name(name).blocking_save_file()
    })
    .await
    .map_err(|_| "File picker stopped")?;
    let Some(chosen) = chosen else {
        return Ok(json!({"cancelled":true}));
    };
    let path = chosen.into_path().map_err(|_| "Choose a local file")?;
    let parent = path.parent().ok_or("Choose a folder")?;
    let mut temp =
        tempfile::NamedTempFile::new_in(parent).map_err(|_| "Could not save the report")?;
    use std::io::Write;
    temp.write_all(&body)
        .map_err(|_| "Could not save the report")?;
    temp.persist(&path)
        .map_err(|_| "Could not save the report")?;
    Ok(json!({"saved":path.display().to_string()}))
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
        pool
    }

    #[tokio::test]
    async fn counts_only_known_events_and_once_per_meeting() {
        let pool = database().await;
        count(&pool, "recording_started").await;
        count(&pool, "recording_started").await;
        count(&pool, "meeting title or anything else").await;
        count_once(&pool, "transcript_ready", "meeting-a").await;
        count_once(&pool, "transcript_ready", "meeting-a").await;
        count_once(&pool, "transcript_ready", "meeting-b").await;
        let report = report(&pool, "open").await.unwrap();
        assert_eq!(
            report["totals"],
            json!({"recording_started": 2, "transcript_ready": 2})
        );
        assert_eq!(report["weeks"].as_array().unwrap().len(), 1);
        assert_eq!(report["weeks"][0]["counts"]["recording_started"], 2);
        assert_eq!(report["weeks_with_meetings"], 1);
        assert!(!report.to_string().contains("meeting-a"));
    }

    #[tokio::test]
    async fn first_run_minutes_come_from_first_occurrences() {
        let pool = database().await;
        for (event, at) in [
            ("app_opened", "2026-10-20T09:00:00+00:00"),
            ("models_download_started", "2026-10-20T09:02:00+00:00"),
            ("recording_started", "2026-10-20T09:05:00+00:00"),
            ("speech_model_ready", "2026-10-20T09:07:00+00:00"),
            ("notes_model_ready", "2026-10-20T09:09:00+00:00"),
            ("transcript_ready", "2026-10-20T09:40:00+00:00"),
        ] {
            sqlx::query("INSERT INTO usage_counts VALUES(?,1,?,?)")
                .bind(event)
                .bind(at)
                .bind(at)
                .execute(&pool)
                .await
                .unwrap();
        }
        let first_run = report(&pool, "open").await.unwrap()["first_run"].clone();
        assert_eq!(first_run["minutes_from_first_open_to_first_recording"], 5);
        assert_eq!(first_run["model_download_minutes"], 7);
        assert_eq!(
            first_run["minutes_from_first_recording_to_first_transcript"],
            35
        );
        assert_eq!(
            first_run["minutes_from_first_recording_to_first_notes"],
            Value::Null
        );
    }

    #[test]
    fn the_interface_may_record_only_its_own_events() {
        assert!(INTERFACE_EVENTS.iter().all(|e| EVENTS.contains(e)));
        assert!(!INTERFACE_EVENTS.contains(&"recording_started"));
    }
}
