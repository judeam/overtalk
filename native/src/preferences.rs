//! Local appearance preferences and a process-held workspace lock.
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::PathBuf,
};

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Preferences {
    #[serde(default)]
    pub profile_version: u8,
    pub dark: bool,
    pub zoom: f32,
    pub timeline: bool,
    pub notes_timeline_fraction: f32,
    pub notes_highlights_open: bool,
    pub latin_devanagari: bool,
    pub reading_size: i32,
    pub motion_mode: i32,
    pub close_to_tray: bool,
    pub auto_summary: bool,
    pub setup_audio_confirmed: bool,
    pub default_summary_language: String,
    pub summary_template_id: String,
    pub guide_read: Vec<String>,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            profile_version: 2,
            dark: true,
            zoom: 1.0,
            timeline: true,
            notes_timeline_fraction: 0.0,
            notes_highlights_open: true,
            latin_devanagari: true,
            reading_size: 0,
            motion_mode: 0,
            close_to_tray: false,
            auto_summary: true,
            setup_audio_confirmed: false,
            default_summary_language: "auto".into(),
            summary_template_id: "standard_meeting".into(),
            guide_read: Vec::new(),
        }
    }
}
fn directory() -> Result<PathBuf, String> {
    let path = dirs::data_local_dir()
        .ok_or("No local application data directory")?
        .join("com.judeam.sonefold");
    fs::create_dir_all(&path).map_err(|e| e.to_string())?;
    Ok(path)
}
pub(crate) fn lock() -> Result<File, String> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(directory()?.join("native-ui.lock"))
        .map_err(|e| e.to_string())?;
    file.try_lock_exclusive()
        .map_err(|_| "Overtalk's native interface is already running.".to_owned())?;
    Ok(file)
}
pub(crate) fn load() -> Preferences {
    directory()
        .ok()
        .and_then(|p| fs::read(p.join("native-ui.json")).ok())
        .and_then(|v| serde_json::from_slice(&v).ok())
        .map(migrate)
        .unwrap_or_default()
}
// The preview silently chose Daily Standup for every meeting. Upgrade that
// legacy default once; versioned profiles keep subsequent explicit choices.
fn migrate(mut preferences: Preferences) -> Preferences {
    if preferences.profile_version == 0 && preferences.summary_template_id == "daily_standup" {
        preferences.summary_template_id = "standard_meeting".into();
    }
    if preferences.profile_version < 2 {
        preferences.auto_summary = true;
    }
    preferences.profile_version = 2;
    preferences
}

pub(crate) fn save(preferences: &Preferences) -> Result<(), String> {
    let path = directory()?;
    save_at(&path, preferences)
}
fn save_at(path: &std::path::Path, preferences: &Preferences) -> Result<(), String> {
    let mut file = tempfile::NamedTempFile::new_in(path).map_err(|e| e.to_string())?;
    file.write_all(&serde_json::to_vec(preferences).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    file.as_file().sync_all().map_err(|e| e.to_string())?;
    file.persist(path.join("native-ui.json"))
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preferences_can_replace_an_existing_file() {
        let directory = tempfile::tempdir().unwrap();
        save_at(directory.path(), &Preferences::default()).unwrap();
        let changed = Preferences {
            reading_size: 2,
            ..Default::default()
        };
        save_at(directory.path(), &changed).unwrap();
        let saved: Preferences =
            serde_json::from_slice(&fs::read(directory.path().join("native-ui.json")).unwrap())
                .unwrap();
        assert_eq!(saved.reading_size, 2);
    }
}

/// Coalesce preference changes and keep disk I/O off the UI thread.
pub(crate) struct Writer {
    sender: Option<std::sync::mpsc::SyncSender<()>>,
    latest: std::sync::Arc<std::sync::Mutex<Option<Preferences>>>,
    worker: Option<std::thread::JoinHandle<()>>,
}
impl Writer {
    pub(crate) fn new() -> Self {
        let latest = std::sync::Arc::new(std::sync::Mutex::new(None::<Preferences>));
        let pending = latest.clone();
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        let worker = std::thread::spawn(move || {
            while receiver.recv().is_ok() {
                while receiver.try_recv().is_ok() {}
                let next = pending.lock().unwrap().take();
                if let Some(preferences) = next {
                    if let Err(error) = save(&preferences) {
                        eprintln!("Could not save appearance settings: {error}");
                    }
                }
            }
        });
        Self {
            sender: Some(sender),
            latest,
            worker: Some(worker),
        }
    }
    pub(crate) fn enqueue(&self, preferences: Preferences) {
        *self.latest.lock().unwrap() = Some(preferences);
        if let Some(sender) = &self.sender {
            let _ = sender.try_send(());
        }
    }
}
impl Drop for Writer {
    fn drop(&mut self) {
        self.sender.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

pub(crate) fn snapshot(ui: &crate::AppWindow) -> Preferences {
    use slint::{ComponentHandle, Model};
    let state = ui.global::<crate::State>();
    Preferences {
        profile_version: 2,
        dark: state.get_dark(),
        zoom: state.get_zoom(),
        timeline: state.get_timeline_open(),
        notes_timeline_fraction: state.get_notes_timeline_fraction(),
        notes_highlights_open: state.get_notes_highlights_open(),
        latin_devanagari: state.get_latin_devanagari(),
        reading_size: state.get_reading_size(),
        motion_mode: state.get_motion_mode(),
        close_to_tray: state.get_close_to_tray(),
        auto_summary: state.get_auto_summary(),
        setup_audio_confirmed: state.get_setup_audio_confirmed(),
        default_summary_language: state.get_default_summary_language().into(),
        summary_template_id: state.get_summary_template_id().into(),
        guide_read: state
            .get_guide_read()
            .iter()
            .map(|id| id.to_string())
            .collect(),
    }
}

#[cfg(test)]
mod migration_tests {
    use super::*;
    #[test]
    fn replaces_only_the_legacy_preview_template_default() {
        let legacy: Preferences =
            serde_json::from_str(r#"{"summary_template_id":"daily_standup"}"#).unwrap();
        assert_eq!(migrate(legacy).summary_template_id, "standard_meeting");
        let custom: Preferences =
            serde_json::from_str(r#"{"summary_template_id":"my_template"}"#).unwrap();
        assert_eq!(migrate(custom).summary_template_id, "my_template");
        let explicit: Preferences =
            serde_json::from_str(r#"{"profile_version":1,"summary_template_id":"daily_standup"}"#)
                .unwrap();
        assert_eq!(migrate(explicit).summary_template_id, "daily_standup");
    }
}
