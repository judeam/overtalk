//! Narrow native file selection and streaming access to a meeting-owned recording.
use crate::state::AppState;
#[cfg(not(target_os = "linux"))]
use tauri::Manager;
use tauri::{AppHandle, Runtime, State};
use tauri_plugin_dialog::DialogExt;
#[tauri::command]
pub async fn workspace_pick_file<R: Runtime>(
    app: AppHandle<R>,
    kind: String,
    name: Option<String>,
) -> Result<Option<String>, String> {
    tokio::task::spawn_blocking(move || {
        let picker = app.dialog().file();
        let name = name.as_deref().unwrap_or("Meeting");
        let selected = match kind.as_str() {
            "share-page" => picker
                .add_filter("Web page", &["html"])
                .set_file_name(super::share::file_name(name, "html"))
                .blocking_save_file(),
            "share-file" => picker
                .add_filter("Overtalk meeting", &["overtalk"])
                .set_file_name(super::share::file_name(name, "overtalk"))
                .blocking_save_file(),
            "share-open" => picker
                .add_filter("Overtalk meeting", &["overtalk"])
                .blocking_pick_file(),
            "calendar" => picker.add_filter("Calendar", &["ics"]).blocking_pick_file(),
            "backup-open" => picker
                .add_filter("Encrypted backup", &["age"])
                .blocking_pick_file(),
            "backup-save" => picker
                .add_filter("Encrypted backup", &["age"])
                .set_file_name("Overtalk-backup.age")
                .blocking_save_file(),
            "restore-folder" | "recording-folder" => picker.blocking_pick_folder(),
            _ => return Err("Unsupported file selection".into()),
        };
        let chosen = selected
            .map(|file| file.into_path().map_err(|e| e.to_string()))
            .transpose()?;
        if let Some(path) = &chosen {
            match kind.as_str() {
                "share-page" | "share-file" => remember_chosen(path, Purpose::Save),
                "share-open" => remember_chosen(path, Purpose::Open),
                _ => {}
            }
        }
        Ok(chosen.map(|p| p.to_string_lossy().into_owned()))
    })
    .await
    .map_err(|e| e.to_string())?
}
/// Why the person picked a file: a save dialog confirms writing there, an open dialog
/// only reading. A file picked to open can never become a place to write.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Purpose {
    Save,
    Open,
}

/// Files the person chose in a save or open dialog during the last hour. A command that
/// writes or opens where the interface says accepts only one of these, once, and only
/// for the purpose it was picked for, so an interface cannot name a file the person
/// never chose.
static CHOSEN: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<(std::path::PathBuf, Purpose), std::time::Instant>>,
> = std::sync::LazyLock::new(Default::default);
const CHOICE_LIFETIME: std::time::Duration = std::time::Duration::from_secs(3600);

fn remember_chosen(path: &std::path::Path, purpose: Purpose) {
    if let Ok(mut chosen) = CHOSEN.lock() {
        chosen.retain(|_, at| at.elapsed() < CHOICE_LIFETIME);
        chosen.insert((path.to_path_buf(), purpose), std::time::Instant::now());
    }
}

/// Use up a file chosen for `purpose`. False if it was never chosen for it, already
/// used, or chosen too long ago.
pub fn take_chosen(path: &std::path::Path, purpose: Purpose) -> bool {
    CHOSEN
        .lock()
        .ok()
        .and_then(|mut chosen| chosen.remove(&(path.to_path_buf(), purpose)))
        .is_some_and(|at| at.elapsed() < CHOICE_LIFETIME)
}

#[cfg(test)]
pub fn choose_for_test(path: &std::path::Path, purpose: Purpose) {
    remember_chosen(path, purpose);
}

#[tauri::command]
pub async fn workspace_audio_source(
    _app: AppHandle,
    meeting_id: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let recording = recording(state.db_manager.pool(), &meeting_id).await?;
    #[cfg(target_os = "linux")]
    {
        let extension = recording
            .extension()
            .and_then(|v| v.to_str())
            .unwrap_or("wav");
        let file = overtalk_worker_sandbox::open_regular(&recording, false, false)
            .map_err(|e| e.to_string())?;
        return super::media_stream::source(file, extension, &meeting_id).await;
    }
    #[cfg(not(target_os = "linux"))]
    {
        _app.asset_protocol_scope()
            .allow_file(&recording)
            .map_err(|e| e.to_string())?;
        Ok(recording.to_string_lossy().into_owned())
    }
}

