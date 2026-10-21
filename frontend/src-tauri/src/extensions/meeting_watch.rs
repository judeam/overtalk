//! Opt-in local meeting hints. Browser signals arrive through native messaging;
//! process presence is labelled as such and is never treated as proof of a call.
use crate::state::AppState;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager};
#[derive(Default, Clone, Serialize, Deserialize)]
pub struct WatchSettings {
    pub enabled: bool,
    pub show_widget: bool,
}
fn path(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("meeting-watch.json"))
}
fn read(app: &AppHandle) -> WatchSettings {
    path(app)
        .ok()
        .and_then(|p| crate::security::storage::read_limited(&p, 8192).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}
#[tauri::command]
pub async fn workspace_meeting_watch(
    app: AppHandle,
    settings: Option<WatchSettings>,
) -> Result<WatchSettings, String> {
    if let Some(settings) = settings {
        super::integrations::write_private_json(&path(&app)?, &settings)?;
    }
    Ok(read(&app))
}
pub fn start(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut prior = String::new();
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
            let config = read(&app);
            if !config.enabled {
                continue;
            }
            let Some(state) = app.try_state::<AppState>() else {
                continue;
            };
            let directory = match app.path().app_data_dir() {
                Ok(p) => p,
                Err(_) => continue,
            };
            let browser = crate::security::storage::read_limited(
                &directory.join("browser-meeting.json"),
                8192,
            )
            .ok()
            .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
            .filter(|v| chrono::Utc::now().timestamp() - v["seen_at"].as_i64().unwrap_or(0) < 35);
            let at = chrono::Utc::now();
            let upcoming: Option<(String, String, Option<String>, String)> = sqlx::query_as(
                "SELECT id,title,meeting_url,participants FROM workspace_calendar \
                 WHERE starts_at<=? AND ends_at>=? AND all_day=0 ORDER BY starts_at LIMIT 1",
            )
            .bind((at + chrono::Duration::minutes(5)).to_rfc3339())
            .bind(at.to_rfc3339())
            .fetch_optional(state.db_manager.pool())
            .await
            .unwrap_or(None);
            let hint = if let Some(browser) = browser {
                json!({
                    "id": browser["url"],
                    "title": browser["title"],
                    "url": browser["url"],
                    "description": if browser["active"] == true {
                        "Video is active on this meeting page"
                    } else {
                        "A supported meeting page is open"
                    },
                    "show_widget": config.show_widget
                })
            } else if let Some((id, title, url, participants)) = upcoming {
                json!({
                    "id": id,
                    "title": title,
                    "url": url,
                    "participants": participants,
                    "description": "Your scheduled meeting is starting",
                    "show_widget": config.show_widget
                })
            } else {
                let app_name = tokio::task::spawn_blocking(|| {
                    let system = sysinfo::System::new_with_specifics(
                        sysinfo::RefreshKind::new()
                            .with_processes(sysinfo::ProcessRefreshKind::new()),
                    );
                    system.processes().values().find_map(|p| {
                        let n = p.name().to_string_lossy().to_lowercase();
                        if n == "zoom" || n == "zoom.exe" {
                            Some("Zoom")
                        } else if n == "ms-teams.exe" || n == "teams.exe" || n == "teams" {
                            Some("Microsoft Teams")
                        } else {
                            None
                        }
                    })
                })
                .await
                .ok()
                .flatten();
                app_name
                    .map(|name| {
                        json!({
                            "id": name,
                            "title": name,
                            "url": "",
                            "description":
                                "Meeting app is open. Start recording when your call begins.",
                            "show_widget": config.show_widget
                        })
                    })
                    .unwrap_or(Value::Null)
            };
            let revision = hint.to_string();
            if revision != prior {
                prior = revision;
                let _ = app.emit("workspace-meeting-detected", hint);
            }
        }
    });
}
