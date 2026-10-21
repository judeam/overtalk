//! Resolve a packaged or explicitly installed encoder. Never download or run a
//! program from the current working directory during application startup.
use once_cell::sync::Lazy;
use std::path::PathBuf;

#[cfg(windows)]
const EXECUTABLE_NAME: &str = "ffmpeg.exe";
#[cfg(not(windows))]
const EXECUTABLE_NAME: &str = "ffmpeg";
static FFMPEG_PATH: Lazy<Option<PathBuf>> = Lazy::new(find_ffmpeg_path_internal);
pub fn find_ffmpeg_path() -> Option<PathBuf> {
    FFMPEG_PATH.clone()
}
fn find_ffmpeg_path_internal() -> Option<PathBuf> {
    if let Ok(executable) = std::env::current_exe() {
        if let Some(directory) = executable.parent() {
            for candidate in [
                directory.join(EXECUTABLE_NAME),
                directory.join("../Resources").join(EXECUTABLE_NAME),
                directory.join("lib").join(EXECUTABLE_NAME),
            ] {
                if candidate.is_file() {
                    return candidate.canonicalize().ok();
                }
            }
        }
    }
    // System package-manager locations only. The working directory and a
    // relative PATH entry must never supply an executable for imported media.
    #[cfg(unix)]
    for directory in ["/usr/bin", "/usr/local/bin", "/opt/homebrew/bin"] {
        let candidate = PathBuf::from(directory).join(EXECUTABLE_NAME);
        if candidate.is_file() {
            return candidate.canonicalize().ok();
        }
    }
    log::error!(
        "Packaged audio encoder missing; reinstall Overtalk or install the system FFmpeg package"
    );
    None
}
