/// Opens macOS System Settings to a specific privacy preference pane
#[cfg(target_os = "macos")]
#[tauri::command]
pub async fn open_system_settings(preference_pane: String) -> Result<(), String> {
    use std::process::Command;

    // Construct the URL for System Settings
    let url = format!(
        "x-apple.systempreferences:com.apple.preference.security?{}",
        preference_pane
    );

    // Use the 'open' command on macOS to open the URL
    Command::new("/usr/bin/open")
        .arg(&url)
        .spawn()
        .map_err(|e| format!("Failed to open system settings: {}", e))?;

    Ok(())
}

/// Open an already-authorized folder with the platform file manager.
/// Callers retain ownership of path validation and directory creation.
pub(crate) fn open_folder(folder: &std::path::Path) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    let program = "explorer";
    #[cfg(target_os = "macos")]
    let program = "/usr/bin/open";
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let program = "xdg-open";

    std::process::Command::new(program)
        .arg(folder)
        .spawn()
        .map_err(|error| format!("Failed to open folder: {error}"))?;
    Ok(())
}
