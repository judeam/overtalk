use crate::database::models::SummaryProcess;
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::SqlitePool;
use tracing::{error, info as log_info};

pub struct SummaryProcessesRepository;

impl SummaryProcessesRepository {
    /// Retrieves the current summary process state for a given meeting ID.
    pub async fn get_summary_data(
        pool: &SqlitePool,
        meeting_id: &str,
    ) -> Result<Option<SummaryProcess>, sqlx::Error> {
        sqlx::query_as::<_, SummaryProcess>("SELECT * FROM summary_processes WHERE meeting_id = ?")
            .bind(meeting_id)
            .fetch_optional(pool)
            .await
    }

    pub async fn update_meeting_summary(
        pool: &SqlitePool,
        meeting_id: &str,
        summary: &Value,
    ) -> Result<bool, sqlx::Error> {
        let mut transaction = crate::database::begin_write(pool).await?;

        let meeting_exists: bool = sqlx::query("SELECT 1 FROM meetings WHERE id = ?")
            .bind(meeting_id)
            .fetch_optional(&mut *transaction)
            .await?
            .is_some();

        if !meeting_exists {
            log_info!(
                "Attempted to save summary for a non-existent meeting_id: {}",
                meeting_id
            );
            transaction.rollback().await?;
            return Ok(false);
        }

        let result_json = serde_json::to_string(summary);
        if result_json.is_err() {
            error!("Can't convert the json to string for saving to Database");
            transaction.rollback().await?;
            return Ok(false);
        }
        let now = Utc::now();
        // Writing notes does not require an earlier AI generation. Keep an
        // existing generation's bookkeeping intact when editing its result.
        sqlx::query(
            "INSERT INTO summary_processes(meeting_id, status, created_at, updated_at, result)
             VALUES (?, 'completed', ?, ?, ?)
             ON CONFLICT(meeting_id) DO UPDATE SET result=excluded.result, updated_at=excluded.updated_at"
        )
        .bind(meeting_id)
        .bind(now)
        .bind(now)
        .bind(result_json.unwrap())
        .execute(&mut *transaction)
        .await?;

        sqlx::query("UPDATE meetings SET updated_at = ? WHERE id = ?")
            .bind(now)
            .bind(meeting_id)
            .execute(&mut *transaction)
            .await?;

        transaction.commit().await?;

        log_info!(
            "Successfully updated summary and timestamp for meeting_id: {}",
            meeting_id
        );
        Ok(true)
    }

    pub async fn get_summary_data_for_meeting(
        pool: &SqlitePool,
        meeting_id: &str,
    ) -> Result<Option<SummaryProcess>, sqlx::Error> {
        sqlx::query_as::<_, SummaryProcess>(
            "SELECT p.* FROM summary_processes p JOIN meetings m ON p.meeting_id = m.id WHERE p.meeting_id = ?",
        )
        .bind(meeting_id)
        .fetch_optional(pool)
        .await
    }

    pub async fn create_or_reset_process(
        pool: &SqlitePool,
        meeting_id: &str,
        started_at: DateTime<Utc>,
    ) -> Result<(), sqlx::Error> {
        log_info!(
            "Creating or resetting summary process for meeting_id: {}",
            meeting_id
        );
        sqlx::query(
            r#"
            INSERT INTO summary_processes (meeting_id, status, created_at, updated_at, start_time, result, error)
            VALUES (?, 'PENDING', ?, ?, ?, NULL, NULL)
            ON CONFLICT(meeting_id) DO UPDATE SET
                status = 'PENDING',
                updated_at = excluded.updated_at,
                start_time = excluded.start_time,
                result_backup = result,
                result_backup_timestamp = excluded.updated_at,
                result = result,
                error = NULL
            "#
        )
        .bind(meeting_id)
        .bind(started_at)
        .bind(started_at)
        .bind(started_at)
        .execute(pool)
        .await?;
        log_info!(
            "Backed up existing summary before regeneration for meeting_id: {}",
            meeting_id
        );
        Ok(())
    }

