use crate::api::{TranscriptSearchResult, TranscriptSegment};
use chrono::Utc;
use sqlx::{Error as SqlxError, SqlitePool};
use tracing::{error, info};
use uuid::Uuid;

pub struct TranscriptsRepository;

impl TranscriptsRepository {
    /// A stable recording ID lets the native service safely retry a saved journal
    /// after a crash between the database commit and journal cleanup.
    pub(crate) async fn save_transcript_with_id(
        pool: &SqlitePool,
        meeting_id: &str,
        meeting_title: &str,
        transcripts: &[TranscriptSegment],
        folder_path: Option<String>,
    ) -> Result<String, SqlxError> {
        Self::save_import(
            pool,
            meeting_id,
            meeting_title,
            transcripts,
            folder_path,
            &[],
        )
        .await
    }

    /// Import already analyzed audio without rerunning ASR or changing its
    /// assignments. Timeline turns and passages are committed together.
    pub async fn save_transcript_with_timeline(
        pool: &SqlitePool,
        meeting_title: &str,
        transcripts: &[TranscriptSegment],
        folder_path: Option<String>,
        turns: &[crate::speakers::DiarizationTurn],
    ) -> Result<String, SqlxError> {
        let meeting_id = format!("meeting-{}", Uuid::new_v4());
        Self::save_import(
            pool,
            &meeting_id,
            meeting_title,
            transcripts,
            folder_path,
            turns,
        )
        .await
    }

