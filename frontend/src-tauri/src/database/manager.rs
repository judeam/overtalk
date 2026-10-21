use sqlx::{migrate::MigrateDatabase, Result, Sqlite, SqlitePool};
use std::fs;
use std::path::Path;
use tauri::Manager;

#[derive(Clone)]
pub struct DatabaseManager {
    pool: SqlitePool,
}

impl DatabaseManager {
    pub async fn new(tauri_db_path: &str, backend_db_path: &str) -> Result<Self> {
        if let Some(parent_dir) = Path::new(tauri_db_path).parent() {
            if !parent_dir.exists() {
                crate::security::storage::private_directory(parent_dir).map_err(sqlx::Error::Io)?;
            }
        }

        if !Path::new(tauri_db_path).exists() {
            if Path::new(backend_db_path).exists() {
                log::info!(
                    "Copying database from {} to {}",
                    backend_db_path,
                    tauri_db_path
                );
                fs::copy(backend_db_path, tauri_db_path).map_err(sqlx::Error::Io)?;
            } else {
                log::info!("Creating database at {}", tauri_db_path);
                Sqlite::create_database(tauri_db_path).await?;
            }
        }

        if let Some(parent) = Path::new(tauri_db_path).parent() {
            crate::security::storage::private_directory(parent).map_err(sqlx::Error::Io)?;
        }
        // Do this BEFORE SQLite opens any handles. Closing an unrelated raw
        // descriptor to a live database or sidecar drops this process's POSIX
        // locks, allowing another connection to delete an in-use WAL.
        // SQLite creates new sidecars with the database's private permissions.
        for suffix in ["", "-wal", "-shm"] {
            crate::security::storage::private_file(Path::new(&format!("{tauri_db_path}{suffix}")))
                .map_err(sqlx::Error::Io)?;
        }
        let options = tauri_db_path
            .parse::<sqlx::sqlite::SqliteConnectOptions>()?
            .pragma("secure_delete", "ON");
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .connect_with(options)
            .await?;

        sqlx::migrate!("./migrations").run(&pool).await?;

        match crate::security::credentials::migrate(&pool).await {
            Ok(count) if count>0 => {
                let _=sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)").execute(&pool).await;
                let _=sqlx::query("VACUUM").execute(&pool).await;
            },
            Err(_)=>log::warn!("Provider credential migration is pending; unlock the OS keychain. Original credentials remain recoverable."),
            _=>{},
        }
        crate::security::privacy::initialize(&pool)
            .await
            .map_err(sqlx::Error::Protocol)?;
        Ok(DatabaseManager { pool })
    }

    // NOTE: So for the first time users they needs to start the application
    // after they can just delete the existing .sqlite file and then copy the existing .db file to
    // the current app dir, So the system detects legacy db and copy it and starts with that data
    // (Newly created .sqlite with the copied content from .db)
    pub async fn new_from_app_handle(app_handle: &tauri::AppHandle) -> Result<Self> {
        if let Some(state) = app_handle.try_state::<crate::state::AppState>() {
            return Ok(state.db_manager.clone());
        }
        // Resolve the app's data directory
        let app_data_dir = app_handle
            .path()
            .app_data_dir()
            .expect("failed to get app data dir");
        if !app_data_dir.exists() {
            fs::create_dir_all(&app_data_dir).map_err(sqlx::Error::Io)?;
        }
        crate::extensions::backup_schedule::activate_pending(&app_data_dir)
            .await
            .map_err(sqlx::Error::Protocol)?;

        // Define database paths
        let tauri_db_path = app_data_dir
            .join("meeting_minutes.sqlite")
            .to_string_lossy()
            .to_string();
        // Legacy backend DB path (for auto-migration if exists)
        let backend_db_path = app_data_dir
            .join("meeting_minutes.db")
            .to_string_lossy()
            .to_string();

        // A WAL may hold committed transactions absent from the main file.
        // Preserve every artifact; corruption is never a reason to delete it.
        match Self::new(&tauri_db_path, &backend_db_path).await {
            Ok(manager) => Ok(manager),
            Err(error) => {
                let message = error.to_string().to_ascii_lowercase();
                if message.contains("malformed") || message.contains("corrupt") {
                    let directory = app_data_dir.clone();
                    match tokio::task::spawn_blocking(move || preserve_database_artifacts(&directory)).await {
                        Ok(Ok(backup)) => log::error!("Database could not open. Original files preserved; recovery copy: {}", backup.display()),
                        result => log::error!("Database could not open. Original files preserved; recovery copy failed: {:?}", result),
                    }
                }
                Err(error)
            }
        }
    }

    /// Check if this is the first launch (sqlite database doesn't exist yet)
    pub async fn is_first_launch(app_handle: &tauri::AppHandle) -> Result<bool> {
        let app_data_dir = app_handle
            .path()
            .app_data_dir()
            .expect("failed to get app data dir");

        let tauri_db_path = app_data_dir.join("meeting_minutes.sqlite");

        Ok(!tauri_db_path.exists())
    }

    /// Import a legacy database from the specified path and initialize
    pub async fn import_legacy_database(
        app_handle: &tauri::AppHandle,
        legacy_db_path: &str,
    ) -> Result<Self> {
        if app_handle.try_state::<crate::state::AppState>().is_some() {
            return Err(sqlx::Error::Protocol(
                "The library is already open. Use backup restore to replace it safely.".into(),
            ));
        }
        let app_data_dir = app_handle
            .path()
            .app_data_dir()
            .expect("failed to get app data dir");

        if !app_data_dir.exists() {
            fs::create_dir_all(&app_data_dir).map_err(sqlx::Error::Io)?;
        }

        // Copy legacy database to app data directory as meeting_minutes.db
        let target_legacy_path = app_data_dir.join("meeting_minutes.db");
        log::info!(
            "Copying legacy database from {} to {}",
            legacy_db_path,
            target_legacy_path.display()
        );

        fs::copy(legacy_db_path, &target_legacy_path).map_err(sqlx::Error::Io)?;

        // Now use the standard initialization which will detect and migrate the legacy db
        Self::new_from_app_handle(app_handle).await
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// Cleanup database connection and checkpoint WAL
    /// This should be called on application shutdown to ensure:
    /// - All WAL changes are written to the main database file
    /// - The .wal and .shm files are deleted
    /// - Connection pool is gracefully closed
    pub async fn cleanup(&self) -> Result<()> {
        log::info!("Starting database cleanup...");

        // Force checkpoint of WAL to main database file and remove WAL file
        // TRUNCATE mode: checkpoints all pages AND deletes the WAL file
        match sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
            .execute(&self.pool)
            .await
        {
            Ok(_) => log::info!("WAL checkpoint completed successfully"),
            Err(e) => log::warn!("WAL checkpoint failed (non-fatal): {}", e),
        }

        // Close the connection pool gracefully
        self.pool.close().await;
        log::info!("Database connection pool closed");

        Ok(())
    }
}

