//! Whole-meeting timeline data for the session view: speaker lanes, the audio times of
//! the transcript lines that summaries cite as [S1], [S2], …, the speaking pattern drawn
//! in the meeting list, and waveform peaks for the clips.

use crate::state::AppState;
use serde::Serialize;
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use tauri::{AppHandle, Manager, State};

/// Peak windows per second of audio returned by `meeting_waveform`.
pub const PEAKS_PER_SECOND: u32 = 10;
const MAX_WAVEFORM_BYTES: usize = 8 * 3600 * PEAKS_PER_SECOND as usize + 1;
/// Bins in the speaking pattern drawn for each meeting in the list.
const PATTERN_BINS: usize = 96;
// Only the most recently opened meeting needs an uncached waveform. Abandoned
// decodes must not accumulate CPU work while browsing a long library.
static WAVEFORM_GENERATION: AtomicU64 = AtomicU64::new(0);
static WAVEFORM_DECODER: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(1);

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct TimelineSpeaker {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct TimelineTurn {
    pub speaker_id: String,
    pub start: f64,
    pub end: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct MeetingTimeline {
    pub duration: f64,
    pub speakers: Vec<TimelineSpeaker>,
    pub turns: Vec<TimelineTurn>,
    /// Audio interval of each transcript line in summary-source order; `None` when untimed.
    pub sources: Vec<Option<(f64, f64)>>,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct PatternBin {
    /// Index into `speakers` of whoever talked most in this bin, or -1 for silence.
    pub speaker: i32,
    /// Fraction of the bin covered by speech.
    pub level: f32,
}

#[derive(Clone, Debug, Serialize)]
pub struct MeetingOverview {
    pub id: String,
    pub title: String,
    pub created_at: String,
    pub duration: f64,
    pub speakers: Vec<TimelineSpeaker>,
    pub pattern: Vec<PatternBin>,
}

#[derive(sqlx::FromRow)]
struct TurnRow {
    meeting_id: String,
    speaker_id: String,
    start_time: f64,
    end_time: f64,
}

#[derive(sqlx::FromRow)]
struct SegmentRow {
    meeting_id: String,
    speaker_id: Option<String>,
    speaker: Option<String>,
    audio_start_time: Option<f64>,
    audio_end_time: Option<f64>,
}

fn source_label(source: &str) -> String {
    match source {
        "mic" | "microphone" => "You".into(),
        "system" | "speaker" => "Others".into(),
        other => other.into(),
    }
}

/// Turns from diarization when present, otherwise the timed transcript lines.
fn turns_for(turns: &[TurnRow], segments: &[SegmentRow]) -> Vec<TimelineTurn> {
    if !turns.is_empty() {
        return turns
            .iter()
            .map(|t| TimelineTurn {
                speaker_id: t.speaker_id.clone(),
                start: t.start_time,
                end: t.end_time,
            })
            .collect();
    }
    segments
        .iter()
        .filter_map(|s| {
            let (start, end) = (s.audio_start_time?, s.audio_end_time?);
            if !(start.is_finite() && end.is_finite() && end > start) {
                return None;
            }
            let speaker = s
                .speaker_id
                .clone()
                .or_else(|| s.speaker.clone())
                .unwrap_or_else(|| "unknown".into());
            Some(TimelineTurn {
                speaker_id: speaker,
                start,
                end,
            })
        })
        .collect()
}

/// Speakers in order of first appearance, named from the meeting's speaker table.
fn speakers_for(turns: &[TimelineTurn], names: &HashMap<String, String>) -> Vec<TimelineSpeaker> {
    let mut ordered: Vec<&TimelineTurn> = turns.iter().collect();
    ordered.sort_by(|a, b| a.start.total_cmp(&b.start));
    let mut speakers: Vec<TimelineSpeaker> = Vec::new();
    for turn in ordered {
        if speakers.iter().any(|s| s.id == turn.speaker_id) {
            continue;
        }
        let name = names
            .get(&turn.speaker_id)
            .cloned()
            .unwrap_or_else(|| source_label(&turn.speaker_id));
        speakers.push(TimelineSpeaker {
            id: turn.speaker_id.clone(),
            name,
        });
    }
    speakers
}

pub fn speaking_pattern(
    turns: &[TimelineTurn],
    speakers: &[TimelineSpeaker],
    duration: f64,
    bins: usize,
) -> Vec<PatternBin> {
    if duration <= 0.0 || bins == 0 {
        return Vec::new();
    }
    let width = duration / bins as f64;
    let mut coverage = vec![vec![0.0_f64; speakers.len()]; bins];
    let mut speech = vec![Vec::<(f64, f64)>::new(); bins];
    for turn in turns {
        let Some(index) = speakers.iter().position(|s| s.id == turn.speaker_id) else {
            continue;
        };
        let first = ((turn.start / width).floor().max(0.0) as usize).min(bins - 1);
        let last = ((turn.end / width).ceil().max(0.0) as usize).min(bins);
        for bin in first..last {
            let (lo, hi) = (bin as f64 * width, (bin + 1) as f64 * width);
            let overlap = turn.end.min(hi) - turn.start.max(lo);
            if overlap > 0.0 {
                coverage[bin][index] += overlap;
                speech[bin].push((turn.start.max(lo), turn.end.min(hi)));
            }
        }
    }
    coverage
        .iter()
        .zip(speech.iter_mut())
        .map(|(totals, spans)| {
            // Union of spans so two people talking at once do not count twice.
            spans.sort_by(|a, b| a.0.total_cmp(&b.0));
            let mut covered = 0.0;
            let mut current: Option<(f64, f64)> = None;
            for &(lo, hi) in spans.iter() {
                current = match current {
                    Some((a, b)) if lo <= b => Some((a, b.max(hi))),
                    Some((a, b)) => {
                        covered += b - a;
                        Some((lo, hi))
                    }
                    None => Some((lo, hi)),
                };
            }
            if let Some((a, b)) = current {
                covered += b - a;
            }
            let speaker = totals
                .iter()
                .enumerate()
                .filter(|(_, v)| **v > 0.0)
                .max_by(|a, b| a.1.total_cmp(b.1))
                .map_or(-1, |(i, _)| i as i32);
            PatternBin {
                speaker,
                level: (covered / width).clamp(0.0, 1.0) as f32,
            }
        })
        .collect()
}

async fn speaker_names(
    pool: &SqlitePool,
    meeting_id: Option<&str>,
) -> Result<HashMap<(String, String), String>, String> {
    let rows: Vec<(String, String, String)> = match meeting_id {
        Some(id) => {
            sqlx::query_as("SELECT meeting_id, id, name FROM meeting_speakers WHERE meeting_id = ?")
                .bind(id)
                .fetch_all(pool)
                .await
        }
        None => {
            sqlx::query_as("SELECT meeting_id, id, name FROM meeting_speakers")
                .fetch_all(pool)
                .await
        }
    }
    .map_err(|e| e.to_string())?;
    Ok(rows
        .into_iter()
        .map(|(meeting, id, name)| ((meeting, id), name))
        .collect())
}

fn names_for(all: &HashMap<(String, String), String>, meeting_id: &str) -> HashMap<String, String> {
    all.iter()
        .filter(|((meeting, _), _)| meeting == meeting_id)
        .map(|((_, id), name)| (id.clone(), name.clone()))
        .collect()
}

fn duration_of(turns: &[TimelineTurn], segments: &[SegmentRow]) -> f64 {
    let turn_end = turns.iter().map(|t| t.end).fold(0.0, f64::max);
    let segment_end = segments
        .iter()
        .filter_map(|s| s.audio_end_time)
        .filter(|v| v.is_finite())
        .fold(0.0, f64::max);
    turn_end.max(segment_end)
}

pub async fn load_timeline(pool: &SqlitePool, meeting_id: &str) -> Result<MeetingTimeline, String> {
    let turns: Vec<TurnRow> = sqlx::query_as(
        "SELECT meeting_id, speaker_id, start_time, end_time FROM speaker_turns \
         WHERE meeting_id = ? ORDER BY start_time",
    )
    .bind(meeting_id)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;
    // Same ordering as paginated transcript reads, which number the summary sources.
    let segments: Vec<SegmentRow> = sqlx::query_as(
        "SELECT meeting_id, speaker_id, speaker, audio_start_time, audio_end_time FROM transcripts \
         WHERE meeting_id = ? ORDER BY audio_start_time ASC, id",
    )
    .bind(meeting_id)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;
    let names = names_for(&speaker_names(pool, Some(meeting_id)).await?, meeting_id);
    let timeline_turns = turns_for(&turns, &segments);
    let speakers = speakers_for(&timeline_turns, &names);
    let sources = segments
        .iter()
        .map(|s| match (s.audio_start_time, s.audio_end_time) {
            (Some(a), Some(b)) if a.is_finite() && b.is_finite() => Some((a, b.max(a))),
            (Some(a), None) if a.is_finite() => Some((a, a)),
            _ => None,
        })
        .collect();
    Ok(MeetingTimeline {
        duration: duration_of(&timeline_turns, &segments),
        speakers,
        turns: timeline_turns,
        sources,
    })
}

pub async fn load_overviews(pool: &SqlitePool) -> Result<Vec<MeetingOverview>, String> {
    let meetings: Vec<(String, String, String)> =
        sqlx::query_as("SELECT id, title, created_at FROM meetings WHERE NOT EXISTS(SELECT 1 FROM meeting_workspace_state ws WHERE ws.meeting_id=meetings.id AND ws.trashed_at IS NOT NULL) ORDER BY created_at DESC")
            .fetch_all(pool)
            .await
            .map_err(|e| e.to_string())?;
    let turns: Vec<TurnRow> = sqlx::query_as(
        "SELECT meeting_id, speaker_id, start_time, end_time FROM speaker_turns ORDER BY meeting_id, start_time",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;
    let segments: Vec<SegmentRow> = sqlx::query_as(
        "SELECT meeting_id, speaker_id, speaker, audio_start_time, audio_end_time FROM transcripts \
         ORDER BY meeting_id, audio_start_time",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;
    let names = speaker_names(pool, None).await?;
    let mut turns_by: HashMap<String, Vec<TurnRow>> = HashMap::new();
    for row in turns {
        turns_by
            .entry(row.meeting_id.clone())
            .or_default()
            .push(row);
    }
    let mut segments_by: HashMap<String, Vec<SegmentRow>> = HashMap::new();
    for row in segments {
        segments_by
            .entry(row.meeting_id.clone())
            .or_default()
            .push(row);
    }
    Ok(meetings
        .iter()
        .map(|(id, title, created_at)| {
            let meeting_turns = turns_by.get(id).map_or(&[][..], |v| v.as_slice());
            let meeting_segments = segments_by.get(id).map_or(&[][..], |v| v.as_slice());
            let timeline_turns = turns_for(meeting_turns, meeting_segments);
            let speakers = speakers_for(&timeline_turns, &names_for(&names, id));
            let duration = duration_of(&timeline_turns, meeting_segments);
            MeetingOverview {
                id: id.clone(),
                title: title.clone(),
                created_at: created_at.clone(),
                duration,
                pattern: speaking_pattern(&timeline_turns, &speakers, duration, PATTERN_BINS),
                speakers,
            }
        })
        .collect())
}

#[tauri::command]
pub async fn meeting_timeline(
    meeting_id: String,
    state: State<'_, AppState>,
) -> Result<MeetingTimeline, String> {
    load_timeline(state.db_manager.pool(), &meeting_id).await
}

#[tauri::command]
pub async fn meeting_overviews(
    include_pattern: Option<bool>,
    state: State<'_, AppState>,
) -> Result<Vec<MeetingOverview>, String> {
    if include_pattern.unwrap_or(true) {
        load_overviews(state.db_manager.pool()).await
    } else {
        load_compact_overviews(state.db_manager.pool()).await
    }
}

/// The covering end-time indexes let SQLite find each duration directly, without
/// scanning all transcript rows every time the library is refreshed.
async fn load_compact_overviews(pool: &SqlitePool) -> Result<Vec<MeetingOverview>, String> {
    let rows: Vec<(String, String, String, f64)> = sqlx::query_as(
        "SELECT m.id, m.title, m.created_at, MAX(0.0, \
         COALESCE((SELECT MAX(end_time) FROM speaker_turns WHERE meeting_id=m.id),0.0), \
         COALESCE((SELECT MAX(audio_end_time) FROM transcripts WHERE meeting_id=m.id),0.0)) \
         FROM meetings m \
         WHERE NOT EXISTS(SELECT 1 FROM meeting_workspace_state ws WHERE ws.meeting_id=m.id AND ws.trashed_at IS NOT NULL) \
         ORDER BY m.created_at DESC",
    ).fetch_all(pool).await.map_err(|error| error.to_string())?;
    Ok(rows
        .into_iter()
        .map(|(id, title, created_at, duration)| MeetingOverview {
            id,
            title,
            created_at,
            duration,
            speakers: Vec::new(),
            pattern: Vec::new(),
        })
        .collect())
}

/// Peak amplitude per 1/PEAKS_PER_SECOND window, streamed so long recordings stay small.
pub fn compute_peaks(path: &Path) -> Result<Vec<u8>, String> {
    compute_peaks_while(path, || true)
}

fn compute_peaks_while(path: &Path, is_current: impl Fn() -> bool) -> Result<Vec<u8>, String> {
    let mut reader = crate::audio::decoder::IsolatedPcmReader::open(path, 16000, 2, 0.0)
        .map_err(|e| e.to_string())?;
    let window = (16000 / PEAKS_PER_SECOND) as usize;
    let mut peaks = Vec::new();
    let (mut peak, mut frames) = (0.0_f32, 0_usize);
    loop {
        if !is_current() {
            return Err("Waveform request superseded".into());
        }
        let Some(samples) = reader.read_samples().map_err(|e| e.to_string())? else {
            break;
        };
        for frame in samples.chunks_exact(2) {
            peak = frame
                .iter()
                .fold(peak, |value, sample| value.max(sample.abs()));
            frames += 1;
            if frames == window {
                peaks.push((peak.min(1.0) * 255.0).round() as u8);
                peak = 0.0;
                frames = 0;
                if peaks.len() > 8 * 3600 * PEAKS_PER_SECOND as usize {
                    return Err("Recording exceeds the eight-hour waveform limit".into());
                }
            }
        }
    }
    if frames > 0 {
        peaks.push((peak.min(1.0) * 255.0).round() as u8);
    }
    Ok(peaks)
}

fn cache_path(app: &AppHandle, meeting_id: &str, recording: &Path) -> Option<PathBuf> {
    let meta = std::fs::metadata(recording).ok()?;
    let modified = meta
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    let safe: String = meeting_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let dir = app.path().app_cache_dir().ok()?.join("waveforms");
    Some(dir.join(format!(
        "{safe}-{}-{modified}-{PEAKS_PER_SECOND}.peaks",
        meta.len()
    )))
}

/// Waveform peaks (0–255) at PEAKS_PER_SECOND for the meeting's recording, cached on disk.
#[tauri::command]
pub async fn meeting_waveform(
    app: AppHandle,
    meeting_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<u8>, String> {
    let generation = WAVEFORM_GENERATION.fetch_add(1, Ordering::Relaxed) + 1;
    let folder: Option<Option<String>> =
        sqlx::query_scalar("SELECT folder_path FROM meetings WHERE id=?")
            .bind(&meeting_id)
            .fetch_optional(state.db_manager.pool())
            .await
            .map_err(|e| e.to_string())?;
    let folder = PathBuf::from(folder.flatten().ok_or("This meeting has no saved audio")?);
    let recording =
        crate::audio::retranscription::find_audio_file(&folder).map_err(|e| e.to_string())?;
    let cache = cache_path(&app, &meeting_id, &recording);
    if let Some(bytes) = cache
        .as_ref()
        .and_then(|p| crate::security::storage::read_limited(p, MAX_WAVEFORM_BYTES).ok())
    {
        return Ok(bytes);
    }
    let permit = WAVEFORM_DECODER
        .acquire()
        .await
        .map_err(|e| e.to_string())?;
    // Superseded requests may have waited behind a decode. Do not launch
    // another FFmpeg process for a meeting the user has already left.
    if WAVEFORM_GENERATION.load(Ordering::Relaxed) != generation {
        return Err("Waveform request was superseded".into());
    }
    let peaks = tokio::task::spawn_blocking(move || {
        compute_peaks_while(&recording, || {
            WAVEFORM_GENERATION.load(Ordering::Relaxed) == generation
        })
    })
    .await
    .map_err(|e| e.to_string())??;
    drop(permit);
    if let Some(path) = cache {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = crate::security::storage::write_atomic(&path, |out| {
            std::io::Write::write_all(out, &peaks)
        });
    }
    Ok(peaks)
}

#[derive(Clone, Debug, Serialize, serde::Deserialize, sqlx::FromRow, PartialEq)]
pub struct MeetingMark {
    #[serde(default)]
    pub id: i64,
    pub at: f64,
    #[serde(default)]
    pub label: String,
}

pub async fn list_marks(pool: &SqlitePool, meeting_id: &str) -> Result<Vec<MeetingMark>, String> {
    sqlx::query_as("SELECT id, at, label FROM meeting_marks WHERE meeting_id = ? ORDER BY at, id")
        .bind(meeting_id)
        .fetch_all(pool)
        .await
        .map_err(|e| e.to_string())
}

pub async fn add_marks(
    pool: &SqlitePool,
    meeting_id: &str,
    marks: &[MeetingMark],
) -> Result<Vec<MeetingMark>, String> {
    if marks.len() > 500 {
        return Err("Too many marks at once".into());
    }
    let mut transaction = pool.begin().await.map_err(|e| e.to_string())?;
    for mark in marks {
        if !(mark.at.is_finite() && mark.at >= 0.0) {
            return Err("A mark needs a time from the start of the recording".into());
        }
        let label: String = mark.label.trim().chars().take(200).collect();
        sqlx::query("INSERT INTO meeting_marks (meeting_id, at, label) VALUES (?, ?, ?)")
            .bind(meeting_id)
            .bind(mark.at)
            .bind(label)
            .execute(&mut *transaction)
            .await
            .map_err(|e| e.to_string())?;
    }
    transaction.commit().await.map_err(|e| e.to_string())?;
    list_marks(pool, meeting_id).await
}

#[tauri::command]
pub async fn meeting_marks(
    meeting_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<MeetingMark>, String> {
    list_marks(state.db_manager.pool(), &meeting_id).await
}

#[tauri::command]
pub async fn meeting_add_marks(
    meeting_id: String,
    marks: Vec<MeetingMark>,
    state: State<'_, AppState>,
) -> Result<Vec<MeetingMark>, String> {
    add_marks(state.db_manager.pool(), &meeting_id, &marks).await
}

#[tauri::command]
pub async fn meeting_delete_mark(
    meeting_id: String,
    id: i64,
    state: State<'_, AppState>,
) -> Result<(), String> {
    sqlx::query("DELETE FROM meeting_marks WHERE meeting_id = ? AND id = ?")
        .bind(&meeting_id)
        .bind(id)
        .execute(state.db_manager.pool())
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waveform_decode_stops_when_a_new_meeting_supersedes_it() {
        use std::io::Write;
        let mut file = tempfile::NamedTempFile::new().unwrap();
        let data_size = 16000u32 * 2 * 10;
        file.write_all(b"RIFF").unwrap();
        file.write_all(&(36 + data_size).to_le_bytes()).unwrap();
        file.write_all(b"WAVEfmt ").unwrap();
        file.write_all(&16u32.to_le_bytes()).unwrap();
        file.write_all(&1u16.to_le_bytes()).unwrap();
        file.write_all(&1u16.to_le_bytes()).unwrap();
        file.write_all(&16000u32.to_le_bytes()).unwrap();
        file.write_all(&32000u32.to_le_bytes()).unwrap();
        file.write_all(&2u16.to_le_bytes()).unwrap();
        file.write_all(&16u16.to_le_bytes()).unwrap();
        file.write_all(b"data").unwrap();
        file.write_all(&data_size.to_le_bytes()).unwrap();
        file.write_all(&vec![0; data_size as usize]).unwrap();
        assert_eq!(compute_peaks(file.path()).unwrap().len(), 100);
        let packets = std::sync::atomic::AtomicUsize::new(0);
        let result =
            compute_peaks_while(file.path(), || packets.fetch_add(1, Ordering::Relaxed) < 2);
        assert!(result.unwrap_err().contains("superseded"));
        assert_eq!(packets.load(Ordering::Relaxed), 3);
    }

    #[test]
    fn opposite_phase_stereo_does_not_disappear_from_the_waveform() {
        use std::io::Write;
        let mut file = tempfile::NamedTempFile::new().unwrap();
        let frames = 1600u32;
        let bytes = frames * 4;
        file.write_all(b"RIFF").unwrap();
        file.write_all(&(36 + bytes).to_le_bytes()).unwrap();
        file.write_all(b"WAVEfmt ").unwrap();
        file.write_all(&16u32.to_le_bytes()).unwrap();
        file.write_all(&1u16.to_le_bytes()).unwrap();
        file.write_all(&2u16.to_le_bytes()).unwrap();
        file.write_all(&16000u32.to_le_bytes()).unwrap();
        file.write_all(&64000u32.to_le_bytes()).unwrap();
        file.write_all(&4u16.to_le_bytes()).unwrap();
        file.write_all(&16u16.to_le_bytes()).unwrap();
        file.write_all(b"data").unwrap();
        file.write_all(&bytes.to_le_bytes()).unwrap();
        for _ in 0..frames {
            file.write_all(&30000i16.to_le_bytes()).unwrap();
            file.write_all(&(-30000i16).to_le_bytes()).unwrap();
        }
        let peaks = compute_peaks(file.path()).unwrap();
        assert_eq!(peaks.len(), 1);
        assert!(peaks[0] > 220, "Stereo energy was lost: {peaks:?}");
    }

    #[tokio::test]
    async fn compact_list_keeps_duration_and_excludes_trash() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::raw_sql("CREATE TABLE meetings(id TEXT, title TEXT, created_at TEXT); \
            CREATE TABLE transcripts(meeting_id TEXT, audio_end_time REAL); \
            CREATE TABLE speaker_turns(meeting_id TEXT, end_time REAL); \
            CREATE TABLE meeting_workspace_state(meeting_id TEXT, trashed_at TEXT); \
            INSERT INTO meetings VALUES('one','First','2026-10-12'),('two','Trash','2026-10-13'),('three','Empty','2026-10-11'); \
            INSERT INTO transcripts VALUES('one',42),('one',30); INSERT INTO speaker_turns VALUES('one',50); \
            INSERT INTO meeting_workspace_state VALUES('two','2026-10-14');")
            .execute(&pool).await.unwrap();
        let rows = load_compact_overviews(&pool).await.unwrap();
        assert_eq!(
            rows.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            ["one", "three"]
        );
        assert_eq!(rows[0].duration, 50.0);
        assert_eq!(rows[1].duration, 0.0);
        assert!(rows
            .iter()
            .all(|r| r.pattern.is_empty() && r.speakers.is_empty()));
    }

    fn turn(speaker: &str, start: f64, end: f64) -> TimelineTurn {
        TimelineTurn {
            speaker_id: speaker.into(),
            start,
            end,
        }
    }

    #[test]
    fn speakers_follow_first_appearance_and_use_saved_names() {
        let turns = vec![
            turn("B", 5.0, 6.0),
            turn("A", 1.0, 2.0),
            turn("mic", 7.0, 8.0),
        ];
        let names = HashMap::from([("A".to_string(), "Lena".to_string())]);
        let speakers = speakers_for(&turns, &names);
        let labels: Vec<_> = speakers.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(labels, ["Lena", "B", "You"]);
    }

    #[test]
    fn pattern_counts_overlap_once_and_picks_dominant_speaker() {
        let speakers = vec![
            TimelineSpeaker {
                id: "A".into(),
                name: "A".into(),
            },
            TimelineSpeaker {
                id: "B".into(),
                name: "B".into(),
            },
        ];
        let turns = vec![
            turn("A", 0.0, 8.0),
            turn("B", 4.0, 10.0),
            turn("B", 12.0, 13.0),
        ];
        let pattern = speaking_pattern(&turns, &speakers, 20.0, 2);
        assert_eq!(
            pattern[0],
            PatternBin {
                speaker: 0,
                level: 1.0
            }
        );
        assert_eq!(pattern[1].speaker, 1);
        assert!((pattern[1].level - 0.1).abs() < 1e-6);
    }

    #[test]
    fn transcript_lines_stand_in_when_no_diarization_exists() {
        let segments = vec![SegmentRow {
            meeting_id: "m".into(),
            speaker_id: None,
            speaker: Some("system".into()),
            audio_start_time: Some(3.0),
            audio_end_time: Some(4.5),
        }];
        let turns = turns_for(&[], &segments);
        assert_eq!(turns, vec![turn("system", 3.0, 4.5)]);
        assert_eq!(speakers_for(&turns, &HashMap::new())[0].name, "Others");
    }

    #[tokio::test]
    async fn marks_are_stored_in_time_order_and_validated() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at) VALUES ('m', 'T', '2026-10-14T10:00:00Z', '2026-10-14T10:00:00Z')")
            .execute(&pool)
            .await
            .unwrap();
        let mark = |at: f64, label: &str| MeetingMark {
            id: 0,
            at,
            label: label.into(),
        };
        let saved = add_marks(&pool, "m", &[mark(90.0, " later "), mark(12.5, "")])
            .await
            .unwrap();
        let times: Vec<_> = saved.iter().map(|m| (m.at, m.label.as_str())).collect();
        assert_eq!(times, [(12.5, ""), (90.0, "later")]);
        assert!(add_marks(&pool, "m", &[mark(-1.0, "")]).await.is_err());
        assert_eq!(list_marks(&pool, "m").await.unwrap().len(), 2);
    }
}