    async fn save_import(
        pool: &SqlitePool,
        meeting_id: &str,
        meeting_title: &str,
        transcripts: &[TranscriptSegment],
        folder_path: Option<String>,
        turns: &[crate::speakers::DiarizationTurn],
    ) -> Result<String, SqlxError> {
        crate::speakers::validate_turns(turns).map_err(SqlxError::Protocol)?;

        let mut transaction = crate::database::begin_write(pool).await?;

        let now = Utc::now();

        // 1. Create the new meeting
        let result = sqlx::query(
            "INSERT INTO meetings (id, title, created_at, updated_at, folder_path) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(meeting_id)
        .bind(meeting_title)
        .bind(now)
        .bind(now)
        .bind(&folder_path)
        .execute(&mut *transaction)
        .await;

        if let Err(e) = result {
            error!("Failed to create meeting '{}': {}", meeting_title, e);
            transaction.rollback().await?;
            return Err(e);
        }

        info!("Successfully created meeting with id: {}", meeting_id);

        // 2. Save each transcript segment with audio timing fields
        for (ordinal, segment) in transcripts.iter().enumerate() {
            if let Some(speaker_id) = segment.speaker_id.as_deref() {
                let fallback = speaker_id
                    .strip_prefix("SPEAKER_")
                    .and_then(|index| index.parse::<usize>().ok())
                    .map(|index| format!("Speaker {}", index.saturating_add(1)))
                    .unwrap_or_else(|| speaker_id.to_string());
                let name = segment
                    .speaker_name
                    .as_deref()
                    .filter(|name| !name.trim().is_empty())
                    .unwrap_or(&fallback);
                sqlx::query("INSERT INTO meeting_speakers (meeting_id, id, name) VALUES (?, ?, ?) ON CONFLICT(meeting_id, id) DO NOTHING")
                    .bind(meeting_id).bind(speaker_id).bind(name)
                    .execute(&mut *transaction).await?;
            }
            // Imported source references depend on stable ordering even where
            // two overlapping passages start at exactly the same time.
            let transcript_id = if turns.is_empty() {
                format!("transcript-{}", Uuid::new_v4())
            } else {
                format!("{meeting_id}-transcript-{ordinal:08}")
            };
            let result = sqlx::query(
                "INSERT INTO transcripts (id, meeting_id, transcript, timestamp, audio_start_time, audio_end_time, duration, speaker, speaker_id, speaker_confidence, overlap, is_partial)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
            )
            .bind(&transcript_id)
            .bind(meeting_id)
            .bind(&segment.text)
            .bind(&segment.timestamp)
            .bind(segment.audio_start_time)
            .bind(segment.audio_end_time)
            .bind(segment.duration)
            .bind(&segment.speaker)
            .bind(&segment.speaker_id)
            .bind(segment.speaker_confidence)
            .bind(segment.overlap)
            .bind(segment.is_partial)
            .execute(&mut *transaction)
            .await;

            if let Err(e) = result {
                error!(
                    "Failed to save transcript segment for meeting {}: {}",
                    meeting_id, e
                );
                transaction.rollback().await?;
                return Err(e);
            }
        }

        info!(
            "Successfully saved {} transcript segments for meeting {}",
            transcripts.len(),
            meeting_id
        );

        for turn in turns {
            let fallback = turn
                .speaker
                .strip_prefix("SPEAKER_")
                .and_then(|index| index.parse::<usize>().ok())
                .map(|index| format!("Speaker {}", index.saturating_add(1)))
                .unwrap_or_else(|| turn.speaker.clone());
            sqlx::query("INSERT INTO meeting_speakers(meeting_id,id,name) VALUES(?,?,?) ON CONFLICT(meeting_id,id) DO NOTHING")
                .bind(meeting_id).bind(&turn.speaker).bind(fallback).execute(&mut *transaction).await?;
            sqlx::query("INSERT INTO speaker_turns(meeting_id,speaker_id,start_time,end_time,confidence,model) VALUES(?,?,?,?,?,'imported-analysis')")
                .bind(meeting_id).bind(&turn.speaker).bind(turn.start).bind(turn.end).bind(turn.confidence)
                .execute(&mut *transaction).await?;
        }

        // Commit the transaction
        transaction.commit().await?;

        Ok(meeting_id.to_owned())
    }

    /// Searches for a query string within the transcripts.
    /// It returns a list of matching transcripts with context.
    pub async fn search_transcripts(
        pool: &SqlitePool,
        query: &str,
    ) -> Result<Vec<TranscriptSearchResult>, SqlxError> {
        if query.trim().is_empty() {
            return Ok(Vec::new());
        }

        // Search literal user text; LIKE metacharacters must not broaden a
        // confidential transcript search into every meeting.
        let escaped = query
            .to_lowercase()
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_");
        let search_query = format!("%{}%", escaped);

        let rows = sqlx::query_as::<_, (String, String, String, String)>(
            "SELECT m.id, m.title, t.transcript, t.timestamp
             FROM meetings m
             JOIN transcripts t ON m.id = t.meeting_id
             WHERE LOWER(t.transcript) LIKE ? ESCAPE '\\'
             ORDER BY m.created_at DESC, t.timestamp DESC
             LIMIT 500",
        )
        .bind(&search_query)
        .fetch_all(pool)
        .await?;

        let results = rows
            .into_iter()
            .map(|(id, title, transcript, timestamp)| {
                let match_context = Self::get_match_context(&transcript, query);
                TranscriptSearchResult {
                    id,
                    title,
                    match_context,
                    timestamp,
                }
            })
            .collect();

        Ok(results)
    }

    /// Helper function to extract a snippet of text around the first match of a query.
    fn get_match_context(transcript: &str, query: &str) -> String {
        // Unicode lowercasing can change byte length (e.g. İ → i + combining
        // dot). Map folded byte offsets back to the original scalar boundaries.
        let mut transcript_lower = String::new();
        let mut original_offsets = Vec::new();
        for (byte, character) in transcript.char_indices() {
            let lowered = character.to_lowercase().collect::<String>();
            original_offsets.extend(std::iter::repeat_n(byte, lowered.len()));
            transcript_lower.push_str(&lowered);
        }
        original_offsets.push(transcript.len());
        let query_lower = query.to_lowercase();

        if query_lower.is_empty() {
            return transcript.chars().take(200).collect();
        }

        match transcript_lower.find(&query_lower) {
            Some(match_index) => {
                let mut scalar_offsets: Vec<usize> =
                    transcript.char_indices().map(|(byte, _)| byte).collect();
                scalar_offsets.push(transcript.len());
                let matched_start = original_offsets[match_index];
                let last_matched = original_offsets[match_index + query_lower.len() - 1];
                let matched_end = last_matched
                    + transcript[last_matched..]
                        .chars()
                        .next()
                        .map_or(0, char::len_utf8);
                let start_scalar = scalar_offsets
                    .partition_point(|byte| *byte < matched_start)
                    .saturating_sub(100);
                let end_scalar = (scalar_offsets.partition_point(|byte| *byte < matched_end) + 100)
                    .min(scalar_offsets.len() - 1);
                let start_index = scalar_offsets[start_scalar];
                let end_index = scalar_offsets[end_scalar];

                let mut context = String::new();
                if start_index > 0 {
                    context.push_str("...");
                }
                context.push_str(&transcript[start_index..end_index]);
                if end_index < transcript.len() {
                    context.push_str("...");
                }
                context
            }
            None => transcript.chars().take(200).collect(), // Fallback to the start of the transcript
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The service's first request often arrives while start-up jobs write.
    /// Preparing the meetings insert on a new connection makes FTS5 read its
    /// configuration through the meeting triggers; inside a deferred BEGIN that
    /// read became a snapshot, and SQLite then refused the write at once
    /// ("database is locked") instead of waiting out the busy timeout.
    #[tokio::test]
    async fn first_save_on_a_new_connection_waits_for_a_concurrent_writer() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("meeting_minutes.sqlite");
        let legacy = directory.path().join("meeting_minutes.db");
        let (path, legacy) = (path.to_str().unwrap(), legacy.to_str().unwrap());
        // Create and migrate the library, then reopen it as the service does,
        // so no pooled connection has loaded the full-text index yet.
        crate::database::manager::DatabaseManager::new(path, legacy)
            .await
            .unwrap()
            .pool()
            .close()
            .await;
        let manager = crate::database::manager::DatabaseManager::new(path, legacy)
            .await
            .unwrap();
        let pool = manager.pool().clone();
        let mut writer = pool.begin().await.unwrap();
        sqlx::query(
            "INSERT INTO usage_counts(event,count,first_at,last_at) VALUES('app_opened',1,'','')",
        )
        .execute(&mut *writer)
        .await
        .unwrap();
        let saving = pool.clone();
        let save = tokio::spawn(async move {
            TranscriptsRepository::save_transcript_with_id(
                &saving,
                "meeting-first",
                "First",
                &[],
                None,
            )
            .await
        });
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        if save.is_finished() {
            panic!("The save did not wait for the writer: {:?}", save.await);
        }
        writer.commit().await.unwrap();
        assert_eq!(save.await.unwrap().unwrap(), "meeting-first");
    }

    #[tokio::test]
    async fn importing_timeline_preserves_unassigned_passages_and_validates_before_write() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let segments = vec![TranscriptSegment {
            id: "source-1".into(),
            text: "Uncertain overlapping speech".into(),
            timestamp: "00:01".into(),
            audio_start_time: Some(1.0),
            audio_end_time: Some(2.0),
            duration: Some(1.0),
            speaker: None,
            speaker_id: None,
            speaker_name: None,
            speaker_confidence: None,
            overlap: true,
            is_partial: false,
        }];
        let turns = vec![crate::speakers::DiarizationTurn {
            start: 0.5,
            end: 3.0,
            speaker: "SPEAKER_00".into(),
            confidence: Some(0.8),
        }];
        let id = TranscriptsRepository::save_transcript_with_timeline(
            &pool, "Imported", &segments, None, &turns,
        )
        .await
        .unwrap();
        let saved: (Option<String>, bool) =
            sqlx::query_as("SELECT speaker_id,overlap FROM transcripts WHERE meeting_id=?")
                .bind(&id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(saved, (None, true));
        let timeline = crate::extensions::timeline::load_timeline(&pool, &id)
            .await
            .unwrap();
        assert_eq!(timeline.turns.len(), 1);
        assert_eq!(timeline.turns[0].end, 3.0);
        let invalid = vec![crate::speakers::DiarizationTurn {
            start: 3.0,
            end: 1.0,
            ..turns[0].clone()
        }];
        assert!(TranscriptsRepository::save_transcript_with_timeline(
            &pool, "Invalid", &segments, None, &invalid
        )
        .await
        .is_err());
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM meetings")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn saved_live_speakers_preserve_overlap_partial_state_and_times() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let segments = vec![
            TranscriptSegment {
                id: "live:a:0".into(),
                text: "Hello".into(),
                timestamp: "[00:02]".into(),
                audio_start_time: Some(2.0),
                audio_end_time: Some(3.0),
                duration: Some(1.0),
                speaker: None,
                speaker_id: Some("SPEAKER_00".into()),
                speaker_name: Some("Alice".into()),
                speaker_confidence: None,
                overlap: true,
                is_partial: true,
            },
            TranscriptSegment {
                id: "live:a:1".into(),
                text: "Hi".into(),
                timestamp: "[00:02]".into(),
                audio_start_time: Some(2.5),
                audio_end_time: Some(3.5),
                duration: Some(1.0),
                speaker: None,
                speaker_id: Some("SPEAKER_01".into()),
                speaker_name: None,
                speaker_confidence: None,
                overlap: true,
                is_partial: true,
            },
        ];
        let first = TranscriptsRepository::save_transcript_with_id(
            &pool,
            "meeting-first",
            "first",
            &segments,
            None,
        )
        .await
        .unwrap();
        let second = TranscriptsRepository::save_transcript_with_id(
            &pool,
            "meeting-second",
            "second",
            &segments,
            None,
        )
        .await
        .unwrap();
        crate::speakers::rename_speaker(&pool, &first, "SPEAKER_00", "Renamed")
            .await
            .unwrap();
        let name: String = sqlx::query_scalar(
            "SELECT name FROM meeting_speakers WHERE meeting_id=? AND id='SPEAKER_00'",
        )
        .bind(second)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(name, "Alice");
        let unnamed: String = sqlx::query_scalar(
            "SELECT name FROM meeting_speakers WHERE meeting_id=? AND id='SPEAKER_01'",
        )
        .bind(&first)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(unnamed, "Speaker 2");
        let rows: Vec<(String, f64, f64, bool, bool, Option<f64>)> = sqlx::query_as("SELECT transcript,audio_start_time,audio_end_time,overlap,is_partial,speaker_confidence FROM transcripts WHERE meeting_id=? ORDER BY audio_start_time")
            .bind(first).fetch_all(&pool).await.unwrap();
        assert_eq!(
            rows,
            vec![
                ("Hello".into(), 2.0, 3.0, true, true, None),
                ("Hi".into(), 2.5, 3.5, true, true, None)
            ]
        );
    }

    #[test]
    fn unicode_search_context_never_splits_scalars() {
        let text = format!("{}会議の決定😀{}", "東京 ".repeat(100), "要点 ".repeat(100));
        let snippet = TranscriptsRepository::get_match_context(&text, "決定");
        assert!(snippet.contains("会議の決定😀"));
        assert!(snippet.starts_with("..."));
        assert!(snippet.ends_with("..."));
    }

    #[test]
    fn expanding_lowercase_offsets_map_to_the_original_text() {
        let text = format!("{}İSTANBUL {}", "😀".repeat(120), "東京".repeat(120));
        let snippet = TranscriptsRepository::get_match_context(&text, "i\u{307}stanbul");
        assert!(snippet.contains("İSTANBUL"));
        assert!(TranscriptsRepository::get_match_context("会議😀", "missing").contains("会議😀"));
    }
}
