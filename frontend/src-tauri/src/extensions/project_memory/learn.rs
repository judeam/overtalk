//! Learning from the person's own filing. Only a choice someone made teaches:
//! automatic decisions never reinforce themselves.
use super::signals::{Event, Own};
use serde_json::json;
use sqlx::SqlitePool;

/// Large invitation lists (all-hands, conferences) say little about a project.
const MAX_LEARNED_PEOPLE: usize = 15;
const MAX_LEARNED_DOMAINS: usize = 3;

async fn event_for(
    pool: &SqlitePool,
    meeting: Option<&str>,
    event: Option<&str>,
) -> Option<serde_json::Value> {
    if let Some(meeting) = meeting {
        let row: Option<(String, String, String, String, String, String, String)> = sqlx::query_as("SELECT event_id,source,series_id,title,starts_at,organizer,participants FROM meeting_events WHERE meeting_id=?")
            .bind(meeting).fetch_optional(pool).await.ok()?;
        return row.map(|(id, source, series_id, title, starts_at, organizer, participants)| {
            json!({"id":id,"source":source,"series_id":series_id,"title":title,"starts_at":starts_at,"organizer":organizer,"participants":participants})
        });
    }
    let row: Option<(String, String, String, String, String, String, String)> = sqlx::query_as("SELECT id,source,series_id,title,starts_at,organizer,participants FROM workspace_calendar WHERE id=?")
        .bind(event?).fetch_optional(pool).await.ok()?;
    row.map(|(id, source, series_id, title, starts_at, organizer, participants)| {
        json!({"id":id,"source":source,"series_id":series_id,"title":title,"starts_at":starts_at,"organizer":organizer,"participants":participants})
    })
}

async fn remember(pool: &SqlitePool, project: &str, kind: &str, value: &str, label: &str) {
    let Ok(value) = crate::extensions::projects::rule_value(kind, value) else {
        return;
    };
    let _ = sqlx::query(
        "INSERT INTO project_match_rules(id,project_id,kind,value,label,origin,confirmations,\
         updated_at) VALUES(?,?,?,?,?,'learned',1,?) ON CONFLICT(project_id,kind,value) DO UPDATE \
         SET confirmations=project_match_rules.confirmations+1,label=CASE \
         WHEN project_match_rules.label='' THEN excluded.label ELSE project_match_rules.label \
         END,updated_at=excluded.updated_at",
    )
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(project)
    .bind(kind)
    .bind(value)
    .bind(label.chars().take(160).collect::<String>())
    .bind(super::now())
    .execute(pool)
    .await;
}

pub async fn filed(
    pool: &SqlitePool,
    meeting: Option<&str>,
    event: Option<&str>,
    project: Option<&str>,
) {
    // A meeting the person filed, or deliberately left without a project, is
    // never filed automatically afterwards.
    if let Some(meeting) = meeting {
        let _ = sqlx::query("INSERT INTO filing_log(meeting_id,outcome,project_id,reason,decided_at) VALUES(?,'person',?,'',?) ON CONFLICT(meeting_id) DO UPDATE SET outcome=excluded.outcome,project_id=excluded.project_id,decided_at=excluded.decided_at")
            .bind(meeting).bind(project).bind(super::now()).execute(pool).await;
    }
    let Some(project) = project else {
        return;
    };
    let Some(row) = event_for(pool, meeting, event).await else {
        return;
    };
    let own = Own::load(pool).await;
    let event = Event::from_row(&row, &own);
    if !event.series_id.is_empty() {
        remember(pool, project, "series", &event.series_id, &event.title).await;
    }
    let guests: Vec<_> = event.guests().collect();
    if guests.len() <= MAX_LEARNED_PEOPLE {
        for guest in guests {
            remember(pool, project, "email", &guest.email, &guest.label()).await;
        }
        let domains = event.domains(&own);
        if domains.len() <= MAX_LEARNED_DOMAINS {
            for domain in domains {
                remember(pool, project, "domain", &domain, &domain).await;
            }
        }
    }
    if !event.source.is_empty() {
        let connection = event.source.split_once(':').map(|(_, id)| id).unwrap_or("");
        let label: Option<String> =
            sqlx::query_scalar("SELECT name FROM workspace_connections WHERE id=?")
                .bind(connection)
                .fetch_optional(pool)
                .await
                .ok()
                .flatten();
        remember(
            pool,
            project,
            "account",
            &event.source,
            label.as_deref().unwrap_or("imported"),
        )
        .await;
    }
}