/// Copy failure artifacts without attempting speculative recovery or removing WAL.
fn preserve_database_artifacts(directory: &Path) -> std::io::Result<std::path::PathBuf> {
    let backup = directory.join(format!("recovery-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&backup)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&backup, fs::Permissions::from_mode(0o700))?;
    }
    for name in [
        "meeting_minutes.sqlite",
        "meeting_minutes.sqlite-wal",
        "meeting_minutes.sqlite-shm",
    ] {
        let source = directory.join(name);
        if source.is_file() {
            fs::copy(&source, backup.join(name))?;
        }
    }
    Ok(backup)
}

#[cfg(test)]
mod recovery_preservation_tests {
    use super::*;
    #[test]
    fn corrupt_database_recovery_keeps_committed_wal_and_originals() {
        let directory = tempfile::tempdir().unwrap();
        for (name, contents) in [
            ("meeting_minutes.sqlite", b"malformed".as_slice()),
            (
                "meeting_minutes.sqlite-wal",
                b"recent committed notes".as_slice(),
            ),
            ("meeting_minutes.sqlite-shm", b"shared state".as_slice()),
        ] {
            fs::write(directory.path().join(name), contents).unwrap();
        }
        let backup = preserve_database_artifacts(directory.path()).unwrap();
        for name in [
            "meeting_minutes.sqlite",
            "meeting_minutes.sqlite-wal",
            "meeting_minutes.sqlite-shm",
        ] {
            assert_eq!(
                fs::read(directory.path().join(name)).unwrap(),
                fs::read(backup.join(name)).unwrap()
            );
        }
        assert_ne!(backup, directory.path());
    }
}
