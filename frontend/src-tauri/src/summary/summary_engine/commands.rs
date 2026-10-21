// Tauri commands for built-in AI model management
// Exposes model download, status, and management functionality to frontend

use tauri::{AppHandle, Emitter, Manager, Runtime, State};

pub use super::manager_state::ModelManagerState;
use super::model_manager::{DownloadProgress, ModelInfo};

const QWEN35_4B_RECOMMENDED_RAM_GB: u64 = 14;

pub(crate) fn recommend_summary_model(_is_macos: bool, system_ram_gb: u64) -> &'static str {
    if system_ram_gb >= QWEN35_4B_RECOMMENDED_RAM_GB {
        "qwen3.5:4b"
    } else {
        "qwen3.5:2b"
    }
}

pub(crate) fn get_recommended_summary_model_for_current_system() -> Result<&'static str, String> {
    let profile = crate::extensions::quality_profile::detected();
    log::info!(
        "Local quality profile: RAM={} MiB, CUDA VRAM={:?} MiB, summary={}",
        profile.system_ram_mib,
        profile.cuda_vram_mib,
        profile.summary_model
    );
    Ok(profile.summary_model)
}

/// List all available built-in AI models with their status
#[tauri::command]
pub async fn builtin_ai_list_models<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, ModelManagerState>,
) -> Result<Vec<ModelInfo>, String> {
    let manager = state.get_or_initialize(&app).await?;

    let models = manager.list_models().await;
    Ok(models)
}

/// Download a built-in AI model with progress updates
#[tauri::command]
pub async fn builtin_ai_download_model<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, ModelManagerState>,
    model_name: String,
) -> Result<(), String> {
    let manager = state.get_or_initialize(&app).await?;
    // IMPORTANT: Only emit "downloading" status here, never "completed"
    // Completion event is emitted AFTER download task fully finishes (validation, etc.)
    let app_clone = app.clone();
    let model_name_clone = model_name.clone();
    let progress_callback = Box::new(move |progress: DownloadProgress| {
        let _ = app_clone.emit(
            "builtin-ai-download-progress",
            serde_json::json!({
                "model": model_name_clone,
                "progress": progress.percent,
                "downloaded_mb": progress.downloaded_mb,
                "total_mb": progress.total_mb,
                "speed_mbps": progress.speed_mbps,
                "status": "downloading"  // Always "downloading", never "completed" from progress callback
            }),
        );
    });

    match manager
        .download_model_detailed(&model_name, Some(progress_callback))
        .await
    {
        Ok(_) => {
            // Download task completed successfully (validation passed, status set to Available)
            let _ = app.emit(
                "builtin-ai-download-progress",
                serde_json::json!({
                    "model": model_name,
                    "progress": 100,
                    "downloaded_mb": 0,  // Not used by completion handler
                    "total_mb": 0,       // Not used by completion handler
                    "speed_mbps": 0,     // Not used by completion handler
                    "status": "completed"
                }),
            );
            Ok(())
        }
        Err(e) => {
            let error_msg = e.to_string();

            // Check if this is a cancellation error (marked with "CANCELLED:" prefix)
            // Don't emit error event for cancellations - cancel command already emits cancelled event
            if !error_msg.starts_with("CANCELLED:") {
                // Emit error via progress event for frontend to display (only for real errors)
                let _ = app.emit(
                    "builtin-ai-download-progress",
                    serde_json::json!({
                        "model": model_name,
                        "progress": 0,
                        "downloaded_mb": 0,
                        "total_mb": 0,
                        "speed_mbps": 0,
                        "status": "error",
                        "error": error_msg
                    }),
                );
            }
            Err(error_msg)
        }
    }
}

/// Cancel an ongoing model download
#[tauri::command]
pub async fn builtin_ai_cancel_download<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, ModelManagerState>,
    model_name: String,
) -> Result<(), String> {
    let manager = state.ready()?;

    manager
        .cancel_download(&model_name)
        .await
        .map_err(|e| e.to_string())?;

    let _ = app.emit(
        "builtin-ai-download-progress",
        serde_json::json!({
            "model": model_name,
            "progress": 0,
            "status": "cancelled"
        }),
    );

    Ok(())
}

/// Delete a corrupted or available model file
#[tauri::command]
pub async fn builtin_ai_delete_model(
    state: State<'_, ModelManagerState>,
    model_name: String,
) -> Result<(), String> {
    let manager = state.ready()?;

    manager
        .delete_model(&model_name)
        .await
        .map_err(|e| e.to_string())
}

/// Check if a model is ready to use
#[tauri::command]
pub async fn builtin_ai_is_model_ready<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, ModelManagerState>,
    model_name: String,
    refresh: Option<bool>,
) -> Result<bool, String> {
    let manager = state.get_or_initialize(&app).await?;

    let refresh_scan = refresh.unwrap_or(false);
    let ready = manager.is_model_ready(&model_name, refresh_scan).await;

    log::info!(
        "Model '{}' ready check (refresh={}): {}",
        model_name,
        refresh_scan,
        ready
    );

    Ok(ready)
}

// ============================================================================
// Startup Initialization & Utility Commands
// ============================================================================

pub async fn init_model_manager_at_startup<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    app.state::<ModelManagerState>()
        .get_or_initialize(app)
        .await?;
    Ok(())
}

/// Get the recommended model from the detected local quality profile.
#[tauri::command]
pub async fn builtin_ai_get_recommended_model() -> Result<String, String> {
    let recommended = get_recommended_summary_model_for_current_system()?;

    log::info!("Recommended summary model: {}", recommended);
    Ok(recommended.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recommended_summary_model_uses_qwen2b_below_effective_16gb_floor() {
        assert_eq!(recommend_summary_model(true, 13), "qwen3.5:2b");
        assert_eq!(recommend_summary_model(false, 13), "qwen3.5:2b");
    }

    #[test]
    fn recommended_summary_model_uses_qwen4b_at_effective_16gb_floor() {
        assert_eq!(recommend_summary_model(true, 14), "qwen3.5:4b");
        assert_eq!(recommend_summary_model(false, 14), "qwen3.5:4b");
    }
}