/// The meeting's own recording: a regular file inside its meeting folder.
pub(crate) async fn recording(
    pool: &sqlx::SqlitePool,
    meeting_id: &str,
) -> Result<std::path::PathBuf, String> {
    let folder: Option<Option<String>> =
        sqlx::query_scalar("SELECT folder_path FROM meetings WHERE id=?")
            .bind(meeting_id)
            .fetch_optional(pool)
            .await
            .map_err(|e| e.to_string())?;
    let folder =
        std::path::PathBuf::from(folder.flatten().ok_or("This meeting has no saved audio")?)
            .canonicalize()
            .map_err(|_| "The recording folder was moved or removed")?;
    let recording = crate::audio::retranscription::find_audio_file(&folder)
        .map_err(|e| e.to_string())?
        .canonicalize()
        .map_err(|e| e.to_string())?;
    if !recording.starts_with(&folder) || !recording.is_file() {
        return Err("The recording must be a regular file inside this meeting folder".into());
    }
    Ok(recording)
}

/// Locate a cited passage using precisely the same ordering as paginated reads.
#[tauri::command]
pub async fn workspace_transcript_offset(
    meeting_id: String,
    segment_id: String,
    state: State<'_, AppState>,
) -> Result<i64, String> {
    transcript_offset(state.db_manager.pool(), &meeting_id, &segment_id).await
}

// Count preceding index entries, without numbering and materializing every
// preceding row. NULL timestamps sort first, then equal timestamps sort by id.
const TRANSCRIPT_OFFSET: &str = "SELECT CASE WHEN t.audio_start_time IS NULL THEN
 (SELECT count(*) FROM transcripts p WHERE p.meeting_id=t.meeting_id
  AND p.audio_start_time IS NULL AND p.id<t.id)
 ELSE
 (SELECT count(*) FROM transcripts p WHERE p.meeting_id=t.meeting_id AND p.audio_start_time IS NULL) +
 (SELECT count(*) FROM transcripts p WHERE p.meeting_id=t.meeting_id AND p.audio_start_time<t.audio_start_time) +
 (SELECT count(*) FROM transcripts p WHERE p.meeting_id=t.meeting_id AND p.audio_start_time=t.audio_start_time AND p.id<t.id)
 END FROM transcripts t WHERE t.meeting_id=? AND t.id=?";

async fn transcript_offset(
    pool: &sqlx::SqlitePool,
    meeting_id: &str,
    segment_id: &str,
) -> Result<i64, String> {
    let position: Option<i64> = sqlx::query_scalar(TRANSCRIPT_OFFSET)
        .bind(meeting_id)
        .bind(segment_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())?;
    position
        .map(|n| n / 100 * 100)
        .ok_or_else(|| "The cited passage no longer exists".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn citation_pages_match_transcript_order_including_nulls_ties_and_other_meetings() {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::raw_sql(
            "CREATE TABLE transcripts(id TEXT PRIMARY KEY, meeting_id TEXT, audio_start_time REAL);
          CREATE INDEX transcripts_meeting_time ON transcripts(meeting_id,audio_start_time,id);",
        )
        .execute(&pool)
        .await
        .unwrap();
        for n in (0..260).rev() {
            sqlx::query("INSERT INTO transcripts VALUES(?,?,?)")
                .bind(format!("line-{n:03}"))
                .bind(if n % 7 == 0 { "other" } else { "meeting" })
                .bind(if n % 5 == 0 {
                    None
                } else {
                    Some((n / 3) as f64)
                })
                .execute(&pool)
                .await
                .unwrap();
        }
        let ordered: Vec<String> = sqlx::query_scalar(
            "SELECT id FROM transcripts WHERE meeting_id='meeting' ORDER BY audio_start_time,id",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        for (n, id) in ordered.iter().enumerate() {
            assert_eq!(
                transcript_offset(&pool, "meeting", id).await.unwrap(),
                (n / 100 * 100) as i64
            );
        }
        assert!(transcript_offset(&pool, "meeting", "missing")
            .await
            .is_err());
        assert!(transcript_offset(&pool, "other", &ordered[0])
            .await
            .is_err());
    }
}

#[tauri::command]
pub fn workspace_release_audio(source: String) {
    #[cfg(target_os = "linux")]
    super::media_stream::release(&source);
    // Other systems play recordings without a local media stream to release.
    #[cfg(not(target_os = "linux"))]
    drop(source);
}
