//! Scheduled public-key encryption. The app stores only the public recipient;
//! the recovery key is saved separately at a location chosen by the user.
use super::integrations::write_private_json;
use crate::state::AppState;
use age::secrecy::ExposeSecret;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;
#[derive(Default, Clone, Serialize, Deserialize)]
pub struct BackupSettings {
    pub enabled: bool,
    pub directory: String,
    pub recipient: String,
    pub interval_hours: u32,
    pub include_audio: bool,
    #[serde(default)]
    pub last_success: Option<String>,
    #[serde(default)]
    pub last_error: Option<String>,
}
fn config(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("backup-schedule.json"))
}
fn load(app: &AppHandle) -> Result<BackupSettings, String> {
    match crate::security::storage::read_limited(&config(app)?, 64 * 1024) {
        Ok(b) => serde_json::from_slice(&b).map_err(|e| e.to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(BackupSettings {
            interval_hours: 24,
            ..Default::default()
        }),
        Err(e) => Err(e.to_string()),
    }
}
#[tauri::command]
pub async fn workspace_backup_settings(
    app: AppHandle,
    settings: Option<BackupSettings>,
) -> Result<BackupSettings, String> {
    if let Some(mut settings) = settings {
        if settings.enabled {
            settings
                .recipient
                .parse::<age::x25519::Recipient>()
                .map_err(|_| "Choose a valid recovery-key recipient")?;
            if !(1..=168).contains(&settings.interval_hours) {
                return Err("Choose an interval between 1 and 168 hours".into());
            }
            settings.directory = Path::new(&settings.directory)
                .canonicalize()
                .map_err(|_| "Choose an existing backup folder")?
                .to_string_lossy()
                .into_owned();
            if !Path::new(&settings.directory).is_dir() {
                return Err("Choose a backup folder".into());
            }
        }
        settings.last_success = load(&app)?.last_success;
        settings.last_error = None;
        write_private_json(&config(&app)?, &settings)?;
    }
    load(&app)
}
#[tauri::command]
pub async fn workspace_backup_key(app: AppHandle) -> Result<Value, String> {
    tokio::task::spawn_blocking(move || {
        let Some(file) = app
            .dialog()
            .file()
            .set_file_name("overtalk-recovery-key.txt")
            .blocking_save_file()
        else {
            return Ok(Value::Null);
        };
        let path = file.into_path().map_err(|e| e.to_string())?;
        let identity = age::x25519::Identity::generate();
        let public = identity.to_public().to_string();
        use std::io::Write;
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&path).map_err(|e| e.to_string())?;
        file.write_all(identity.to_string().expose_secret().as_bytes())
            .map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        Ok(json!({"recipient":public,"path":path}))
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn workspace_schedule_restore(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
    passphrase: String,
) -> Result<Value, String> {
    if crate::audio::recording_commands::is_recording().await
        || super::speech::is_processing().await
    {
        return Err("Finish recording and processing before restoring".into());
    }
    let active: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workspace_jobs WHERE state IN ('queued','running')",
    )
    .fetch_one(state.db_manager.pool())
    .await
    .map_err(|e| e.to_string())?;
    if active > 0 {
        return Err("Finish or cancel queued work before restoring".into());
    }
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let report = super::archive::workspace_restore_backup(
        path,
        dir.to_string_lossy().into_owned(),
        passphrase,
    )
    .await?;
    write_private_json(
        &dir.join("pending-restore.json"),
        &json!({"database":report.database}),
    )?;
    Ok(
        json!({"restart_required":true,"message":"Backup verified. Close and reopen Overtalk to restore. Your current database will be retained in recovery backups."}),
    )
}
/// Executed before opening SQLite. Copy the authenticated staged file, keep the
/// original database and sidecars together, and roll back a failed replacement.
pub async fn activate_pending(directory: &Path) -> Result<(), String> {
    let pending = directory.join("pending-restore.json");
    if !pending.exists() {
        return Ok(());
    }
    let value: Value = serde_json::from_slice(
        &crate::security::storage::read_limited(&pending, 64 * 1024).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let source = PathBuf::from(
        value["database"]
            .as_str()
            .ok_or("Invalid pending restore")?,
    )
    .canonicalize()
    .map_err(|e| e.to_string())?;
    let root = directory.canonicalize().map_err(|e| e.to_string())?;
    if !source.starts_with(&root)
        || source.file_name().and_then(|s| s.to_str()) != Some("meeting_minutes.sqlite")
        || !source
            .parent()
            .and_then(|p| p.file_name())
            .is_some_and(|s| s.to_string_lossy().starts_with("Sonefold-restored-"))
    {
        return Err("Invalid staged restore location".into());
    }
    // Preserve deletion decisions made after the backup was created (including
    // those made after restore was scheduled). Never resurrect erased records.
    let current_path = root.join("meeting_minutes.sqlite");
    if current_path.exists() {
        let current = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(
                sqlx::sqlite::SqliteConnectOptions::new()
                    .filename(&current_path)
                    .read_only(true),
            )
            .await
            .map_err(|_| "Could not read current erasure history")?;
        let has_ledger: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='privacy_erasures'",
        )
        .fetch_one(&current)
        .await
        .map_err(|_| "Could not check erasure history")?;
        if has_ledger > 0 {
            let staged = sqlx::sqlite::SqlitePoolOptions::new()
                .max_connections(1)
                .connect(source.to_string_lossy().as_ref())
                .await
                .map_err(|_| "Could not prepare restored privacy state")?;
            crate::security::recovery::reconcile(
                &current,
                &staged,
                &[source
                    .parent()
                    .ok_or("Invalid restore root")?
                    .join("recordings")],
            )
            .await?;
            staged.close().await;
        }
        current.close().await;
    }
    let mut replacement = tempfile::NamedTempFile::new_in(&root).map_err(|e| e.to_string())?;
    let mut input = std::fs::File::open(&source).map_err(|e| e.to_string())?;
    std::io::copy(&mut input, replacement.as_file_mut()).map_err(|e| e.to_string())?;
    replacement
        .as_file()
        .sync_all()
        .map_err(|e| e.to_string())?;
    let backup = root.join(format!(
        "before-restore-{}",
        chrono::Utc::now().format("%Y%m%dT%H%M%S%f")
    ));
    std::fs::create_dir(&backup).map_err(|e| e.to_string())?;
    let mut moved = Vec::new();
    for name in [
        "meeting_minutes.sqlite-wal",
        "meeting_minutes.sqlite-shm",
        "meeting_minutes.sqlite",
    ] {
        let path = root.join(name);
        if path.exists() {
            if let Err(e) = std::fs::rename(&path, backup.join(name)) {
                for previous in moved.iter().rev() {
                    let _ = std::fs::rename(backup.join(previous), root.join(previous));
                }
                return Err(e.to_string());
            }
            moved.push(name);
        }
    }
    if let Err(e) = replacement.persist(root.join("meeting_minutes.sqlite")) {
        let _ = std::fs::remove_file(root.join("meeting_minutes.sqlite"));
        for previous in moved.iter().rev() {
            let _ = std::fs::rename(backup.join(previous), root.join(previous));
        }
        return Err(e.to_string());
    }
    std::fs::remove_file(pending).map_err(|e| e.to_string())?;
    Ok(())
}
pub fn start(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            let Ok(settings) = load(&app) else {
                continue;
            };
            if !settings.enabled {
                continue;
            }
            let due = settings
                .last_success
                .as_deref()
                .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                .is_none_or(|t| {
                    chrono::Utc::now().signed_duration_since(t).num_hours()
                        >= i64::from(settings.interval_hours)
                });
            if !due {
                continue;
            }
            let path = Path::new(&settings.directory).join(format!(
                "Overtalk-{}.age",
                chrono::Utc::now().format("%Y%m%dT%H%M%SZ")
            ));
            let result = super::archive::workspace_create_backup(
                app.clone(),
                path.to_string_lossy().into_owned(),
                settings.recipient.clone(),
                settings.include_audio,
                app.state(),
            )
            .await;
            // A user may have changed the schedule while the backup ran.
            // Never overwrite their new destination or enabled state.
            if let Ok(mut latest) = load(&app) {
                if result.is_ok() {
                    latest.last_success = Some(chrono::Utc::now().to_rfc3339());
                }
                latest.last_error = result.as_ref().err().cloned();
                if let Ok(path) = config(&app) {
                    let _ = write_private_json(&path, &latest);
                }
            }
            let _ = app.emit(
                "workspace-backup-finished",
                json!({"ok":result.is_ok(),"error":result.err()}),
            );
        }
    });
}
