//! A restore changes meeting content, not current privacy decisions or audit history.
use futures_util::TryStreamExt;
use sqlx::{Row, SqlitePool};
use std::path::PathBuf;

pub async fn reconcile(
    current: &SqlitePool,
    staged: &SqlitePool,
    roots: &[PathBuf],
) -> Result<(), String> {
    sqlx::migrate!("./migrations")
        .run(staged)
        .await
        .map_err(|_| "Could not migrate restored privacy state")?;
    let policy: (String, String) =
        sqlx::query_as("SELECT credential_namespace,policy FROM privacy_settings WHERE id=1")
            .fetch_one(current)
            .await
            .map_err(|_| "Current privacy policy unavailable")?;
    sqlx::query("UPDATE privacy_settings SET credential_namespace=?,policy=? WHERE id=1")
        .bind(policy.0)
        .bind(policy.1)
        .execute(staged)
        .await
        .map_err(|e| e.to_string())?;
    sqlx::query("DELETE FROM privacy_events")
        .execute(staged)
        .await
        .map_err(|e| e.to_string())?;
    let mut events = sqlx::query(
        "SELECT sequence,occurred_at,actor,operation,object_id,outcome,previous_hash,event_hash \
         FROM privacy_events ORDER BY sequence",
    )
    .fetch(current);
    while let Some(row) = events.try_next().await.map_err(|e| e.to_string())? {
        sqlx::query("INSERT INTO privacy_events VALUES(?,?,?,?,?,?,?,?)")
            .bind(row.get::<i64, _>(0))
            .bind(row.get::<String, _>(1))
            .bind(row.get::<String, _>(2))
            .bind(row.get::<String, _>(3))
            .bind(row.get::<String, _>(4))
            .bind(row.get::<String, _>(5))
            .bind(row.get::<String, _>(6))
            .bind(row.get::<String, _>(7))
            .execute(staged)
            .await
            .map_err(|e| e.to_string())?;
    }
    drop(events);
    sqlx::query("DELETE FROM privacy_authorizations")
        .execute(staged)
        .await
        .map_err(|e| e.to_string())?;
    let mut authorizations = sqlx::query(
        "SELECT id,subject_name,purpose,basis,notice_version,recorded_at,expires_at,withdrawn_at \
         FROM privacy_authorizations",
    )
    .fetch(current);
    while let Some(row) = authorizations.try_next().await.map_err(|e| e.to_string())? {
        sqlx::query("INSERT INTO privacy_authorizations VALUES(?,?,?,?,?,?,?,?)")
            .bind(row.get::<String, _>(0))
            .bind(row.get::<String, _>(1))
            .bind(row.get::<String, _>(2))
            .bind(row.get::<String, _>(3))
            .bind(row.get::<String, _>(4))
            .bind(row.get::<String, _>(5))
            .bind(row.get::<Option<String>, _>(6))
            .bind(row.get::<Option<String>, _>(7))
            .execute(staged)
            .await
            .map_err(|e| e.to_string())?;
    }
    drop(authorizations);
    sqlx::query(
        "DELETE FROM voice_profiles WHERE lower(trim(name)) IN(SELECT lower(trim(subject_name)) \
         FROM privacy_authorizations WHERE withdrawn_at IS NOT NULL) OR id IN(SELECT p.profile_id \
         FROM project_people p JOIN privacy_authorizations a \
         ON lower(trim(p.name))=lower(trim(a.subject_name)) WHERE a.withdrawn_at IS NOT NULL)",
    )
    .execute(staged)
    .await
    .map_err(|e| e.to_string())?;
    let erased: Vec<(String, String, String)> =
        sqlx::query_as("SELECT meeting_id,occurred_at,pending_paths FROM privacy_erasures")
            .fetch_all(current)
            .await
            .map_err(|e| e.to_string())?;
    for (id, at, pending) in erased {
        sqlx::query("DELETE FROM privacy_holds WHERE meeting_id=?")
            .bind(&id)
            .execute(staged)
            .await
            .map_err(|e| e.to_string())?;
        sqlx::query("UPDATE workspace_jobs SET state='cancelled' WHERE meeting_id=?")
            .bind(&id)
            .execute(staged)
            .await
            .map_err(|e| e.to_string())?;
        sqlx::query("UPDATE summary_processes SET status='cancelled' WHERE meeting_id=?")
            .bind(&id)
            .execute(staged)
            .await
            .map_err(|e| e.to_string())?;
        super::erasure::delete(staged, &id, roots).await?;
        sqlx::query(
            "INSERT INTO privacy_erasures(meeting_id,occurred_at,pending_paths) VALUES(?,?,?) \
             ON CONFLICT(meeting_id) DO UPDATE SET occurred_at=excluded.occurred_at,\
             pending_paths=excluded.pending_paths",
        )
        .bind(id)
        .bind(at)
        .bind(pending)
        .execute(staged)
        .await
        .map_err(|e| e.to_string())?;
    }
    let holds: Vec<(String, String, String)> =
        sqlx::query_as("SELECT meeting_id,reason,created_at FROM privacy_holds")
            .fetch_all(current)
            .await
            .map_err(|e| e.to_string())?;
    for (id, reason, at) in holds {
        sqlx::query(
            "INSERT INTO privacy_holds(meeting_id,reason,created_at) SELECT ?,?,? \
             WHERE EXISTS(SELECT 1 FROM meetings WHERE id=?) ON CONFLICT(meeting_id) DO UPDATE \
             SET reason=excluded.reason,created_at=excluded.created_at",
        )
        .bind(&id)
        .bind(reason)
        .bind(at)
        .bind(&id)
        .execute(staged)
        .await
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn restore_keeps_current_privacy_and_does_not_resurrect_erased_meetings() {
        let current = SqlitePool::connect("sqlite::memory:").await.unwrap();
        let staged = SqlitePool::connect("sqlite::memory:").await.unwrap();
        for pool in [&current, &staged] {
            sqlx::migrate!("./migrations").run(pool).await.unwrap();
        }
        sqlx::query("UPDATE privacy_settings SET policy='{\"local_only\":true}'")
            .execute(&current)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO privacy_erasures(meeting_id,occurred_at) VALUES('erased',datetime('now'))",
        )
        .execute(&current)
        .await
        .unwrap();
        sqlx::query("INSERT INTO meetings(id,title,created_at,updated_at) VALUES('erased','synthetic',datetime('now'),datetime('now'))").execute(&staged).await.unwrap();
        reconcile(&current, &staged, &[]).await.unwrap();
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM meetings")
                .fetch_one(&staged)
                .await
                .unwrap(),
            0
        );
        assert!(
            super::super::privacy::policy(&staged)
                .await
                .unwrap()
                .local_only
        );
    }
}
