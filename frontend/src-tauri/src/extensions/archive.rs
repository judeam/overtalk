//! Authenticated streaming age backups, credential-free snapshots, staged restore.
use crate::state::AppState;
use age::secrecy::ExposeSecret;
use age::secrecy::SecretString;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{Row, SqlitePool};
use std::{
    collections::BTreeSet,
    fs::{self, File},
    io::{BufRead, BufReader, Cursor, Read, Write},
    path::{Path, PathBuf},
};
use tauri::{AppHandle, Manager, State};
const MAX_MANIFEST: u64 = 2 * 1024 * 1024;
const MAX_ENTRY: u64 = 8 * 1024 * 1024 * 1024;
const MAX_TOTAL: u64 = 50 * 1024 * 1024 * 1024;
const MAX_AGE_HEADER: u64 = 16 * 1024;
const MAX_FILES: usize = 20000;
// Bound unauthenticated KDF work independently of the current CPU's speed.
// New archives use 256 MiB; accept older archives up to 1 GiB of scrypt memory.
const SCRYPT_WRITE_FACTOR: u8 = 18;
const SCRYPT_READ_FACTOR: u8 = 20;
static ARCHIVE_JOB: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
#[derive(Serialize, Deserialize, Clone)]
struct Entry {
    name: String,
    bytes: u64,
    sha256: String,
}
#[derive(Serialize, Deserialize)]
struct Manifest {
    format: String,
    version: u32,
    created_at: String,
    audio_included: bool,
    entries: Vec<Entry>,
}
#[derive(Serialize)]
pub struct RestoreReport {
    pub directory: String,
    pub database: String,
    pub audio_included: bool,
    pub credentials_restored: bool,
}
fn private_dir(path: &Path) -> Result<(), String> {
    crate::security::storage::private_directory(path).map_err(|e| e.to_string())
}
fn safe_name(name: &str) -> Result<PathBuf, String> {
    let path = Path::new(name);
    if name.is_empty()
        || name.contains('\\')
        || name.contains(':')
        || name.split('/').any(|part| {
            let stem = part
                .split('.')
                .next()
                .unwrap_or("")
                .trim_end_matches(' ')
                .to_ascii_uppercase();
            part.is_empty()
                || part == "."
                || part == ".."
                || part.ends_with(['.', ' '])
                || part.contains(['<', '>', '"', '|', '?', '*'])
                || part.chars().any(char::is_control)
                || matches!(
                    stem.as_str(),
                    "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
                )
                || ["COM", "LPT"].iter().any(|prefix| {
                    stem.strip_prefix(prefix).is_some_and(|n| {
                        matches!(
                            n,
                            "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                        )
                    })
                })
        })
        || path.is_absolute()
        || !path
            .components()
            .all(|c| matches!(c, std::path::Component::Normal(_)))
    {
        return Err("Backup contains an unsafe file path".into());
    }
    if name != "meeting_minutes.sqlite"
        && !name.starts_with("recordings/")
        && !name.starts_with("templates/")
    {
        return Err("Backup contains an unexpected file".into());
    }
    Ok(path.into())
}
fn describe(name: String, path: &Path) -> Result<Entry, String> {
    safe_name(&name)?;
    let mut reader =
        overtalk_worker_sandbox::open_regular(path, false, false).map_err(|e| e.to_string())?;
    let meta = reader.metadata().map_err(|e| e.to_string())?;
    if meta.len() > MAX_ENTRY {
        return Err("Backups require regular files below 8 GB each".into());
    }
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    let mut size = 0;
    loop {
        let n = reader.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        size += n as u64;
        if size > meta.len() {
            return Err(
                "A backup file grew while it was read; retry after processing finishes".into(),
            );
        }
        hash.update(&buffer[..n]);
    }
    if size != meta.len() {
        return Err("A backup file changed while it was read; retry".into());
    }
    Ok(Entry {
        name,
        bytes: meta.len(),
        sha256: format!("{:x}", hash.finalize()),
    })
}
fn encrypt(
    destination: &Path,
    password: SecretString,
    files: Vec<(String, PathBuf)>,
    audio: bool,
) -> Result<(), String> {
    if files.len() > MAX_FILES {
        return Err("Too many backup files".into());
    }
    let parent = destination.parent().ok_or("Choose a backup folder")?;
    if destination.exists() {
        return Err("Choose a new backup filename; existing files are not overwritten".into());
    }
    let mut entries = Vec::new();
    let mut total = 0u64;
    for (name, path) in &files {
        let entry = describe(name.clone(), path)?;
        total = total
            .checked_add(entry.bytes)
            .ok_or("Backup is too large")?;
        if total > MAX_TOTAL {
            return Err("This backup exceeds 50 GB".into());
        }
        entries.push(entry);
    }
    let manifest = Manifest {
        format: "sonefold-stream-archive".into(),
        version: 1,
        created_at: chrono::Utc::now().to_rfc3339(),
        audio_included: audio,
        entries,
    };
    let header = serde_json::to_vec(&manifest).map_err(|e| e.to_string())?;
    if header.len() as u64 > MAX_MANIFEST {
        return Err("Too many backup files".into());
    }
    let mut staged = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        staged
            .as_file()
            .set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(|e| e.to_string())?;
    }
    {
        let encryptor =
            if let Ok(recipient) = password.expose_secret().parse::<age::x25519::Recipient>() {
                age::Encryptor::with_recipients(std::iter::once(&recipient as &dyn age::Recipient))
                    .map_err(|e| e.to_string())?
            } else {
                let mut recipient = age::scrypt::Recipient::new(password);
                recipient.set_work_factor(SCRYPT_WRITE_FACTOR);
                age::Encryptor::with_recipients(std::iter::once(&recipient as &dyn age::Recipient))
                    .map_err(|e| e.to_string())?
            };
        let mut output = encryptor
            .wrap_output(staged.as_file_mut())
            .map_err(|e| e.to_string())?;
        output.write_all(&header).map_err(|e| e.to_string())?;
        output.write_all(b"\n").map_err(|e| e.to_string())?;
        for ((_, path), entry) in files.iter().zip(&manifest.entries) {
            let mut input = overtalk_worker_sandbox::open_regular(path, false, false)
                .map_err(|e| e.to_string())?;
            let mut buffer = [0u8; 64 * 1024];
            let mut hash = Sha256::new();
            let mut size = 0u64;
            loop {
                let n = input.read(&mut buffer).map_err(|e| e.to_string())?;
                if n == 0 {
                    break;
                }
                size += n as u64;
                if size > entry.bytes {
                    return Err(
                        "A recording changed during backup; retry after processing finishes".into(),
                    );
                }
                hash.update(&buffer[..n]);
                output.write_all(&buffer[..n]).map_err(|e| e.to_string())?;
            }
            if size != entry.bytes || format!("{:x}", hash.finalize()) != entry.sha256 {
                return Err("A backup file changed while it was copied; retry".into());
            }
        }
        output.finish().map_err(|e| e.to_string())?;
    }
    staged.as_file().sync_all().map_err(|e| e.to_string())?;
    staged
        .persist_noclobber(destination)
        .map_err(|e| e.to_string())?;
    Ok(())
}
/// Copy only the small public header before invoking age's parser. Keep the
/// verified prefix in memory so a concurrent file change cannot replace it.
fn bounded_age_input<R: Read>(source: R) -> Result<impl BufRead, String> {
    let mut input = BufReader::new(source);
    let mut header = Vec::new();
    for _ in 0..128 {
        let before = header.len();
        input
            .by_ref()
            .take(MAX_AGE_HEADER + 1 - before as u64)
            .read_until(b'\n', &mut header)
            .map_err(|e| e.to_string())?;
        if header.len() as u64 > MAX_AGE_HEADER {
            return Err("Backup encryption header exceeds the size limit".into());
        }
        let line = &header[before..];
        if line.is_empty() || !line.ends_with(b"\n") {
            return Err("Backup encryption header is incomplete".into());
        }
        if before == 0 && line != b"age-encryption.org/v1\n" {
            return Err("This is not a supported age backup".into());
        }
        if line.starts_with(b"--- ") {
            return Ok(BufReader::new(Cursor::new(header).chain(input)));
        }
    }
    Err("Backup encryption header has too many lines".into())
}
/// No restored file is published until the entire encrypted stream authenticates.
fn decrypt(
    source: &Path,
    parent: &Path,
    password: SecretString,
) -> Result<(tempfile::TempDir, Manifest), String> {
    let input =
        overtalk_worker_sandbox::open_regular(source, false, false).map_err(|e| e.to_string())?;
    let encrypted = age::Decryptor::new_buffered(bounded_age_input(input)?)
        .map_err(|_| "This is not a supported age backup")?;
    let identity: Box<dyn age::Identity> =
        if password.expose_secret().starts_with("AGE-SECRET-KEY-") {
            Box::new(
                password
                    .expose_secret()
                    .parse::<age::x25519::Identity>()
                    .map_err(|_| "Invalid backup recovery key")?,
            )
        } else {
            let mut identity = age::scrypt::Identity::new(password);
            identity.set_max_work_factor(SCRYPT_READ_FACTOR);
            Box::new(identity)
        };
    let plain = encrypted
        .decrypt(std::iter::once(identity.as_ref()))
        .map_err(|error| match error {
            age::DecryptError::ExcessiveWork { .. } => "This backup exceeds the passphrase memory limit. Re-encrypt it with an age recovery key before importing.",
            _ => "The backup passphrase is incorrect or the backup is damaged",
        })?;
    let mut input = BufReader::new(plain);
    let mut header = Vec::new();
    let size = input
        .by_ref()
        .take(MAX_MANIFEST + 1)
        .read_until(b'\n', &mut header)
        .map_err(|_| "The encrypted backup is incomplete or damaged")?;
    if size == 0 || size as u64 > MAX_MANIFEST || header.last() != Some(&b'\n') {
        return Err("Backup manifest is missing or oversized".into());
    }
    let manifest: Manifest =
        serde_json::from_slice(&header).map_err(|_| "Invalid backup manifest")?;
    if manifest.format != "sonefold-stream-archive"
        || manifest.version != 1
        || manifest.entries.len() > MAX_FILES
    {
        return Err("Unsupported backup format".into());
    }
    let stage = tempfile::Builder::new()
        .prefix(".sonefold-restore-")
        .tempdir_in(parent)
        .map_err(|e| e.to_string())?;
    private_dir(stage.path())?;
    let mut names = BTreeSet::new();
    let mut total = 0u64;
    for entry in &manifest.entries {
        let relative = safe_name(&entry.name)?;
        if !names.insert(entry.name.clone()) || entry.bytes > MAX_ENTRY || entry.sha256.len() != 64
        {
            return Err("Invalid or duplicate backup entry".into());
        }
        total = total
            .checked_add(entry.bytes)
            .ok_or("Backup is too large")?;
        if total > MAX_TOTAL {
            return Err("Backup exceeds the restore limit".into());
        }
        let path = stage.path().join(relative);
        private_dir(path.parent().unwrap())?;
        let mut output = File::options()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            output
                .set_permissions(fs::Permissions::from_mode(0o600))
                .map_err(|e| e.to_string())?;
        }
        let mut remaining = entry.bytes;
        let mut buffer = [0u8; 64 * 1024];
        let mut hash = Sha256::new();
        while remaining > 0 {
            let count = (remaining as usize).min(buffer.len());
            let n = input
                .read(&mut buffer[..count])
                .map_err(|_| "Backup authentication failed; no files were restored")?;
            if n == 0 {
                return Err("Backup ended before a complete file was restored".into());
            }
            hash.update(&buffer[..n]);
            output.write_all(&buffer[..n]).map_err(|e| e.to_string())?;
            remaining -= n as u64;
        }
        if format!("{:x}", hash.finalize()) != entry.sha256 {
            return Err("Backup checksum failed; no files were restored".into());
        }
        output.sync_all().map_err(|e| e.to_string())?;
    }
    let mut end = [0u8; 1];
    if input
        .read(&mut end)
        .map_err(|_| "Final backup authentication failed; no files were restored")?
        != 0
    {
        return Err("Unexpected data after the backup payload".into());
    }
    if !names.contains("meeting_minutes.sqlite") {
        return Err("Backup has no meeting database".into());
    }
    Ok((stage, manifest))
}
async fn scrub_snapshot(pool: &SqlitePool) -> Result<(), String> {
    for table in ["settings", "transcript_settings", "licensing"] {
        sqlx::query(&format!("DELETE FROM {table}"))
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;
    }
    // A portable backup must not point at the source machine's keychain or
    // replay file deletions against its absolute paths. Keep erasure IDs so a
    // later restore can still reconcile the records, but drop pending paths.
    let has_privacy: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='privacy_settings'",
    )
    .fetch_one(pool)
    .await
    .map_err(|e| e.to_string())?;
    if has_privacy > 0 {
        sqlx::query("UPDATE privacy_settings SET credential_namespace=? WHERE id=1")
            .bind(uuid::Uuid::new_v4().to_string())
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;
        sqlx::query("UPDATE privacy_erasures SET pending_paths='[]'")
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;
        // Audit signing keys intentionally never leave the OS credential store.
        // A portable restore starts a new audit chain; an in-place restore
        // reconciles and preserves the current device's chain separately.
        sqlx::query("DELETE FROM privacy_events")
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;
    }
    // Rebuild the file so deleted credential bytes cannot remain in free pages.
    sqlx::query("VACUUM")
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}
#[tauri::command]
pub async fn workspace_create_backup(
    app: AppHandle,
    path: String,
    passphrase: String,
    include_audio: bool,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let job = ARCHIVE_JOB
        .try_lock()
        .map_err(|_| "A backup or restore is already running")?;
    if passphrase.chars().count() < 12 {
        return Err("Use a backup passphrase of at least 12 characters".into());
    }
    if crate::audio::recording_commands::is_recording().await
        || crate::speech::is_processing().await
    {
        return Err("Finish recording and speech processing before backing up".into());
    }
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let stage = tempfile::Builder::new()
        .prefix(".backup-")
        .tempdir_in(&dir)
        .map_err(|e| e.to_string())?;
    private_dir(stage.path())?;
    let database = stage.path().join("meeting_minutes.sqlite");
    sqlx::query("VACUUM INTO ?")
        .bind(database.to_string_lossy().as_ref())
        .execute(state.db_manager.pool())
        .await
        .map_err(|e| format!("Cannot snapshot the database: {e}"))?;
    let snapshot = SqlitePool::connect(database.to_string_lossy().as_ref())
        .await
        .map_err(|e| e.to_string())?;
    scrub_snapshot(&snapshot).await?;
    let rows = sqlx::query("SELECT id,folder_path FROM meetings WHERE folder_path IS NOT NULL")
        .fetch_all(&snapshot)
        .await
        .map_err(|e| e.to_string())?;
    let mut files = vec![("meeting_minutes.sqlite".into(), database.clone())];
    for row in rows {
        let id: String = row.get("id");
        let old: Option<String> = row.get("folder_path");
        let Some(old) = old else {
            continue;
        };
        let folder = PathBuf::from(old);
        let relative = format!("recordings/{:x}", Sha256::digest(id.as_bytes()));
        if include_audio {
            if folder.is_dir() {
                for entry in fs::read_dir(&folder).map_err(|e| e.to_string())? {
                    let path = entry.map_err(|e| e.to_string())?.path();
                    if path.extension().and_then(|s| s.to_str()).is_some_and(|s| {
                        matches!(
                            s.to_ascii_lowercase().as_str(),
                            "wav" | "mp4" | "m4a" | "mp3" | "flac" | "ogg" | "webm"
                        )
                    }) {
                        let name = path
                            .file_name()
                            .ok_or("Recording filename is missing")?
                            .to_string_lossy();
                        files.push((format!("{relative}/{name}"), path));
                        if files.len() > MAX_FILES {
                            return Err("Too many backup files".into());
                        }
                    }
                }
            } else {
                return Err(format!(
                    "Recording folder for a meeting is missing: {}",
                    folder.display()
                ));
            }
        }
        sqlx::query("UPDATE meetings SET folder_path=? WHERE id=?")
            .bind(if include_audio { Some(relative) } else { None })
            .bind(id)
            .execute(&snapshot)
            .await
            .map_err(|e| e.to_string())?;
    }
    snapshot.close().await;
    // Only custom template JSON, never provider/integration/runtime configuration.
    if let Some(data) = dirs::data_dir() {
        let templates = data.join("Sonefold/templates");
        if templates.is_dir() {
            for entry in fs::read_dir(templates).map_err(|e| e.to_string())? {
                let path = entry.map_err(|e| e.to_string())?.path();
                if path.extension().and_then(|s| s.to_str()) == Some("json") {
                    files.push((
                        format!("templates/{}", path.file_name().unwrap().to_string_lossy()),
                        path,
                    ));
                    if files.len() > MAX_FILES {
                        return Err("Too many backup files".into());
                    }
                }
            }
        }
    }
    let destination = PathBuf::from(&path);
    let password = SecretString::from(passphrase);
    let outcome = tokio::task::spawn_blocking(move || {
        let _job = job;
        let _stage = stage;
        encrypt(&destination, password, files, include_audio)
    })
    .await
    .map_err(|e| e.to_string())?;
    outcome?;
    Ok(path)
}
#[tauri::command]
pub async fn workspace_restore_backup(
    path: String,
    directory: String,
    passphrase: String,
) -> Result<RestoreReport, String> {
    let job = ARCHIVE_JOB
        .try_lock()
        .map_err(|_| "A backup or restore is already running")?;
    let parent = PathBuf::from(directory)
        .canonicalize()
        .map_err(|_| "Choose an existing restore folder")?;
    let source = PathBuf::from(path);
    let password = SecretString::from(passphrase);
    let target = parent.join(format!("Sonefold-restored-{}", uuid::Uuid::new_v4()));
    let extract_parent = parent.clone();
    let (result, _job) = tokio::task::spawn_blocking(move || {
        let result = decrypt(&source, &extract_parent, password);
        (result, job)
    })
    .await
    .map_err(|e| e.to_string())?;
    let (stage, manifest) = result?;
    let database = stage.path().join("meeting_minutes.sqlite");
    // Schema-defined SQL in a foreign file may only call innocuous functions.
    let options = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(&database)
        .pragma("trusted_schema", "OFF");
    let restored = SqlitePool::connect_with(options)
        .await
        .map_err(|e| e.to_string())?;
    let integrity: String = sqlx::query_scalar("PRAGMA integrity_check")
        .fetch_one(&restored)
        .await
        .map_err(|e| e.to_string())?;
    if integrity != "ok" {
        return Err("The restored database failed its integrity check".into());
    }
    only_known_schema_code(&restored).await?;
    let rows = sqlx::query("SELECT id,folder_path FROM meetings WHERE folder_path IS NOT NULL")
        .fetch_all(&restored)
        .await
        .map_err(|e| e.to_string())?;
    for row in rows {
        let folder: String = row.get("folder_path");
        safe_name(&format!("{folder}/recording.wav"))?;
        if !folder.starts_with("recordings/") {
            return Err("Restored meeting has an unsafe recording folder".into());
        }
        let restored_folder = stage.path().join(&folder);
        if restored_folder.is_dir() {
            std::fs::write(restored_folder.join(".overtalk-owned"), b"1")
                .map_err(|_| "Could not mark restored recording ownership")?;
        }
        sqlx::query("UPDATE meetings SET folder_path=? WHERE id=?")
            .bind(target.join(folder).to_string_lossy().as_ref())
            .bind(row.get::<String, _>("id"))
            .execute(&restored)
            .await
            .map_err(|e| e.to_string())?;
    }
    scrub_snapshot(&restored).await?;
    restored.close().await;
    let staging = stage.keep();
    if let Err(error) = fs::rename(&staging, &target) {
        let _ = fs::remove_dir_all(&staging);
        return Err(error.to_string());
    }
    Ok(RestoreReport {
        database: target
            .join("meeting_minutes.sqlite")
            .to_string_lossy()
            .into_owned(),
        directory: target.to_string_lossy().into_owned(),
        audio_included: manifest.audio_included,
        credentials_restored: false,
    })
}
/// Triggers and views run whenever a library is written, so a backup may only contain
/// the exact ones Overtalk's migrations create. Anything else is refused, not removed.
async fn only_known_schema_code(restored: &SqlitePool) -> Result<(), String> {
    const CODE: &str = "SELECT type,name,sql FROM sqlite_master WHERE type IN ('trigger','view')";
    let reference = SqlitePool::connect("sqlite::memory:")
        .await
        .map_err(|e| e.to_string())?;
    sqlx::migrate!("./migrations")
        .run(&reference)
        .await
        .map_err(|e| e.to_string())?;
    let ours: std::collections::HashSet<(String, String, Option<String>)> = sqlx::query_as(CODE)
        .fetch_all(&reference)
        .await
        .map_err(|e| e.to_string())?
        .into_iter()
        .collect();
    reference.close().await;
    let theirs: Vec<(String, String, Option<String>)> = sqlx::query_as(CODE)
        .fetch_all(restored)
        .await
        .map_err(|e| e.to_string())?;
    if theirs.iter().all(|object| ours.contains(object)) {
        Ok(())
    } else {
        Err("This backup contains database triggers or views that Overtalk does not create, so it was not restored".into())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn encryption_header_budget_rejects_long_lines_and_recipient_floods() {
        let mut long = b"age-encryption.org/v1\n-> ".to_vec();
        long.extend(std::iter::repeat_n(b'x', 32 * 1024));
        assert!(bounded_age_input(&long[..])
            .err()
            .unwrap()
            .contains("size limit"));
        let lines = format!("age-encryption.org/v1\n{}", "-> X25519 bogus\n".repeat(200));
        assert!(bounded_age_input(lines.as_bytes())
            .err()
            .unwrap()
            .contains("too many lines"));
    }

    #[test]
    fn excessive_scrypt_cost_is_rejected_before_key_derivation() {
        // A valid-shaped unauthenticated stanza requests far more memory than
        // allowed. No expensive encryption is needed to construct this attack.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("excessive.age");
        let header = format!(
            "age-encryption.org/v1\n-> scrypt {} 63\n{}\n--- {}\n",
            "A".repeat(22),
            "A".repeat(43),
            "A".repeat(43)
        );
        let mut bytes = header.into_bytes();
        bytes.extend([0; 32]);
        fs::write(&path, bytes).unwrap();
        assert!(decrypt(
            &path,
            dir.path(),
            SecretString::from("synthetic".to_string())
        )
        .err()
        .unwrap()
        .contains("memory limit"));
    }

    #[cfg(unix)]
    #[test]
    fn backup_input_rejects_links_and_special_files() {
        let dir = tempfile::tempdir().unwrap();
        let fifo = dir.path().join("fifo");
        let c = std::ffi::CString::new(fifo.as_os_str().as_encoded_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&fifo, &link).unwrap();
        for path in [&fifo, &link] {
            assert!(describe("meeting_minutes.sqlite".into(), path).is_err());
            assert!(decrypt(
                path,
                dir.path(),
                SecretString::from("synthetic".to_string())
            )
            .is_err());
        }
    }

    #[test]
    fn backup_paths_reject_portable_aliases() {
        for name in [
            "recordings/a//clip.wav",
            "recordings/a/./clip.wav",
            "recordings/aux.wav",
            "recordings/COM1.mp3",
            "recordings/CON .wav",
            "recordings/CONOUT$",
            "templates/name?.json",
            "templates/name.json.",
            "templates/name.json ",
            "templates/line\nbreak.json",
        ] {
            assert!(safe_name(name).is_err(), "accepted {name:?}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn archive_private_directory_rejects_symlinks() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target");
        fs::create_dir(&target).unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(private_dir(&link).is_err());
    }

    #[test]
    fn restore_authenticates_and_does_not_publish_truncated_or_wrong_password_data() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("db");
        fs::write(&db, b"fixture database").unwrap();
        let backup = dir.path().join("copy.age");
        encrypt(
            &backup,
            SecretString::from("a long private passphrase".to_string()),
            vec![("meeting_minutes.sqlite".into(), db)],
            false,
        )
        .unwrap();
        let (stage, manifest) = decrypt(
            &backup,
            dir.path(),
            SecretString::from("a long private passphrase".to_string()),
        )
        .unwrap();
        assert_eq!(manifest.entries.len(), 1);
        assert_eq!(
            fs::read(stage.path().join("meeting_minutes.sqlite")).unwrap(),
            b"fixture database"
        );
        assert!(decrypt(
            &backup,
            dir.path(),
            SecretString::from("wrong passphrase".to_string())
        )
        .is_err());
        let mut bytes = fs::read(&backup).unwrap();
        bytes.truncate(bytes.len() - 8);
        fs::write(&backup, bytes).unwrap();
        assert!(decrypt(
            &backup,
            dir.path(),
            SecretString::from("a long private passphrase".to_string())
        )
        .is_err());
    }
    #[test]
    fn backup_paths_cannot_escape_staging() {
        for name in [
            "../keys",
            "/etc/passwd",
            "recordings/../../keys",
            "recordings\\bad",
            "C:/key",
            "integrations.json",
        ] {
            assert!(safe_name(name).is_err());
        }
        assert!(safe_name("recordings/meeting/audio.wav").is_ok());
    }
    #[tokio::test]
    async fn a_backup_may_only_bring_the_triggers_and_views_overtalk_creates() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        assert!(only_known_schema_code(&pool).await.is_ok());
        sqlx::query("CREATE TRIGGER rewrite_provider AFTER UPDATE ON meetings BEGIN UPDATE settings SET provider='attacker'; END")
            .execute(&pool)
            .await
            .unwrap();
        assert!(only_known_schema_code(&pool).await.is_err());
        sqlx::query("DROP TRIGGER rewrite_provider")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("CREATE VIEW extra AS SELECT 1")
            .execute(&pool)
            .await
            .unwrap();
        assert!(only_known_schema_code(&pool).await.is_err());
    }
    #[tokio::test]
    async fn snapshot_scrubs_plain_and_nested_provider_credentials() {
        let dir = tempfile::tempdir().unwrap();
        let pool = SqlitePool::connect(&format!(
            "sqlite:{}?mode=rwc",
            dir.path().join("db").display()
        ))
        .await
        .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        sqlx::query("INSERT INTO settings(id,provider,model,whisperModel,openaiApiKey,customOpenAIConfig) VALUES('s','custom','x','x','DO_NOT_EXPORT','{\"apiKey\":\"NESTED_SECRET\"}')").execute(&pool).await.unwrap();
        scrub_snapshot(&pool).await.unwrap();
        pool.close().await;
        let bytes = fs::read(dir.path().join("db")).unwrap();
        assert!(!bytes.windows(13).any(|w| w == b"DO_NOT_EXPORT"));
        assert!(!bytes.windows(13).any(|w| w == b"NESTED_SECRET"));
    }
    #[test]
    fn public_key_backups_require_the_recovery_key() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("fixture");
        fs::write(&db, b"private source").unwrap();
        let output = dir.path().join("backup.age");
        let key = age::x25519::Identity::generate();
        encrypt(
            &output,
            SecretString::from(key.to_public().to_string()),
            vec![("meeting_minutes.sqlite".into(), db)],
            false,
        )
        .unwrap();
        assert!(!fs::read(&output)
            .unwrap()
            .windows(14)
            .any(|w| w == b"private source"));
        let (stage, _) = decrypt(&output, dir.path(), key.to_string()).unwrap();
        assert_eq!(
            fs::read(stage.path().join("meeting_minutes.sqlite")).unwrap(),
            b"private source"
        );
        assert!(decrypt(
            &output,
            dir.path(),
            age::x25519::Identity::generate().to_string()
        )
        .is_err());
    }
}
