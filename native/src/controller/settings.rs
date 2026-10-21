//! Settings presentation and recording-device preferences.

use super::*;

/// Assistants `overtalk-cli setup` configures, by its name for each.
const ASSISTANTS: [(&str, &str); 5] = [
    ("claude-code", "Claude Code"),
    ("claude-desktop", "Claude Desktop"),
    ("codex", "Codex"),
    ("cursor", "Cursor"),
    ("vscode", "VS Code"),
];

fn quoted(path: &str) -> String {
    match (path.contains(' '), cfg!(windows)) {
        (false, _) => path.to_owned(),
        (true, true) => format!("\"{path}\""),
        (true, false) => format!("'{}'", path.replace('\'', r"'\''")),
    }
}

/// How to run overtalk-cli in a terminal: by name when the copy installed
/// beside this app is the one on PATH, otherwise by its path. An AppImage
/// runs it as its `cli` command.
fn cli_command() -> String {
    static COMMAND: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    COMMAND
        .get_or_init(|| {
            if let Some(appimage) = std::env::var_os("APPIMAGE") {
                return format!("{} cli", quoted(&appimage.to_string_lossy()));
            }
            let name = if cfg!(windows) {
                "overtalk-cli.exe"
            } else {
                "overtalk-cli"
            };
            let beside = std::env::current_exe()
                .ok()
                .and_then(|exe| Some(exe.parent()?.join(name)))
                .and_then(|path| std::fs::canonicalize(path).ok());
            let Some(beside) = beside else {
                return "overtalk-cli".into();
            };
            let on_path = std::env::var_os("PATH").is_some_and(|path| {
                std::env::split_paths(&path)
                    .any(|dir| std::fs::canonicalize(dir.join(name)).ok().as_ref() == Some(&beside))
            });
            if on_path {
                "overtalk-cli".into()
            } else {
                quoted(&beside.to_string_lossy())
            }
        })
        .clone()
}

impl Controller {
    /// One setup command per assistant. Nothing is offered until the person
    /// turns on MCP or command line access; everything starts off.
    fn apply_assistants(&self, ui: &AppWindow, settings: &Value) {
        let st = ui.global::<State>();
        st.set_assistants_on(settings["mcp_enabled"] == true || settings["cli_enabled"] == true);
        let cli = cli_command();
        let mut rows: Vec<DetailRow> = ASSISTANTS
            .iter()
            .map(|(id, title)| DetailRow {
                id: (*id).into(),
                title: (*title).into(),
                value: format!("{cli} setup {id} --write").into(),
                ..Default::default()
            })
            .collect();
        rows.push(DetailRow {
            id: "skill".into(),
            title: "Overtalk skill for Claude Code".into(),
            description: "Teaches it to find decisions, prepare from briefs and cite each source."
                .into(),
            value: format!("{cli} skill install").into(),
            ..Default::default()
        });
        st.set_assistant_commands(model(rows));
    }

    pub(super) fn assistant_action(&self, ui: &AppWindow, action: &str, value: &str) {
        let st = ui.global::<State>();
        if action == "assistant-copy" {
            if let Some(row) = st
                .get_assistant_commands()
                .iter()
                .find(|row| row.id == value)
            {
                ui.invoke_copy_text(row.value);
                st.set_message("Command copied. Paste it into a terminal.".into());
            }
        }
    }

    pub(super) fn save_audio_devices(&mut self, ui: &AppWindow) {
        if let Some(mut preferences) = self.settings.get("get_recording_preferences").cloned() {
            preferences["preferred_mic_device"] = json!(self.device_name(ui, true));
            preferences["preferred_system_device"] = json!(self.device_name(ui, false));
            self.settings
                .insert("get_recording_preferences".into(), preferences.clone());
            self.request(
                ui,
                "set_recording_preferences",
                json!({"preferences": preferences}),
                "ignored",
                "",
            );
        }
    }

