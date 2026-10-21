//! Coordinate database availability with the desktop's first-launch flow.
use super::manager::DatabaseManager;
use crate::state::AppState;
use tauri::{AppHandle, Emitter, Manager};

pub(crate) async fn initialize(app: &AppHandle, native: bool) -> Result<(), String> {
    let first_launch = DatabaseManager::is_first_launch(app)
        .await
        .map_err(|error| format!("Could not inspect the meeting library: {error}"))?;

    // The native UI needs storage immediately. The web onboarding flow can
    // instead import a legacy library before opening its first database.
    if native || !first_launch {
        let db_manager = DatabaseManager::new_from_app_handle(app)
            .await
            .map_err(|error| format!("Could not open the meeting library: {error}"))?;
        app.manage(AppState { db_manager });
    }

    if first_launch {
        let ready_app = app.clone();
        tauri::async_runtime::spawn(async move {
            // Preserve the web listener-registration grace period. Failure to
            // deliver a UI hint must not panic an otherwise healthy service.
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            if let Err(error) = ready_app.emit("first-launch-detected", ()) {
                log::warn!("Could not deliver the first-launch notification: {error}");
            }
        });
    }
    Ok(())
}
