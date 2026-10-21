//! Delete app-owned media and all meeting-derived rows without following paths
//! outside the user's configured storage roots. A durable queue survives crashes.
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use std::path::{Path, PathBuf};

#[derive(Clone, Serialize, Deserialize)]
struct PendingPath {
    path: PathBuf,
    directory: bool,
}

fn safe_path(path: &Path, roots: &[PathBuf]) -> Result<PathBuf, String> {
    let meta = std::fs::symlink_metadata(path).map_err(|_| "Could not inspect a recording path")?;
    if meta.file_type().is_symlink() {
        return Err("Refusing to delete a linked recording path".into());
    }
    let canonical = path
        .canonicalize()
        .map_err(|_| "Could not resolve a recording path")?;
    if !roots
        .iter()
        .filter_map(|p| p.canonicalize().ok())
        .any(|root| canonical != root && canonical.starts_with(root))
    {
        return Err("Recording is outside managed storage. Move it into your recordings folder before deleting it.".into());
    }
    Ok(canonical)
}

pub async fn roots<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Result<Vec<PathBuf>, String> {
    use tauri::Manager;
    let preferences = crate::audio::recording_preferences::load_recording_preferences(app)
        .await
        .map_err(|_| "Could not load recording storage settings")?;
    let data = app
        .path()
        .app_data_dir()
        .map_err(|_| "App data is unavailable")?;
    let mut roots = vec![
        preferences.save_folder,
        crate::audio::recording_preferences::get_default_recordings_folder(),
        app.path()
            .app_data_dir()
            .map_err(|_| "App data is unavailable")?
            .join("pending-video"),
        app.path()
            .app_data_dir()
            .map_err(|_| "App data is unavailable")?
            .join("recordings"),
    ];
    if let Ok(entries) = std::fs::read_dir(&data) {
        for entry in entries.flatten() {
            if entry
                .file_name()
                .to_str()
                .is_some_and(|s| s.starts_with("Sonefold-restored-"))
                && entry.file_type().is_ok_and(|t| t.is_dir())
            {
                roots.push(entry.path().join("recordings"));
            }
        }
    }
    Ok(roots)
}

pub async fn delete(pool: &SqlitePool, id: &str, roots: &[PathBuf]) -> Result<bool, String> {
    if id.trim().is_empty() {
        return Err("Choose a meeting to delete".into());
    }
    let active:i64=sqlx::query_scalar("SELECT (SELECT COUNT(*) FROM workspace_jobs WHERE meeting_id=? AND state IN ('queued','running')) + (SELECT COUNT(*) FROM summary_processes WHERE meeting_id=? AND status IN ('pending','processing','running'))")
        .bind(id).bind(id).fetch_one(pool).await.map_err(|e|e.to_string())?;
    if active > 0 {
        return Err("Finish or cancel this meeting's background jobs before deleting it.".into());
    }
    let held: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM privacy_holds WHERE meeting_id=?")
        .bind(id)
        .fetch_one(pool)
        .await
        .map_err(|e| e.to_string())?;
    if held > 0 {
        return Err(
            "This meeting has a retention hold. Review and release the hold before deleting it."
                .into(),
        );
    }
    let row = sqlx::query("SELECT folder_path FROM meetings WHERE id=?")
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())?;
    let Some(row) = row else {
        return cleanup(pool, id, roots).await.map(|_| false);
    };
    let mut paths = Vec::new();
    if let Some(folder) = row
        .get::<Option<String>, _>("folder_path")
        .filter(|v| !v.is_empty())
    {
        let path = PathBuf::from(&folder);
        if path.exists() {
            let path = safe_path(&path, roots)?;
            let other_folders: Vec<String> = sqlx::query_scalar(
                "SELECT folder_path FROM meetings WHERE id!=? AND folder_path IS NOT NULL",
            )
            .bind(id)
            .fetch_all(pool)
            .await
            .map_err(|e| e.to_string())?;
            // Resolve aliases too: /recordings/a/../shared must not bypass the
            // shared-folder guard and destroy another meeting's recording.
            if other_folders
                .iter()
                .filter_map(|p| Path::new(p).canonicalize().ok())
                .any(|other| other == path || other.starts_with(&path) || path.starts_with(&other))
            {
                return Err("This recording folder is shared by multiple meetings; separate them before deletion.".into());
            }
            let owned = owned_recording(&path, id);
            if !owned {
                return Err("This folder is not marked as an Overtalk recording. Its files were not deleted.".into());
            }
            crate::native_service::player::forget(path.clone()).await?;
            paths.push(PendingPath {
                path,
                directory: true,
            });
        }
    }
    #[cfg(target_os = "linux")]
    crate::extensions::media_stream::revoke_meeting(id);
    let videos: Vec<String> =
        sqlx::query_scalar("SELECT path FROM meeting_videos WHERE meeting_id=?")
            .bind(id)
            .fetch_all(pool)
            .await
            .map_err(|e| e.to_string())?;
    for video in videos {
        let video = PathBuf::from(video);
        if video.exists() {
            let path = safe_path(&video, roots)?;
            if !paths.iter().any(|p| path.starts_with(&p.path)) {
                paths.push(PendingPath {
                    path,
                    directory: false,
                });
                let metadata = video.with_extension("json");
                if metadata.exists() {
                    paths.push(PendingPath {
                        path: safe_path(&metadata, roots)?,
                        directory: false,
                    });
                }
            }
        }
    }
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
    sqlx::query("INSERT INTO privacy_erasures(meeting_id,occurred_at,pending_paths) VALUES(?,?,?) ON CONFLICT(meeting_id) DO UPDATE SET pending_paths=excluded.pending_paths")
        .bind(id).bind(chrono::Utc::now().to_rfc3339()).bind(serde_json::to_string(&paths).unwrap())
        .execute(&mut *tx).await.map_err(|e|e.to_string())?;
    // Remove a saved answer if either its scope or sources references this meeting.
    sqlx::query("DELETE FROM workspace_answers WHERE EXISTS(SELECT 1 FROM json_tree(CASE WHEN json_valid(scope_json) THEN scope_json ELSE '{}' END) WHERE atom=?) OR EXISTS(SELECT 1 FROM json_tree(CASE WHEN json_valid(answer_json) THEN answer_json ELSE '{}' END) WHERE atom=?)")
        .bind(id).bind(id).execute(&mut *tx).await.map_err(|e|e.to_string())?;
    sqlx::query("DELETE FROM project_facts WHERE source_meeting_id=?")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    for table in ["transcript_chunks", "summary_processes", "transcripts"] {
        sqlx::query(&format!("DELETE FROM {table} WHERE meeting_id=?"))
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
    }
    sqlx::query("DELETE FROM meetings WHERE id=?")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    // Compact FTS deletions as part of erasure. Besides removing obsolete
    // index entries, this avoids SQLite <=3.46.0's documented false-positive
    // integrity failure for empty secure-delete segments after restoration.
    sqlx::query("INSERT INTO workspace_fts(workspace_fts) VALUES('optimize')")
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    tx.commit().await.map_err(|e| e.to_string())?;
    cleanup(pool, id, roots).await?;
    Ok(true)
}

