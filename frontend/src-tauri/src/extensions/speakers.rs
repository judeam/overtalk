//! Meeting-local speaker names and conservative assignment of diarization turns.
//! Model IDs denote voices within one meeting; names are supplied by the user.

use crate::state::AppState;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqliteConnection, SqlitePool};
use std::collections::{BTreeMap, BTreeSet};
use tauri::Emitter;

#[derive(Clone, Debug, Serialize, Deserialize, FromRow)]
pub struct MeetingSpeaker {
    pub id: String,
    pub name: String,
    pub segment_count: i64,
    pub identity_source: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DiarizationTurn {
    pub start: f64,
    pub end: f64,
    #[serde(alias = "speaker_id")]
    pub speaker: String,
    #[serde(default)]
    pub confidence: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Attribution {
    pub speaker_id: Option<String>,
    /// Fraction of transcript time supported by turns, not identity probability.
    pub confidence: Option<f64>,
    pub overlap: bool,
}

struct TurnIndex {
    turns: Vec<DiarizationTurn>,
    maximum_ends: Vec<f64>,
}

impl TurnIndex {
    fn new(turns: &[DiarizationTurn]) -> Self {
        let mut turns = turns.to_vec();
        turns.sort_by(|a, b| a.start.total_cmp(&b.start));
        let mut maximum = 0.0_f64;
        let maximum_ends = turns
            .iter()
            .map(|turn| {
                maximum = maximum.max(turn.end);
                maximum
            })
            .collect();
        Self {
            turns,
            maximum_ends,
        }
    }

    fn intersecting(&self, start: f64, end: f64) -> &[DiarizationTurn] {
        if !valid_interval(start, end) {
            return &[];
        }
        let upper = self.turns.partition_point(|turn| turn.start < end);
        let lower = self.maximum_ends[..upper].partition_point(|maximum| *maximum <= start);
        &self.turns[lower..upper]
    }
}

fn valid_interval(start: f64, end: f64) -> bool {
    start.is_finite() && end.is_finite() && start >= 0.0 && end > start
}

pub fn validate_turns(turns: &[DiarizationTurn]) -> Result<(), String> {
    if turns.len() > 100_000 {
        return Err("Too many speaker turns".into());
    }
    for turn in turns {
        if !valid_interval(turn.start, turn.end)
            || turn.speaker.trim().is_empty()
            || turn.speaker.len() > 200
            || turn
                .confidence
                .is_some_and(|c| !c.is_finite() || !(0.0..=1.0).contains(&c))
        {
            return Err("Invalid speaker turn or confidence".into());
        }
    }
    Ok(())
}

/// An equal split, cross-talk, or mostly silence remains unassigned.
pub fn attribute_interval(start: f64, end: f64, turns: &[DiarizationTurn]) -> Attribution {
    let unknown = Attribution {
        speaker_id: None,
        confidence: None,
        overlap: false,
    };
    if !valid_interval(start, end) {
        return unknown;
    }
    let intersecting: Vec<_> = turns
        .iter()
        .filter(|t| t.start < end && t.end > start)
        .collect();
    let overlap = intersecting.iter().enumerate().any(|(index, a)| {
        intersecting[index + 1..].iter().any(|b| {
            a.speaker != b.speaker && start.max(a.start).max(b.start) < end.min(a.end).min(b.end)
        })
    });
    let mut intervals: BTreeMap<&str, Vec<(f64, f64)>> = BTreeMap::new();
    for turn in intersecting {
        intervals
            .entry(&turn.speaker)
            .or_default()
            .push((start.max(turn.start), end.min(turn.end)));
    }
    let mut scores: Vec<_> = intervals
        .into_iter()
        .map(|(speaker, mut spans)| {
            spans.sort_by(|a, b| a.0.total_cmp(&b.0));
            let (mut total, mut right) = (0.0, f64::NEG_INFINITY);
            for (left, next) in spans {
                total += (next - right.max(left)).max(0.0);
                right = right.max(next);
            }
            (speaker, total / (end - start))
        })
        .collect();
    scores.sort_by(|a, b| b.1.total_cmp(&a.1));
    if let Some((speaker, coverage)) = scores.first() {
        let runner_up = scores.get(1).map_or(0.0, |(_, score)| *score);
        if *coverage >= 0.55 && coverage - runner_up >= 0.15 {
            return Attribution {
                speaker_id: Some((*speaker).into()),
                confidence: Some(coverage.min(1.0)),
                overlap,
            };
        }
    }
    Attribution { overlap, ..unknown }
}

pub async fn list_speakers(
    pool: &SqlitePool,
    meeting_id: &str,
) -> Result<Vec<MeetingSpeaker>, String> {
    sqlx::query_as(
        "SELECT s.id, s.name, COUNT(t.id) AS segment_count, \
        CASE WHEN v.confirmed=1 THEN 'remembered' WHEN v.confirmed=0 THEN 'matched' ELSE NULL END AS identity_source \
        FROM meeting_speakers s \
        LEFT JOIN transcripts t ON t.meeting_id = s.meeting_id AND t.speaker_id = s.id \
        LEFT JOIN meeting_voice_matches v ON v.meeting_id=s.meeting_id AND v.speaker_id=s.id \
        WHERE s.meeting_id = ? GROUP BY s.id, s.name ORDER BY s.id",
    )
    .bind(meeting_id)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())
}

/// Usable inside the caller's transaction when replacing transcript text too.
pub async fn apply_diarization_on_connection(
    conn: &mut SqliteConnection,
    meeting_id: &str,
    turns: &[DiarizationTurn],
    exclusive_turns: &[DiarizationTurn],
    model: &str,
) -> Result<usize, String> {
    validate_turns(turns)?;
    validate_turns(exclusive_turns)?;
    if model.trim().is_empty() || model.len() > 200 {
        return Err("Invalid diarization model".into());
    }
    let exists: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM meetings WHERE id = ?")
        .bind(meeting_id)
        .fetch_one(&mut *conn)
        .await
        .map_err(|e| e.to_string())?;
    if exists == 0 {
        return Err("Meeting does not exist".into());
    }
    let ids: BTreeSet<_> = turns.iter().map(|turn| turn.speaker.as_str()).collect();
    if exclusive_turns
        .iter()
        .any(|turn| !ids.contains(turn.speaker.as_str()))
    {
        return Err("Exclusive turns refer to an unknown speaker".into());
    }
    for (index, id) in ids.iter().enumerate() {
        sqlx::query(
            "INSERT INTO meeting_speakers (meeting_id, id, name) VALUES (?, ?, ?) \
            ON CONFLICT(meeting_id, id) DO NOTHING",
        )
        .bind(meeting_id)
        .bind(id)
        .bind(format!("Speaker {}", index + 1))
        .execute(&mut *conn)
        .await
        .map_err(|e| e.to_string())?;
    }
    sqlx::query("DELETE FROM speaker_turns WHERE meeting_id = ?")
        .bind(meeting_id)
        .execute(&mut *conn)
        .await
        .map_err(|e| e.to_string())?;
    for turn in turns {
        sqlx::query("INSERT INTO speaker_turns (meeting_id,speaker_id,start_time,end_time,confidence,model) VALUES (?,?,?,?,?,?)")
            .bind(meeting_id).bind(&turn.speaker).bind(turn.start).bind(turn.end).bind(turn.confidence).bind(model)
            .execute(&mut *conn).await.map_err(|e| e.to_string())?;
    }
    let transcripts: Vec<(String, Option<f64>, Option<f64>)> = sqlx::query_as(
        "SELECT id,audio_start_time,audio_end_time FROM transcripts WHERE meeting_id = ?",
    )
    .bind(meeting_id)
    .fetch_all(&mut *conn)
    .await
    .map_err(|e| e.to_string())?;
    let mut assigned = 0;
    let regular_index = TurnIndex::new(turns);
    let exclusive_index = TurnIndex::new(exclusive_turns);
    for (id, start, end) in transcripts {
        let (start, end) = (start.unwrap_or(-1.0), end.unwrap_or(-1.0));
        let regular = attribute_interval(start, end, regular_index.intersecting(start, end));
        let mut attribution = if exclusive_turns.is_empty() {
            regular.clone()
        } else {
            attribute_interval(start, end, exclusive_index.intersecting(start, end))
        };
        attribution.overlap = regular.overlap;
        assigned += usize::from(attribution.speaker_id.is_some());
        sqlx::query("UPDATE transcripts SET speaker_id = ?, speaker_confidence = ?, overlap = ? WHERE meeting_id = ? AND id = ?")
            .bind(attribution.speaker_id).bind(attribution.confidence).bind(attribution.overlap)
            .bind(meeting_id).bind(id).execute(&mut *conn).await.map_err(|e| e.to_string())?;
    }
    Ok(assigned)
}

pub async fn apply_diarization(
    pool: &SqlitePool,
    meeting_id: &str,
    turns: &[DiarizationTurn],
    exclusive_turns: &[DiarizationTurn],
    model: &str,
) -> Result<usize, String> {
    let mut transaction = pool.begin().await.map_err(|e| e.to_string())?;
    let assigned = apply_diarization_on_connection(
        &mut transaction,
        meeting_id,
        turns,
        exclusive_turns,
        model,
    )
    .await?;
    transaction.commit().await.map_err(|e| e.to_string())?;
    Ok(assigned)
}

pub async fn rename_speaker(
    pool: &SqlitePool,
    meeting_id: &str,
    speaker_id: &str,
    name: &str,
) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 120 || name.chars().any(char::is_control) {
        return Err("Speaker name must contain 1–120 characters without control characters".into());
    }
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
    let changed =
        sqlx::query("UPDATE meeting_speakers SET name = ? WHERE meeting_id = ? AND id = ?")
            .bind(name)
            .bind(meeting_id)
            .bind(speaker_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
    if changed.rows_affected() == 0 {
        return Err("Speaker does not exist in this meeting".into());
    }
    sqlx::query("DELETE FROM meeting_voice_matches WHERE meeting_id=? AND speaker_id=?")
        .bind(meeting_id)
        .bind(speaker_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    tx.commit().await.map_err(|e| e.to_string())?;
    Ok(())
}

/// Revisit only unresolved passages after merging voice labels. Known speaker
/// assignments and transcript text stay untouched; real cross-talk abstains.
pub(crate) async fn fill_unassigned(
    conn: &mut SqliteConnection,
    meeting_id: &str,
) -> Result<usize, String> {
    let turns: Vec<(String, f64, f64)> = sqlx::query_as("SELECT speaker_id,start_time,end_time FROM speaker_turns WHERE meeting_id=? ORDER BY start_time")
        .bind(meeting_id).fetch_all(&mut *conn).await.map_err(|e| e.to_string())?;
    let turns: Vec<_> = turns
        .into_iter()
        .map(|(speaker, start, end)| DiarizationTurn {
            speaker,
            start,
            end,
            confidence: None,
        })
        .collect();
    let index = TurnIndex::new(&turns);
    let rows: Vec<(String, f64, f64)> = sqlx::query_as("SELECT id,audio_start_time,audio_end_time FROM transcripts WHERE meeting_id=? AND speaker_id IS NULL AND audio_start_time IS NOT NULL AND audio_end_time IS NOT NULL")
        .bind(meeting_id).fetch_all(&mut *conn).await.map_err(|e| e.to_string())?;
    let mut count = 0;
    for (id, start, end) in rows {
        let result = attribute_interval(start, end, index.intersecting(start, end));
        if let Some(speaker) = result.speaker_id {
            sqlx::query("UPDATE transcripts SET speaker_id=?,speaker_confidence=?,overlap=? WHERE id=? AND meeting_id=? AND speaker_id IS NULL")
                .bind(speaker).bind(result.confidence).bind(result.overlap).bind(id).bind(meeting_id).execute(&mut *conn).await.map_err(|e| e.to_string())?;
            count += 1;
        }
    }
    Ok(count)
}

pub async fn merge_speakers(
    pool: &SqlitePool,
    meeting_id: &str,
    source_id: &str,
    target_id: &str,
) -> Result<(), String> {
    if source_id == target_id {
        return Err("Choose two different speakers".into());
    }
    let mut transaction = pool.begin().await.map_err(|e| e.to_string())?;
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM meeting_speakers WHERE meeting_id = ? AND id IN (?, ?)",
    )
    .bind(meeting_id)
    .bind(source_id)
    .bind(target_id)
    .fetch_one(&mut *transaction)
    .await
    .map_err(|e| e.to_string())?;
    if count != 2 {
        return Err("Both speakers must belong to this meeting".into());
    }
    super::speech::snapshot_transcript(&mut transaction, meeting_id).await?;
    sqlx::query("UPDATE transcripts SET speaker_id = ? WHERE meeting_id = ? AND speaker_id = ?")
        .bind(target_id)
        .bind(meeting_id)
        .bind(source_id)
        .execute(&mut *transaction)
        .await
        .map_err(|e| e.to_string())?;
    sqlx::query("UPDATE speaker_turns SET speaker_id = ? WHERE meeting_id = ? AND speaker_id = ?")
        .bind(target_id)
        .bind(meeting_id)
        .bind(source_id)
        .execute(&mut *transaction)
        .await
        .map_err(|e| e.to_string())?;
    sqlx::query("DELETE FROM meeting_speakers WHERE meeting_id = ? AND id = ?")
        .bind(meeting_id)
        .bind(source_id)
        .execute(&mut *transaction)
        .await
        .map_err(|e| e.to_string())?;
    fill_unassigned(&mut transaction, meeting_id).await?;
    transaction.commit().await.map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn list_meeting_speakers(
    meeting_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<MeetingSpeaker>, String> {
    list_speakers(state.db_manager.pool(), &meeting_id).await
}

#[tauri::command]
pub async fn rename_meeting_speaker(
    app: tauri::AppHandle,
    meeting_id: String,
    speaker_id: String,
    name: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    rename_speaker(state.db_manager.pool(), &meeting_id, &speaker_id, &name).await?;
    let _ = app.emit("meeting-speakers-updated", &meeting_id);
    Ok(())
}

#[tauri::command]
pub async fn merge_meeting_speakers(
    app: tauri::AppHandle,
    meeting_id: String,
    source_id: String,
    target_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    merge_speakers(state.db_manager.pool(), &meeting_id, &source_id, &target_id).await?;
    let _ = app.emit("meeting-speakers-updated", &meeting_id);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn turn(start: f64, end: f64, speaker: &str) -> DiarizationTurn {
        DiarizationTurn {
            start,
            end,
            speaker: speaker.into(),
            confidence: None,
        }
    }
    #[test]
    fn overlap_and_equal_split_remain_unknown() {
        let simultaneous =
            attribute_interval(0.0, 1.0, &[turn(0.0, 1.0, "A"), turn(0.0, 1.0, "B")]);
        assert!(simultaneous.overlap);
        assert!(simultaneous.speaker_id.is_none());
        let sequential = attribute_interval(0.0, 2.0, &[turn(0.0, 1.0, "A"), turn(1.0, 2.0, "B")]);
        assert!(!sequential.overlap);
        assert!(sequential.speaker_id.is_none());
    }
    #[test]
    fn duplicate_turns_do_not_inflate_support() {
        assert!(
            attribute_interval(0.0, 1.0, &[turn(0.0, 0.4, "A"), turn(0.0, 0.4, "A")])
                .speaker_id
                .is_none()
        );
        assert!(validate_turns(&[turn(0.0, f64::NAN, "A")]).is_err());
    }
    async fn database() -> SqlitePool {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::raw_sql("PRAGMA foreign_keys=ON; CREATE TABLE meetings(id TEXT PRIMARY KEY); \
            CREATE TABLE transcripts(id TEXT PRIMARY KEY, meeting_id TEXT, transcript TEXT, speaker TEXT, \
            audio_start_time REAL, audio_end_time REAL, timestamp TEXT, summary TEXT, action_items TEXT, key_points TEXT, duration REAL, is_partial BOOLEAN DEFAULT 0);") .execute(&pool).await.unwrap();
        sqlx::raw_sql(include_str!(
            "../../migrations/20260930000000_speaker_identities.sql"
        ))
        .execute(&pool)
        .await
        .unwrap();
        sqlx::raw_sql(include_str!(
            "../../migrations/20260930010000_speech_backups.sql"
        ))
        .execute(&pool)
        .await
        .unwrap();
        sqlx::raw_sql(include_str!(
            "../../migrations/20261017000000_voice_memory.sql"
        ))
        .execute(&pool)
        .await
        .unwrap();
        sqlx::raw_sql("INSERT INTO meetings VALUES('one'),('two'); \
            INSERT INTO transcripts(id,meeting_id,transcript,speaker,audio_start_time,audio_end_time) VALUES \
            ('a','one','hello','mic',0,1),('b','one','world','system',1,2),('c','two','other','mic',0,1);")
            .execute(&pool).await.unwrap();
        pool
    }
    #[tokio::test]
    async fn merging_reassigns_split_voice_but_keeps_real_overlap_unknown() {
        let pool = database().await;
        sqlx::query("INSERT INTO transcripts(id,meeting_id,transcript,audio_start_time,audio_end_time) VALUES('split','one','Keep this text',0,2),('overlap','one','Two voices',3,4)")
            .execute(&pool).await.unwrap();
        apply_diarization(
            &pool,
            "one",
            &[
                turn(0., 1., "A"),
                turn(1., 2., "B"),
                turn(3., 4., "A"),
                turn(3., 4., "C"),
            ],
            &[],
            "test",
        )
        .await
        .unwrap();
        let before: Option<String> =
            sqlx::query_scalar("SELECT speaker_id FROM transcripts WHERE id='split'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(before, None);
        merge_speakers(&pool, "one", "B", "A").await.unwrap();
        let after: (String, String, f64, f64) = sqlx::query_as("SELECT speaker_id,transcript,audio_start_time,audio_end_time FROM transcripts WHERE id='split'").fetch_one(&pool).await.unwrap();
        assert_eq!(after, ("A".into(), "Keep this text".into(), 0., 2.));
        let overlap: Option<String> =
            sqlx::query_scalar("SELECT speaker_id FROM transcripts WHERE id='overlap'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(overlap, None);
    }

    #[tokio::test]
    async fn names_merges_are_meeting_local_and_preserve_transcripts() {
        let pool = database().await;
        apply_diarization(
            &pool,
            "one",
            &[turn(0.0, 1.0, "A"), turn(1.0, 2.0, "B")],
            &[],
            "test",
        )
        .await
        .unwrap();
        apply_diarization(&pool, "two", &[turn(0.0, 1.0, "A")], &[], "test")
            .await
            .unwrap();
        rename_speaker(&pool, "one", "A", "Alice").await.unwrap();
        assert_eq!(
            list_speakers(&pool, "two").await.unwrap()[0].name,
            "Speaker 1"
        );
        merge_speakers(&pool, "one", "B", "A").await.unwrap();
        assert_eq!(
            list_speakers(&pool, "one").await.unwrap()[0].segment_count,
            2
        );
        let rows: Vec<(String,String,f64,f64)> = sqlx::query_as("SELECT transcript,speaker,audio_start_time,audio_end_time FROM transcripts ORDER BY id")
            .fetch_all(&pool).await.unwrap();
        assert_eq!(
            rows,
            vec![
                ("hello".into(), "mic".into(), 0.0, 1.0),
                ("world".into(), "system".into(), 1.0, 2.0),
                ("other".into(), "mic".into(), 0.0, 1.0)
            ]
        );
        assert!(merge_speakers(&pool, "two", "A", "B").await.is_err());
        assert_eq!(list_speakers(&pool, "two").await.unwrap().len(), 1);
    }
    #[tokio::test]
    async fn invalid_analysis_is_atomic_and_exclusive_keeps_overlap() {
        let pool = database().await;
        apply_diarization(
            &pool,
            "one",
            &[turn(0.0, 1.0, "A"), turn(0.0, 1.0, "B")],
            &[turn(0.0, 1.0, "A")],
            "test",
        )
        .await
        .unwrap();
        let before: (Option<String>, bool) =
            sqlx::query_as("SELECT speaker_id,overlap FROM transcripts WHERE id='a'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(before, (Some("A".into()), true));
        assert!(
            apply_diarization(&pool, "one", &[turn(1.0, 0.0, "C")], &[], "test")
                .await
                .is_err()
        );
        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM speaker_turns WHERE meeting_id='one'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(count, 2);
    }
}
