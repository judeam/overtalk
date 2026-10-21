//! The input snapshot needed to inspect or retry a notes-generation request.
use chrono::Utc;
use sqlx::SqlitePool;

pub(crate) struct SummarySource<'a> {
    pub meeting_id: &'a str,
    pub transcript: &'a str,
    pub provider: &'a str,
    pub model: &'a str,
    pub chunk_size: i32,
    pub overlap: i32,
}

/// Atomically replace the generation input without changing the row's identity
/// or the meeting name maintained by the meeting repository.
pub(crate) async fn save(pool: &SqlitePool, source: SummarySource<'_>) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO transcript_chunks
            (meeting_id, transcript_text, model, model_name, chunk_size, overlap, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT (meeting_id) DO UPDATE SET
            transcript_text = excluded.transcript_text,
            model = excluded.model,
            model_name = excluded.model_name,
            chunk_size = excluded.chunk_size,
            overlap = excluded.overlap,
            created_at = excluded.created_at",
    )
    .bind(source.meeting_id)
    .bind(source.transcript)
    .bind(source.provider)
    .bind(source.model)
    .bind(source.chunk_size)
    .bind(source.overlap)
    .bind(Utc::now())
    .execute(pool)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{save, SummarySource};
    use sqlx::{sqlite::SqlitePoolOptions, Row, SqlitePool};

    async fn database() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::raw_sql(include_str!(
            "../../../migrations/20250916100000_initial_schema.sql"
        ))
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO meetings VALUES ('meeting', 'Title', 'created', 'updated')")
            .execute(&pool)
            .await
            .unwrap();
        pool
    }

    fn source(meeting_id: &str) -> SummarySource<'_> {
        SummarySource {
            meeting_id,
            transcript: "Saved source",
            provider: "local",
            model: "test-model",
            chunk_size: 40_000,
            overlap: 1_000,
        }
    }

    #[tokio::test]
    async fn regeneration_updates_the_snapshot_without_replacing_the_row() {
        let pool = database().await;
        save(&pool, source("meeting")).await.unwrap();
        sqlx::query("UPDATE transcript_chunks SET meeting_name = 'User title', created_at = 'old'")
            .execute(&pool)
            .await
            .unwrap();
        let original_row: i64 = sqlx::query_scalar("SELECT rowid FROM transcript_chunks")
            .fetch_one(&pool)
            .await
            .unwrap();

        save(
            &pool,
            SummarySource {
                transcript: "Corrected source: ? ' ; DROP TABLE meetings; --",
                provider: "remote",
                model: "different-model",
                chunk_size: 8_000,
                overlap: 200,
                ..source("meeting")
            },
        )
        .await
        .unwrap();
        let row = sqlx::query("SELECT rowid, * FROM transcript_chunks")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(row.get::<i64, _>("rowid"), original_row);
        assert_eq!(row.get::<String, _>("meeting_name"), "User title");
        assert_eq!(
            row.get::<String, _>("transcript_text"),
            "Corrected source: ? ' ; DROP TABLE meetings; --"
        );
        assert_eq!(row.get::<String, _>("model"), "remote");
        assert_eq!(row.get::<String, _>("model_name"), "different-model");
        assert_eq!(row.get::<i32, _>("chunk_size"), 8_000);
        assert_eq!(row.get::<i32, _>("overlap"), 200);
        assert_ne!(row.get::<String, _>("created_at"), "old");
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM meetings")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn missing_meetings_are_rejected_and_existing_snapshots_survive() {
        let pool = database().await;
        save(&pool, source("meeting")).await.unwrap();
        assert!(save(&pool, source("missing")).await.is_err());
        let rows: Vec<(String, String)> =
            sqlx::query_as("SELECT meeting_id, transcript_text FROM transcript_chunks")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(rows, [("meeting".into(), "Saved source".into())]);
    }
}