    pub(super) fn apply_settings(&mut self, ui: &AppWindow) {
        let st = ui.global::<State>();
        if let Some(config) = self.settings.get("api_get_transcript_config") {
            st.set_speech_model(text(config, "model").into());
        }
        if let Some(config) = self.settings.get("api_get_model_config") {
            st.set_summary_model(text(config, "model").into());
        }
        if let Some(config) = self.settings.get("get_recording_preferences") {
            st.set_folder(text(config, "save_folder").into());
        }
        if let Some(status) = self.settings.get("integrations_get_settings") {
            self.apply_assistants(ui, &status["settings"]);
        }
        if let Some(devices) = self.settings.get("get_audio_devices") {
            let rows = array(devices);
            let mut mic = vec!["Default microphone".into()];
            let mut system = vec!["Default call audio".into()];
            self.mic_ids = vec![None];
            self.system_ids = vec![None];
            for device in rows {
                let name = text(&device, "name");
                if name.is_empty() || !text(&device, "unavailable_reason").is_empty() {
                    continue;
                }
                let display = text(&device, "display_name");
                let label = if display.is_empty() {
                    name.clone()
                } else {
                    display
                };
                if text(&device, "device_type")
                    .to_lowercase()
                    .contains("input")
                {
                    mic.push(label.into());
                    self.mic_ids.push(Some(format!("{name} (input)")));
                } else {
                    system.push(label.into());
                    self.system_ids.push(Some(format!("{name} (output)")));
                }
            }
            st.set_mic_devices(model(mic));
            st.set_system_devices(model(system));
            if let Some(preferences) = self.settings.get("get_recording_preferences") {
                for (key, ids, mic) in [
                    ("preferred_mic_device", &self.mic_ids, true),
                    ("preferred_system_device", &self.system_ids, false),
                ] {
                    let selected = preferences[key].as_str();
                    let index = ids
                        .iter()
                        .position(|id| id.as_deref() == selected)
                        .unwrap_or(0) as i32;
                    if mic {
                        st.set_mic_index(index);
                    } else {
                        st.set_system_index(index);
                    }
                }
            }
        }
    }

    pub(super) fn settings_view(&self, ui: &AppWindow, tab: i32) {
        if tab == 3 {
            ui.global::<State>().set_cloud_loading(true);
            self.request(
                ui,
                "cloud_account",
                json!({"action":{"op":"status"}}),
                "cloud-status",
                "",
            );
        }
        if tab == 7 {
            self.request(
                ui,
                "workspace_privacy",
                json!({"action":{"op":"status"}}),
                "privacy-status",
                "",
            );
        }
        let st = ui.global::<State>();
        if tab == 5 && (st.get_edition_auto_assign() || st.get_edition_briefs()) {
            self.request(
                ui,
                "workspace_workflow",
                json!({"action":{"op":"automation","settings":null}}),
                "automation-settings",
                "",
            );
        }
        st.set_details(model(Vec::new()));
        if tab == 2 || tab == 3 {
            st.set_provider("".into());
            st.set_model("".into());
            st.set_api_key("".into());
            st.set_endpoint("".into());
            let key = if tab == 2 {
                "api_get_transcript_config"
            } else {
                "api_get_model_config"
            };
            if let Some(config) = self.settings.get(key) {
                st.set_provider(text(config, "provider").into());
                st.set_model(text(config, "model").into());
                st.set_api_key("".into());
                st.set_endpoint(text(config, "ollamaEndpoint").into());
                if st.get_provider() == "custom-openai" {
                    if let Some(custom) = self.settings.get("api_get_custom_openai_config") {
                        st.set_endpoint(text(custom, "endpoint").into());
                        st.set_model(text(custom, "model").into());
                    }
                }
            }
        }
        let rows = match tab {
            2 => vec![(
                "speech",
                "Advanced speech",
                "Local Python engines, diarization and language settings.",
            )],
            4 => vec![(
                "library",
                "Search configuration",
                "Index and retrieval settings for your local meeting library.",
            )],
            5 => vec![(
                "connections",
                "Connections",
                "Explicitly enable local API, CLI, MCP or webhooks.",
            )],
            6 => vec![
                (
                    "gpu",
                    "GPU memory",
                    "Inspect memory use and release idle models.",
                ),
                (
                    "diagnostics",
                    "Diagnostics",
                    "Local runtime and recording health.",
                ),
            ],
            _ => Vec::new(),
        };
        st.set_details(model(
            rows.into_iter()
                .map(|(id, title, description)| DetailRow {
                    id: id.into(),
                    title: title.into(),
                    description: description.into(),
                    ..Default::default()
                })
                .collect(),
        ));
    }
}