    pub async fn update_process_completed(
        pool: &SqlitePool,
        meeting_id: &str,
        started_at: DateTime<Utc>,
        result: Value,
        chunk_count: i64,
        processing_time: f64,
    ) -> Result<bool, sqlx::Error> {
        let now = Utc::now();
        let result_str = serde_json::to_string(&result)
            .map_err(|e| sqlx::Error::Protocol(format!("Failed to serialize result: {}", e)))?;

        let mut tx = crate::database::begin_write(pool).await?;
        let update = sqlx::query(
            r#"
            UPDATE summary_processes
            SET status = 'completed', result = ?, updated_at = ?, end_time = ?, chunk_count = ?, processing_time = ?, error = NULL, result_backup = NULL, result_backup_timestamp = NULL
            WHERE meeting_id = ? AND start_time = ? AND LOWER(status) = 'pending' AND result IS result_backup
            "#
        )
        .bind(&result_str)
        .bind(now)
        .bind(now)
        .bind(chunk_count)
        .bind(processing_time)
        .bind(meeting_id)
        .bind(started_at)
        .execute(&mut *tx)
        .await?;
        if update.rows_affected() == 0 {
            // A person edited while generation was running. Preserve the live
            // document and retain the generated alternative for review.
            let preserved = sqlx::query(
                "UPDATE summary_processes SET status='completed',result_backup=?,\
                 error='Your edited notes were kept. A generated alternative is available.',\
                 end_time=?,updated_at=? WHERE meeting_id=? AND start_time=? \
                 AND LOWER(status)='pending' AND result IS NOT result_backup",
            )
            .bind(&result_str)
            .bind(now)
            .bind(now)
            .bind(meeting_id)
            .bind(started_at)
            .execute(&mut *tx)
            .await?;
            if preserved.rows_affected() == 1 {
                sqlx::query(
                    "INSERT INTO note_versions(meeting_id,content,created_at,origin) \
                     VALUES(?,?,?,'generated-not-applied')",
                )
                .bind(meeting_id)
                .bind(&result_str)
                .bind(now)
                .execute(&mut *tx)
                .await?;
            }
        }
        tx.commit().await?;
        Ok(update.rows_affected() == 1)
    }

    pub async fn update_process_failed(
        pool: &SqlitePool,
        meeting_id: &str,
        started_at: DateTime<Utc>,
        error: &str,
    ) -> Result<bool, sqlx::Error> {
        let now = Utc::now();

        let update = sqlx::query(
            r#"
            UPDATE summary_processes
            SET
                status = 'failed',
                error = ?,
                updated_at = ?,
                end_time = ?,
                result = CASE WHEN result IS result_backup OR result IS NULL THEN COALESCE(result_backup, result) ELSE result END,
                result_backup = NULL,
                result_backup_timestamp = NULL
            WHERE meeting_id = ? AND start_time = ? AND LOWER(status) = 'pending'
            "#,
        )
        .bind(error)
        .bind(now)
        .bind(now)
        .bind(meeting_id)
        .bind(started_at)
        .execute(pool)
        .await?;
        Ok(update.rows_affected() == 1)
    }