fn owned_recording(path: &Path, id: &str) -> bool {
    // Most recordings carry the tiny explicit marker. Avoid opening redundant
    // metadata, and never block on a FIFO or follow an ownership-marker link.
    if super::storage::read_limited(&path.join(".overtalk-owned"), 64).is_ok() {
        return true;
    }
    super::storage::read_limited(&path.join("metadata.json"), 65535)
        .ok()
        .and_then(|v| serde_json::from_slice::<serde_json::Value>(&v).ok())
        .is_some_and(|m| {
            m["transcript_file"] == "transcripts.json"
                && m["version"] == "1.0"
                && (m["meeting_id"].is_null() || m["meeting_id"] == id)
        })
}

async fn cleanup(pool: &SqlitePool, id: &str, roots: &[PathBuf]) -> Result<(), String> {
    let encoded: Option<String> =
        sqlx::query_scalar("SELECT pending_paths FROM privacy_erasures WHERE meeting_id=?")
            .bind(id)
            .fetch_optional(pool)
            .await
            .map_err(|e| e.to_string())?;
    let paths: Vec<PendingPath> = serde_json::from_str(encoded.as_deref().unwrap_or("[]"))
        .map_err(|_| "Invalid deletion queue")?;
    let remaining = tokio::task::spawn_blocking({
        let roots = roots.to_vec();
        move || {
            paths
                .into_iter()
                .filter(|p| {
                    if !p.path.exists() {
                        return false;
                    }
                    let Ok(path) = safe_path(&p.path, &roots) else {
                        return true;
                    };
                    if p.directory {
                        std::fs::remove_dir_all(path).is_err()
                    } else {
                        std::fs::remove_file(path).is_err()
                    }
                })
                .collect::<Vec<_>>()
        }
    })
    .await
    .map_err(|_| "Deletion worker stopped")?;
    sqlx::query("UPDATE privacy_erasures SET pending_paths=? WHERE meeting_id=?")
        .bind(serde_json::to_string(&remaining).unwrap())
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    if !remaining.is_empty() {
        return Err("Meeting removed; some media could not be deleted. Privacy settings shows pending deletion for retry.".into());
    }
    Ok(())
}

