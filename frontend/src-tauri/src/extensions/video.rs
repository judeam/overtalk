//! Optional, explicit screen capture in a separate process. Original audio is
//! retained; completed video is attached only after a meeting has been saved.
use crate::state::AppState;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
struct Session {
    child: tokio::sync::Mutex<tokio::process::Child>,
    path: PathBuf,
    id: String,
    meeting_id: String,
    ready: AtomicBool,
    offset: AtomicU64,
}
static SESSION: tokio::sync::Mutex<Option<Arc<Session>>> = tokio::sync::Mutex::const_new(None);
static COMPLETED: tokio::sync::Mutex<Vec<(String, PathBuf, f64, String)>> =
    tokio::sync::Mutex::const_new(Vec::new());
fn persist_session(session: &Session) -> Result<(), String> {
    super::integrations::write_private_json(
        &session.path.with_extension("json"),
        &json!({"id":session.id,"meeting_id":session.meeting_id,"offset":f64::from_bits(session.offset.load(Ordering::Acquire)),"ready":session.ready.load(Ordering::Acquire)}),
    )
}
pub fn recover(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        let Ok(directory) = app.path().app_data_dir() else {
            return;
        };
        let directory = directory.join("pending-video");
        let Ok(mut entries) = tokio::fs::read_dir(&directory).await else {
            return;
        };
        let mut meetings = std::collections::HashSet::new();
        let mut scanned = 0;
        while let Ok(Some(entry)) = entries.next_entry().await {
            scanned += 1;
            if scanned > 4096 {
                break;
            }
            if entry.path().extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let path = entry.path();
            let Ok(Ok(bytes)) = tokio::task::spawn_blocking(move || {
                crate::security::storage::read_limited(&path, 8192)
            })
            .await
            else {
                continue;
            };
            if bytes.len() > 8192 {
                continue;
            }
            let Ok(meta) = serde_json::from_slice::<Value>(&bytes) else {
                continue;
            };
            let Some(id) = meta["id"].as_str() else {
                continue;
            };
            let Some(meeting) = meta["meeting_id"].as_str() else {
                continue;
            };
            if uuid::Uuid::parse_str(id).is_err() || meta["ready"] != true {
                continue;
            }
            let offset = meta["offset"].as_f64().unwrap_or(0.);
            if !offset.is_finite() || !(0.0..=86400.0).contains(&offset) {
                continue;
            }
            let video = directory.join(format!("{id}.mp4"));
            if !video.is_file() {
                continue;
            }
            let mut pending = COMPLETED.lock().await;
            if !pending.iter().any(|v| v.0 == id) {
                pending.push((id.into(), video, offset, meeting.into()));
            }
            meetings.insert(meeting.to_owned());
        }
        for meeting in meetings {
            let _ = workspace_video(
                app.clone(),
                app.state(),
                VideoAction::Attach {
                    meeting_id: meeting,
                },
            )
            .await;
        }
    });
}
#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum VideoAction {
    Targets,
    Start { target: String },
    Stop,
    Pause { paused: bool },
    Attach { meeting_id: String },
    Open { meeting_id: String },
    Status { meeting_id: String },
}
fn helper() -> Result<PathBuf, String> {
    let here = std::env::current_exe().map_err(|e| e.to_string())?;
    let path = here
        .parent()
        .ok_or("Missing installation directory")?
        .join(if cfg!(windows) {
            "overtalk-capture.exe"
        } else {
            "overtalk-capture"
        });
    if !path.is_file() {
        return Err("The screen capture helper is missing from this installation".into());
    }
    Ok(path)
}
#[tauri::command]
pub async fn workspace_video(
    app: AppHandle,
    state: State<'_, AppState>,
    action: VideoAction,
) -> Result<Value, String> {
    match action {
        VideoAction::Status { meeting_id } => {
            let exists: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM meeting_videos WHERE meeting_id=?)",
            )
            .bind(meeting_id)
            .fetch_one(state.db_manager.pool())
            .await
            .map_err(|e| e.to_string())?;
            Ok(json!({"has_video":exists}))
        }
        VideoAction::Targets => {
            #[cfg(target_os = "linux")]
            return Ok(
                json!([{"id":"system-picker","title":"Choose a window or screen in the system picker"}]),
            );
            #[cfg(not(target_os = "linux"))]
            {
                let mut child = tokio::process::Command::new(helper()?)
                    .stdin(std::process::Stdio::piped())
                    .stdout(std::process::Stdio::piped())
                    .kill_on_drop(true)
                    .spawn()
                    .map_err(|e| e.to_string())?;
                child
                    .stdin
                    .take()
                    .ok_or("Missing capture input")?
                    .write_all(b"{\"action\":\"targets\"}\n")
                    .await
                    .map_err(|e| e.to_string())?;
                let output = tokio::time::timeout(
                    std::time::Duration::from_secs(15),
                    child.wait_with_output(),
                )
                .await
                .map_err(|_| "Capture target selection timed out")?
                .map_err(|e| e.to_string())?;
                let value: Value = serde_json::from_slice(&output.stdout)
                    .map_err(|_| "Could not list screen capture targets")?;
                Ok(value["targets"].clone())
            }
        }
        VideoAction::Start { target } => {
            let meeting_id = crate::native_service::recording::active_id(&app)
                .ok_or("Start meeting audio before including video")?;
            let mut guard = SESSION.lock().await;
            if guard.is_some() {
                return Err("A video recording is already active".into());
            }
            let directory = app
                .path()
                .app_data_dir()
                .map_err(|e| e.to_string())?
                .join("pending-video");
            tokio::fs::create_dir_all(&directory)
                .await
                .map_err(|e| e.to_string())?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                tokio::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))
                    .await
                    .map_err(|e| e.to_string())?;
            }
            let id = uuid::Uuid::new_v4().to_string();
            let path = directory.join(format!("{id}.mp4"));
            let ffmpeg =
                crate::audio::ffmpeg::find_ffmpeg_path().ok_or("The video encoder is missing")?;
            let mut child = tokio::process::Command::new(helper()?)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::null())
                .kill_on_drop(true)
                .spawn()
                .map_err(|e| e.to_string())?;
            let mut request = serde_json::to_vec(
                &json!({"action":"record","path":path,"ffmpeg":ffmpeg,"target":target}),
            )
            .map_err(|e| e.to_string())?;
            request.push(b'\n');
            child
                .stdin
                .as_mut()
                .ok_or("Missing capture input")?
                .write_all(&request)
                .await
                .map_err(|e| e.to_string())?;
            let output = child.stdout.take().ok_or("Missing capture output")?;
            let session = Arc::new(Session {
                child: tokio::sync::Mutex::new(child),
                path,
                id,
                meeting_id,
                ready: AtomicBool::new(false),
                offset: AtomicU64::new(0f64.to_bits()),
            });
            *guard = Some(session.clone());
            if let Err(error) = persist_session(&session) {
                *guard = None;
                return Err(error);
            }
            tauri::async_runtime::spawn(async move {
                let mut lines = tokio::io::BufReader::new(output).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    if line.len() > 16384 {
                        break;
                    }
                    if let Ok(value) = serde_json::from_str::<Value>(&line) {
                        if value["ready"] == true {
                            let status =
                                crate::audio::recording_commands::get_recording_state().await;
                            session.offset.store(
                                status["active_duration"].as_f64().unwrap_or(0.).to_bits(),
                                Ordering::Release,
                            );
                            session.ready.store(true, Ordering::Release);
                            let _ = persist_session(&session);
                        }
                        let _ = app.emit("workspace-video-status", value);
                    }
                }
                // A denied picker or encoder failure must release the active
                // session so another explicit attempt can succeed.
                let mut current = SESSION.lock().await;
                if current
                    .as_ref()
                    .is_some_and(|active| Arc::ptr_eq(active, &session))
                {
                    *current = None;
                    if session.ready.load(Ordering::Acquire)
                        && session.path.metadata().is_ok_and(|m| m.len() > 100)
                    {
                        COMPLETED.lock().await.push((
                            session.id.clone(),
                            session.path.clone(),
                            f64::from_bits(session.offset.load(Ordering::Acquire)),
                            session.meeting_id.clone(),
                        ));
                    }
                    let _ = app.emit("workspace-video-status", json!({"stopped":true}));
                }
            });
            Ok(json!({"starting":true}))
        }
        VideoAction::Pause { paused } => {
            if let Some(session) = SESSION.lock().await.as_ref() {
                if let Some(input) = session.child.lock().await.stdin.as_mut() {
                    input
                        .write_all(if paused { b"pause\n" } else { b"resume\n" })
                        .await
                        .map_err(|e| e.to_string())?;
                }
            }
            Ok(Value::Null)
        }
        VideoAction::Stop => {
            let Some(session) = SESSION.lock().await.take() else {
                return Ok(Value::Null);
            };
            {
                let mut child = session.child.lock().await;
                if let Some(input) = child.stdin.as_mut() {
                    let _ = input.write_all(b"stop\n").await;
                }
            }
            for _ in 0..40 {
                let finished = session
                    .child
                    .lock()
                    .await
                    .try_wait()
                    .map_err(|e| e.to_string())?
                    .is_some();
                if finished {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            }
            {
                let mut child = session.child.lock().await;
                if child.try_wait().map_err(|e| e.to_string())?.is_none() {
                    let _ = child.kill().await;
                }
            }
            if session.ready.load(Ordering::Acquire)
                && session.path.metadata().is_ok_and(|m| m.len() > 100)
            {
                let _ = persist_session(&session);
                COMPLETED.lock().await.push((
                    session.id.clone(),
                    session.path.clone(),
                    f64::from_bits(session.offset.load(Ordering::Acquire)),
                    session.meeting_id.clone(),
                ));
            }
            let _ = app.emit("workspace-video-status", json!({"stopped":true}));
            Ok(json!({"path":session.path}))
        }
        VideoAction::Attach { meeting_id } => {
            let folder: Option<String> =
                sqlx::query_scalar("SELECT folder_path FROM meetings WHERE id=?")
                    .bind(&meeting_id)
                    .fetch_optional(state.db_manager.pool())
                    .await
                    .map_err(|e| e.to_string())?
                    .flatten();
            let folder = PathBuf::from(folder.ok_or("Save the meeting before attaching video")?)
                .canonicalize()
                .map_err(|e| e.to_string())?;
            let audio = crate::audio::retranscription::find_audio_file(&folder).ok();
            let (pending, remaining): (Vec<_>, Vec<_>) =
                std::mem::take(&mut *COMPLETED.lock().await)
                    .into_iter()
                    .partition(|v| v.3 == meeting_id);
            COMPLETED.lock().await.extend(remaining);
            let mut count = 0;
            let mut silent = 0;
            for (id, path, offset, owner) in pending {
                let output = folder.join(format!("video-{id}.mp4"));
                let result = if let Some(audio) = &audio {
                    let ffmpeg = crate::audio::ffmpeg::find_ffmpeg_path()
                        .ok_or("Video encoder is missing")?;
                    let mut sandbox =
                        overtalk_worker_sandbox::Sandbox::new(false).map_err(|e| e.to_string())?;
                    sandbox.read(&path).map_err(|e| e.to_string())?;
                    sandbox.read(audio).map_err(|e| e.to_string())?;
                    sandbox.output(&output).map_err(|e| e.to_string())?;
                    let mut command = sandbox.command(&ffmpeg).map_err(|e| e.to_string())?;
                    command
                        .args([
                            "-nostdin",
                            "-v",
                            "error",
                            "-y",
                            "-protocol_whitelist",
                            "file,pipe",
                            "-i",
                        ])
                        .arg(&path)
                        .args(["-ss", &format!("{offset:.3}"), "-i"])
                        .arg(audio)
                        .args([
                            "-map",
                            "0:v",
                            "-map",
                            "1:a",
                            "-c:v",
                            "copy",
                            "-c:a",
                            "aac",
                            "-shortest",
                        ])
                        .arg(&output);
                    tokio::task::spawn_blocking(move || {
                        let _sandbox = sandbox;
                        overtalk_worker_sandbox::run_bounded(
                            command,
                            65536,
                            std::time::Duration::from_secs(300),
                        )
                    })
                    .await
                    .ok()
                    .and_then(Result::ok)
                    .is_some_and(|o| o.status.success())
                } else {
                    false
                };
                if !result {
                    // A failed remux may have left a partial destination.
                    let _ = tokio::fs::remove_file(&output).await;
                    if tokio::fs::copy(&path, &output).await.is_err() {
                        COMPLETED.lock().await.push((id, path, offset, owner));
                        continue;
                    }
                    silent += 1;
                }
                sqlx::query("INSERT OR IGNORE INTO meeting_videos VALUES(?,?,?,?,?)")
                    .bind(id)
                    .bind(&meeting_id)
                    .bind(output.to_string_lossy().as_ref())
                    .bind(offset)
                    .bind(chrono::Utc::now().to_rfc3339())
                    .execute(state.db_manager.pool())
                    .await
                    .map_err(|e| e.to_string())?;
                let _ = tokio::fs::remove_file(&path).await;
                let _ = tokio::fs::remove_file(path.with_extension("json")).await;
                count += 1;
            }
            Ok(json!({"attached":count,"silent":silent}))
        }
        VideoAction::Open { meeting_id } => {
            let row:Option<(String,String)>=sqlx::query_as("SELECT v.path,m.folder_path FROM meeting_videos v JOIN meetings m ON m.id=v.meeting_id WHERE v.meeting_id=? ORDER BY v.created_at DESC LIMIT 1").bind(meeting_id).fetch_optional(state.db_manager.pool()).await.map_err(|e|e.to_string())?;
            let (path, folder) = row.ok_or("This meeting has no saved video")?;
            let path = PathBuf::from(path)
                .canonicalize()
                .map_err(|e| e.to_string())?;
            if !path.starts_with(
                PathBuf::from(folder)
                    .canonicalize()
                    .map_err(|e| e.to_string())?,
            ) {
                return Err("Video is outside the meeting folder".into());
            }
            tokio::task::spawn_blocking(move || {
                let mut cmd = if cfg!(windows) {
                    let mut cmd = std::process::Command::new("rundll32.exe");
                    cmd.arg("url.dll,FileProtocolHandler");
                    cmd
                } else if cfg!(target_os = "macos") {
                    std::process::Command::new("/usr/bin/open")
                } else {
                    std::process::Command::new("xdg-open")
                };
                cmd.arg(path).spawn().map_err(|e| e.to_string()).map(|_| ())
            })
            .await
            .map_err(|e| e.to_string())??;
            Ok(Value::Null)
        }
    }
}