    pub async fn update_process_cancelled(
        pool: &SqlitePool,
        meeting_id: &str,
        started_at: DateTime<Utc>,
    ) -> Result<bool, sqlx::Error> {
        let now = Utc::now();

        let update = sqlx::query(
            r#"
            UPDATE summary_processes
            SET
                status = 'cancelled',
                updated_at = ?,
                end_time = ?,
                error = 'Generation was cancelled by user',
                result = CASE WHEN result IS result_backup OR result IS NULL THEN COALESCE(result_backup, result) ELSE result END,
                result_backup = NULL,
                result_backup_timestamp = NULL
            WHERE meeting_id = ? AND start_time = ? AND LOWER(status) = 'pending'
            "#,
        )
        .bind(now)
        .bind(now)
        .bind(meeting_id)
        .bind(started_at)
        .execute(pool)
        .await?;
        Ok(update.rows_affected() == 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    async fn test_pool() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::query(
            "CREATE TABLE summary_processes (
                meeting_id TEXT PRIMARY KEY,
                status TEXT NOT NULL,
                created_at TEXT,
                updated_at TEXT,
                start_time TEXT,
                result TEXT,
                result_backup TEXT,
                result_backup_timestamp TEXT,
                end_time TEXT,
                chunk_count INTEGER DEFAULT 0,
                processing_time REAL DEFAULT 0,
                metadata TEXT,
                error TEXT
            )",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("CREATE TABLE note_versions(id INTEGER PRIMARY KEY,meeting_id TEXT,content TEXT,created_at TEXT,origin TEXT)").execute(&pool).await.unwrap();
        pool
    }

    async fn seed_pending(
        pool: &SqlitePool,
        meeting_id: &str,
        started_at: DateTime<Utc>,
        result: Option<&str>,
        result_backup: Option<&str>,
    ) {
        sqlx::query(
            "INSERT INTO summary_processes (
                meeting_id, status, created_at, updated_at, start_time, result, result_backup
            ) VALUES (?, 'PENDING', ?, ?, ?, ?, ?)",
        )
        .bind(meeting_id)
        .bind(started_at)
        .bind(started_at)
        .bind(started_at)
        .bind(result)
        .bind(result_backup)
        .execute(pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn manual_notes_can_be_saved_before_any_generation() {
        let pool = test_pool().await;
        sqlx::query("CREATE TABLE meetings (id TEXT PRIMARY KEY, updated_at TEXT)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO meetings(id) VALUES ('manual-notes')")
            .execute(&pool)
            .await
            .unwrap();
        let notes = json!({"markdown": "# Notes\n\nAn independently written meeting note."});
        assert!(
            SummaryProcessesRepository::update_meeting_summary(&pool, "manual-notes", &notes)
                .await
                .unwrap()
        );
        let (status, saved): (String, String) = sqlx::query_as(
            "SELECT status, result FROM summary_processes WHERE meeting_id = 'manual-notes'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(status, "completed");
        assert_eq!(serde_json::from_str::<Value>(&saved).unwrap(), notes);
        assert!(
            SummaryProcessesRepository::get_summary_data_for_meeting(&pool, "manual-notes")
                .await
                .unwrap()
                .is_some()
        );
        assert!(
            !SummaryProcessesRepository::update_meeting_summary(&pool, "missing", &notes)
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn terminal_compare_and_set_preserves_first_writer() {
        let pool = test_pool().await;
        let completed_start = Utc::now();
        seed_pending(&pool, "completed-first", completed_start, None, None).await;
        assert!(SummaryProcessesRepository::update_process_completed(
            &pool,
            "completed-first",
            completed_start,
            json!({"markdown": "completed"}),
            1,
            1.0,
        )
        .await
        .unwrap());
        assert!(!SummaryProcessesRepository::update_process_cancelled(
            &pool,
            "completed-first",
            completed_start,
        )
        .await
        .unwrap());
        let completed_status: String = sqlx::query_scalar(
            "SELECT status FROM summary_processes WHERE meeting_id = 'completed-first'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(completed_status, "completed");

        let cancelled_start = completed_start + chrono::Duration::nanoseconds(1);
        let previous = r#"{"markdown":"previous"}"#;
        seed_pending(
            &pool,
            "cancelled-first",
            cancelled_start,
            Some(previous),
            Some(previous),
        )
        .await;
        assert!(SummaryProcessesRepository::update_process_cancelled(
            &pool,
            "cancelled-first",
            cancelled_start,
        )
        .await
        .unwrap());
        assert!(!SummaryProcessesRepository::update_process_completed(
            &pool,
            "cancelled-first",
            cancelled_start,
            json!({"markdown": "completed"}),
            1,
            1.0,
        )
        .await
        .unwrap());
        let cancelled: (String, String) = sqlx::query_as(
            "SELECT status, result FROM summary_processes WHERE meeting_id = 'cancelled-first'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(cancelled, ("cancelled".to_string(), previous.to_string()));
    }

    #[tokio::test]
    async fn in_flight_generation_and_cancellation_preserve_manual_edits() {
        let pool = test_pool().await;
        let start = Utc::now();
        let original = r#"{"markdown":"Original"}"#;
        let edited = r#"{"markdown":"My correction","manual":true}"#;
        seed_pending(&pool, "edit", start, Some(edited), Some(original)).await;
        let generated = serde_json::json!({"markdown":"Generated alternative"});
        assert!(!SummaryProcessesRepository::update_process_completed(
            &pool,
            "edit",
            start,
            generated.clone(),
            1,
            1.
        )
        .await
        .unwrap());
        let (status, result, backup): (String, String, String) = sqlx::query_as(
            "SELECT status,result,result_backup FROM summary_processes WHERE meeting_id='edit'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(status, "completed");
        assert_eq!(result, edited);
        assert_eq!(serde_json::from_str::<Value>(&backup).unwrap(), generated);
        seed_pending(&pool, "cancel-edit", start, Some(edited), Some(original)).await;
        SummaryProcessesRepository::update_process_cancelled(&pool, "cancel-edit", start)
            .await
            .unwrap();
        let saved: String = sqlx::query_scalar(
            "SELECT result FROM summary_processes WHERE meeting_id='cancel-edit'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(saved, edited);
    }

    #[tokio::test]
    async fn cancelled_compare_and_set_rejects_stale_start_time() {
        let pool = test_pool().await;
        let current_start = Utc::now();
        seed_pending(&pool, "stale-cancel", current_start, None, None).await;
        assert!(!SummaryProcessesRepository::update_process_cancelled(
            &pool,
            "stale-cancel",
            current_start - chrono::Duration::nanoseconds(1),
        )
        .await
        .unwrap());
        let status: String = sqlx::query_scalar(
            "SELECT status FROM summary_processes WHERE meeting_id = 'stale-cancel'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(status, "PENDING");
    }
}