pub async fn resume(pool: &SqlitePool, roots: &[PathBuf]) -> Result<(), String> {
    let ids: Vec<String> =
        sqlx::query_scalar("SELECT meeting_id FROM privacy_erasures WHERE pending_paths!='[]'")
            .fetch_all(pool)
            .await
            .map_err(|e| e.to_string())?;
    for id in ids {
        cleanup(pool, &id, roots).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn permanent_erasure_removes_managed_media_and_derived_content() {
        tokio::time::timeout(
            std::time::Duration::from_secs(15),
            check_permanent_erasure(),
        )
        .await
        .expect("Synthetic erasure workflow exceeded its deadline");
    }

    async fn check_permanent_erasure() {
        // This fixture exercises sequential erasure, not connection contention.
        // Keep one in-memory SQLite connection and await shutdown so parallel
        // tests do not leave its connection workers tied to a dropped runtime.
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("meeting");
        std::fs::create_dir(&folder).unwrap();
        std::fs::write(folder.join(".overtalk-owned"), b"1").unwrap();
        std::fs::write(folder.join("audio.mp4"), b"synthetic").unwrap();
        sqlx::query("INSERT INTO meetings(id,title,created_at,updated_at,folder_path) VALUES('m','synthetic',datetime('now'),datetime('now'),?)")
            .bind(folder.to_string_lossy().as_ref()).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO workspace_answers VALUES('a','question','{\"meeting_id\":\"m\"}','{}',datetime('now'))").execute(&pool).await.unwrap();
        assert!(delete(&pool, "m", &[root.path().to_owned()]).await.unwrap());
        assert!(!folder.exists());
        assert_eq!(
            sqlx::query_scalar::<_, String>("PRAGMA integrity_check")
                .fetch_all(&pool)
                .await
                .unwrap(),
            vec!["ok"]
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM workspace_answers")
                .fetch_one(&pool)
                .await
                .unwrap(),
            0
        );
        assert!(!delete(&pool, "m", &[root.path().to_owned()]).await.unwrap());
        pool.close().await;
    }
    #[test]
    fn deletion_refuses_root_and_outside_paths() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        assert!(safe_path(root.path(), &[root.path().to_owned()]).is_err());
        assert!(safe_path(outside.path(), &[root.path().to_owned()]).is_err());
    }
    #[cfg(unix)]
    #[test]
    fn deletion_never_follows_a_recording_symlink() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), root.path().join("link")).unwrap();
        assert!(safe_path(&root.path().join("link"), &[root.path().to_owned()]).is_err());
    }
    #[cfg(unix)]
    #[test]
    fn ownership_proof_rejects_links_special_files_and_oversized_metadata() {
        let root = tempfile::tempdir().unwrap();
        let marker = root.path().join(".overtalk-owned");
        std::os::unix::fs::symlink("/dev/null", &marker).unwrap();
        assert!(!owned_recording(root.path(), "meeting"));
        std::fs::remove_file(&marker).unwrap();
        let metadata = root.path().join("metadata.json");
        let name = std::ffi::CString::new(metadata.as_os_str().as_encoded_bytes()).unwrap();
        // SAFETY: `name` is a NUL-terminated C string that outlives the call; mkfifo only reads it.
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        assert!(!owned_recording(root.path(), "meeting"));
        std::fs::write(&marker, b"1").unwrap();
        assert!(owned_recording(root.path(), "meeting"));
        std::fs::remove_file(&marker).unwrap();
        std::fs::remove_file(&metadata).unwrap();
        std::fs::write(&metadata, vec![b' '; 65536]).unwrap();
        assert!(!owned_recording(root.path(), "meeting"));
        std::fs::write(
            &metadata,
            br#"{"transcript_file":"transcripts.json","version":"1.0","meeting_id":"meeting"}"#,
        )
        .unwrap();
        assert!(owned_recording(root.path(), "meeting"));
        assert!(!owned_recording(root.path(), "other"));
    }
    #[tokio::test]
    async fn held_meeting_cannot_be_erased() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        sqlx::query("INSERT INTO meetings(id,title,created_at,updated_at) VALUES('held','synthetic',datetime('now'),datetime('now'))").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO privacy_holds VALUES('held','synthetic hold',datetime('now'))")
            .execute(&pool)
            .await
            .unwrap();
        assert!(delete(&pool, "held", &[])
            .await
            .unwrap_err()
            .contains("retention hold"));
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM meetings")
                .fetch_one(&pool)
                .await
                .unwrap(),
            1
        );
    }
    #[tokio::test]
    async fn canonical_alias_cannot_delete_another_meetings_media() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("shared");
        std::fs::create_dir(&folder).unwrap();
        std::fs::write(folder.join(".overtalk-owned"), b"1").unwrap();
        let alias = folder.join("..").join("shared");
        for (id, path) in [("one", folder.clone()), ("two", alias)] {
            sqlx::query("INSERT INTO meetings(id,title,created_at,updated_at,folder_path) VALUES(?,'synthetic',datetime('now'),datetime('now'),?)")
                .bind(id).bind(path.to_string_lossy().as_ref()).execute(&pool).await.unwrap();
        }
        assert!(delete(&pool, "one", &[root.path().to_owned()])
            .await
            .unwrap_err()
            .contains("shared"));
        assert!(folder.exists());
    }
}
