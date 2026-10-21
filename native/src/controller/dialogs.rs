use super::*;

impl Controller {
    pub(super) fn dialog(
        &mut self,
        ui: &AppWindow,
        action: &str,
        title: &str,
        text: &str,
        kind: &str,
        data: Value,
    ) {
        self.cancel_account_signin(ui);
        self.dialog = (action.into(), data);
        let st = ui.global::<State>();
        st.set_connection_provider("".into());
        st.set_connection_busy(false);
        st.set_connection_browser(false);
        st.set_connection_ready(false);
        st.set_connection_code("".into());
        st.set_choice_total(0);
        st.set_choice_height(0);
        st.set_modal(kind.into());
        st.set_modal_title(title.into());
        st.set_modal_text(text.into());
        st.set_modal_value("".into());
        st.set_modal_secondary("".into());
        st.set_modal_secondary_label("".into());
        st.set_modal_has_date(false);
        st.set_modal_date("".into());
        st.set_modal_submit("Save".into());
        st.set_error("".into());
    }

    pub(super) fn choices(
        &mut self,
        ui: &AppWindow,
        action: &str,
        title: &str,
        rows: Vec<(String, String)>,
        data: Value,
    ) {
        let rows = rows
            .into_iter()
            .map(|(id, title)| menus::present(action, id, title))
            .collect();
        self.rich_choices(ui, action, title, rows, data);
    }

    /// Choices whose rows already carry their description and icon.
    pub(super) fn rich_choices(
        &mut self,
        ui: &AppWindow,
        action: &str,
        title: &str,
        rows: Vec<DetailRow>,
        data: Value,
    ) {
        self.dialog(ui, action, title, "", "choice", data);
        self.choice_rows = rows;
        let state = ui.global::<State>();
        state.set_choice_query("".into());
        state.set_choice_total(self.choice_rows.len() as i32);
        state.set_choice_height(
            self.choice_rows
                .iter()
                .map(|r| if r.description.is_empty() { 50 } else { 78 })
                .sum(),
        );
        state.set_modal_choices(model(self.choice_rows.clone()));
    }

    pub(super) fn form(&mut self, ui: &AppWindow, action: &str, title: &str, data: Value) {
        self.dialog(
            ui,
            action,
            title,
            "Changes take effect when you save.",
            "form",
            data,
        );
        self.render_form(ui);
    }

    /// Remembering a voice records how the person agreed first: voice used to recognise
    /// someone is biometric data, which needs their explicit consent.
    pub(super) fn voice_consent_form(
        &mut self,
        ui: &AppWindow,
        speaker: &str,
        name: &str,
        person: &str,
    ) {
        self.form(
            ui,
            "remember-voice",
            "Remember this voice",
            json!({"name":name,"consent":"in_meeting","consent_note":"","confirmed":false,"speaker_id":speaker,"person_id":person}),
        );
        let st = ui.global::<State>();
        st.set_modal_text("The voice profile is kept in this computer's Overtalk library and compared only here. Recognising a meeting also analyses the other speakers' voices; nothing is kept for anyone you have not enrolled. You can export or forget this voice at any time.".into());
        st.set_modal_submit("Remember voice".into());
    }

    pub(super) fn render_form(&self, ui: &AppWindow) {
        let mut rows = Vec::new();
        if self.dialog.0 == "hub-connection-config" {
            rows = connection_form_rows(&self.dialog.1);
        } else {
            collect_form_rows(&self.dialog.1, "", &mut rows);
        }
        let order: &[&str] = match self.dialog.0.as_str() {
            "privacy-policy" => &[
                "local_only",
                "require_recording_notice",
                "recording_notice_local",
                "recording_notice_external",
                "voice_recognition_automatic",
                "retention_days",
                "automatic_retention",
                "protected_mode",
                "organization",
                "privacy_contact",
                "recording_basis",
                "approved_hosts_text",
                "agreement_reference",
                "audit_retention_days",
                "diagnostic_retention_days",
            ],
            "privacy-voice-add" => &["name", "basis", "reference", "days"],
            "remember-voice" => &["name", "consent", "consent_note", "confirmed"],
            _ => &[],
        };
        if !order.is_empty() {
            rows.sort_by_key(|row| {
                order
                    .iter()
                    .position(|key| row.key.as_str().trim_start_matches('/') == *key)
                    .unwrap_or(usize::MAX)
            });
        }
        if self.dialog.0 == "privacy-voice-add" {
            for row in &mut rows {
                if row.key == "/basis" {
                    row.kind = 3;
                }
            }
        }
        if self.dialog.0 == "remember-voice" {
            // A project person's name comes from the project; the ids are not for editing.
            let person = !text(&self.dialog.1, "person_id").is_empty();
            rows.retain(|row| match row.key.as_str() {
                "/speaker_id" | "/person_id" => false,
                "/name" => !person,
                _ => true,
            });
            for row in &mut rows {
                if row.key == "/consent" {
                    row.kind = 4;
                }
            }
        }
        ui.global::<State>().set_form_rows(model(rows));
    }

    pub(crate) fn form_edited(&mut self, ui: &AppWindow, key: &str, value: &str) {
        if let Some(key) = key.strip_prefix("template:") {
            self.template_field(key, value);
            return;
        }
        if let Some(old) = self.dialog.1.pointer_mut(key) {
            let next = if old.is_boolean() {
                Value::Bool(value == "true")
            } else if old.is_number()
                || (old.is_null()
                    && matches!(key, "/maxTokens" | "/temperature" | "/topP")
                    && !value.trim().is_empty())
            {
                match serde_json::from_str::<Value>(value) {
                    Ok(number) if number.is_number() => number,
                    _ => {
                        ui.global::<State>()
                            .set_error("Enter a valid number.".into());
                        return;
                    }
                }
            } else if old.is_null() && value.is_empty() {
                Value::Null
            } else {
                Value::String(value.into())
            };
            *old = next;
            // Preserve cursor position in text fields; only toggles need a row update.
            if old.is_boolean() {
                self.render_form(ui);
            }
        }
    }

    pub(super) fn modal_save(&mut self, ui: &AppWindow) {
        if ui.global::<State>().get_modal() == "template" {
            self.save_template(ui);
            return;
        }
        let st = ui.global::<State>();
        let (action, data) = self.dialog.clone();
        if action.starts_with("privacy-") {
            self.privacy_modal_save(ui, &action, data);
            return;
        }
        if action.starts_with("hub-") {
            self.hub_modal_save(ui, &action, data);
            return;
        }
        if action.starts_with("automation-") {
            self.automation_save(ui, &action);
            return;
        }
        if action == "share-options" {
            self.share_save(ui);
            return;
        }
        match action.as_str() {
            "add-mark" => {
                self.request(
                    ui,
                    "meeting_add_marks",
                    json!({
                        "meetingId": self.active,
                        "marks": [{
                            "at": data["at"],
                            "label": st.get_modal_value().as_str()
                        }]
                    }),
                    "marks",
                    &self.active,
                );
                st.set_modal("".into());
            }
            "notes-language" => {
                let code = st.get_modal_value().trim().to_owned();
                st.set_notes_language_code(code.clone().into());
                self.request(
                    ui,
                    "api_save_meeting_summary_language",
                    json!({"meetingId": self.active, "summaryLanguage": code}),
                    "ignored",
                    "",
                );
                st.set_modal("".into());
            }
            "custom-provider" => {
                let config = json!({
                    "provider": "custom-openai",
                    "model": data["model"],
                    "ollamaEndpoint": data["endpoint"],
                    "whisperModel": self
                        .settings
                        .get("api_get_model_config")
                        .and_then(|value| value["whisperModel"].as_str())
                        .unwrap_or("base")
                });
                self.request(
                    ui,
                    "api_save_custom_openai_config",
                    data,
                    "custom-provider-saved",
                    &config.to_string(),
                );
            }
            "delete-model" => {
                let provider = text(&data, "provider");
                let command = match provider.as_str() {
                    "localWhisper" => "whisper_delete_corrupted_model",
                    "parakeet" => "parakeet_delete_corrupted_model",
                    _ => "builtin_ai_delete_model",
                };
                self.request(
                    ui,
                    command,
                    json!({"modelName": data["name"]}),
                    "model-changed",
                    &provider,
                );
            }
            "template-delete" => self.request(
                ui,
                "api_delete_template",
                json!({"templateId": data}),
                "template-deleted",
                "",
            ),
            "audio-sources" => st.set_modal("".into()),
            "diagnostics" => st.set_modal("".into()),
            "edit-transcript" => self.request(
                ui,
                "workspace_correct_transcript",
                json!({
                    "change": {
                        "meeting_id": self.active,
                        "segment_id": text(&data, "id"),
                        "text": st.get_modal_value().as_str(),
                        "speaker_id": data["speaker_id"],
                        "expected_text": data["text"],
                        "expected_speaker_id": data["speaker_id"]
                    }
                }),
                "mutation",
                "",
            ),
            "rename-meeting" => self.request(
                ui,
                "api_save_meeting_title",
                json!({
                    "meetingId": self.active,
                    "title": st.get_modal_value().as_str()
                }),
                "mutation",
                "",
            ),
            "rename-speaker" => self.request(
                ui,
                "rename_meeting_speaker",
                json!({
                    "meetingId": self.active,
                    "speakerId": data,
                    "name": st.get_modal_value().as_str()
                }),
                "mutation",
                "",
            ),
            "remember-voice" => {
                if data["confirmed"] != true {
                    st.set_error(
                        "Confirm that they explicitly agreed before remembering their voice."
                            .into(),
                    );
                    return;
                }
                let person = text(&data, "person_id");
                st.set_voice_busy(true);
                st.set_modal("".into());
                st.set_message("Saving voice samples on this device…".into());
                self.request(
                    ui,
                    "remember_speaker",
                    json!({
                        "meetingId": self.active,
                        "speakerId": data["speaker_id"],
                        "name": data["name"],
                        "personId": if person.is_empty() {
                            Value::Null
                        } else {
                            person.into()
                        },
                        "consent": {
                            "method": data["consent"],
                            "note": data["consent_note"]
                        }
                    }),
                    "voice-saved",
                    &self.active,
                );
            }
            "forget-voice" => self.request(
                ui,
                "forget_voice",
                json!({"profileId": data}),
                "voice-forgotten",
                "",
            ),
            "delete-permanently" => self.request(
                ui,
                "api_delete_meeting",
                json!({"meetingId": data}),
                "mutation",
                "",
            ),
            "task" => {
                let mut input = data;
                input["title"] = st.get_modal_value().as_str().into();
                input["owner"] = st.get_modal_secondary().as_str().into();
                input["due_date"] = if st.get_modal_date().trim().is_empty() {
                    Value::Null
                } else {
                    st.get_modal_date().as_str().into()
                };
                self.request(
                    ui,
                    "workspace_save_task",
                    json!({"input": input}),
                    "mutation",
                    "",
                );
            }
            "speech" | "library" | "connections" => {
                let command = match action.as_str() {
                    "speech" => "speech_save_settings",
                    "library" => "library_save_settings",
                    _ => "integrations_save_settings",
                };
                self.request(
                    ui,
                    command,
                    json!({"settings": data}),
                    "settings-saved",
                    &action,
                );
            }
            "create-backup" => {
                if st.get_modal_value().len() < 10 {
                    st.set_error("Use a backup passphrase of at least 10 characters.".into());
                    return;
                }
                self.dialog.1["passphrase"] = st.get_modal_value().as_str().into();
                self.request(
                    ui,
                    "workspace_pick_file",
                    json!({"kind": "backup-save"}),
                    "backup-path",
                    "",
                );
            }
            "restore-backup" => {
                self.dialog.1["passphrase"] = st.get_modal_value().as_str().into();
                self.request(
                    ui,
                    "workspace_pick_file",
                    json!({"kind": "restore-folder"}),
                    "restore-directory",
                    "",
                );
            }
            _ => {}
        }
    }

    pub(super) fn modal_choice(&mut self, ui: &AppWindow, value: &str) {
        let st = ui.global::<State>();
        let (action, data) = self.dialog.clone();
        st.set_modal("".into());
        if action.starts_with("privacy-") {
            self.privacy_choice(ui, &action, value, data);
            return;
        }
        if action.starts_with("hub-") {
            self.hub_modal_choice(ui, &action, data, value);
            return;
        }
        if action.starts_with("automation-") {
            self.automation_choice(ui, &action, data, value);
            return;
        }
        match action.as_str() {
            "forget-voice-choice" => {
                let name = st
                    .get_voice_profiles()
                    .iter()
                    .find(|r| r.id.as_str() == value)
                    .map(|r| r.title.to_string())
                    .unwrap_or_default();
                self.choices(
                    ui,
                    "voice-profile",
                    &name,
                    vec![
                        (
                            "review".into(),
                            "Review or export this person's data".into(),
                        ),
                        ("forget".into(), "Forget this voice".into()),
                    ],
                    json!({"id":value,"name":name}),
                );
            }
            "voice-profile" if value == "review" => {
                st.set_modal("".into());
                self.request(
                    ui,
                    "workspace_privacy",
                    json!({"action":{"op":"subject_preview","name":data["name"]}}),
                    "privacy-subject-preview",
                    "",
                );
            }
            "voice-profile" => {
                self.dialog(ui,"forget-voice","Forget this voice?","Future meetings will no longer recognize this person. Names already saved in meetings stay unchanged.","confirm",data["id"].clone());
                st.set_modal_submit("Forget voice".into());
            }
            "voice-menu" => self.action(ui, value, ""),
            "timeline-notes" => {
                self.select_note(ui, value);
                if st.get_meeting_tab() == 1 {
                    self.action(ui, "show-inspector", "");
                }
            }
            "saved-answer" => {
                if let Some(answer) = self
                    .answers
                    .iter()
                    .find(|answer| text(answer, "id") == value)
                    .cloned()
                {
                    st.set_view(2);
                    st.set_query(text(&answer, "question").into());
                    self.show_answer(ui, &answer);
                }
            }
            "marker" => match value {
                "jump" => self.action(ui, "seek-absolute", &data["at"].to_string()),
                "delete" => self.request(
                    ui,
                    "meeting_delete_mark",
                    json!({"meetingId":self.active,"id":data["id"]}),
                    "mark-deleted",
                    &self.active,
                ),
                _ => {}
            },
            "template" => {
                self.selected_template = value.into();
                st.set_summary_template_id(value.into());
                if let Some(template) = self
                    .settings
                    .get("api_list_templates")
                    .and_then(|v| v.as_array())
                    .and_then(|rows| rows.iter().find(|v| text(v, "id") == value))
                {
                    st.set_selected_template(text(template, "name").into());
                }
            }
            "meeting-options" => match value {
                "rename" => {
                    let title = st.get_title();
                    self.dialog(
                        ui,
                        "rename-meeting",
                        "Rename meeting",
                        "Meeting name",
                        "field",
                        Value::Null,
                    );
                    st.set_modal_value(title);
                }
                "pin" | "archive" | "trash" => {
                    let state = self.organization.get(&self.active);
                    let field = match value {
                        "pin" => "pinned",
                        "archive" => "archived",
                        _ => "trashed",
                    };
                    let mut change = json!({"meeting_id":self.active});
                    change[field] = Value::Bool(if value == "trash" {
                        true
                    } else {
                        !state.and_then(|v| v[field].as_bool()).unwrap_or(false)
                    });
                    self.request(
                        ui,
                        "workspace_organise_meeting",
                        json!({"change":change}),
                        "mutation",
                        "",
                    );
                    if value == "trash" {
                        st.set_view(0);
                    }
                }
                "transcribe" => self.transcribe(ui),
                _ => {}
            },
            "merge-speaker" => {
                self.request(
                    ui,
                    "merge_meeting_speakers",
                    json!({"meetingId":self.active,"sourceId":data,"targetId":value}),
                    "mutation",
                    "",
                );
            }
            "provider" => {
                st.set_provider(value.into());
                st.set_model("".into());
                st.set_api_key("".into());
                st.set_endpoint("".into());
                if value == "privatemode" {
                    st.set_endpoint("http://127.0.0.1:8080/v1".into());
                    st.set_model("glm-5.3".into());
                }
                if value == "overtalk-cloud" {
                    st.set_model("glm-5.3-flash".into());
                }
            }
            "models" => self.model_choice(ui, data.as_str().unwrap_or_default(), value),
            "list-options" => {
                self.list_filter = value.into();
                self.filter_list(ui);
            }
            "export" => {
                let args = json!({"meetingId":self.active,"format":value});
                if self.drafts.unsaved(&self.active).is_some() {
                    self.pending_export = Some(args);
                    self.save_notes(ui);
                } else {
                    self.request(ui, "native_export", args, "exported", "");
                }
            }
            _ => {}
        }
    }

    pub(super) fn transcribe(&self, ui: &AppWindow) {
        if let Some(folder) = self
            .cache
            .get(&self.active)
            .and_then(|m| m.metadata.as_ref())
            .and_then(|v| v["folder_path"].as_str())
        {
            ui.global::<State>().set_transcribing(true);
            self.request(
                ui,
                "start_retranscription_command",
                json!({"meetingId":self.active,"meetingFolderPath":folder}),
                "transcribing",
                "",
            );
        } else {
            ui.global::<State>()
                .set_error("This meeting has no saved audio folder.".into());
        }
    }

    pub(super) fn device_name(&self, ui: &AppWindow, microphone: bool) -> Option<String> {
        let st = ui.global::<State>();
        let index = if microphone {
            st.get_mic_index()
        } else {
            st.get_system_index()
        };
        let values = if microphone {
            &self.mic_ids
        } else {
            &self.system_ids
        };
        values.get(index.max(0) as usize).cloned().flatten()
    }
}

fn field_label(key: &str) -> String {
    match key {
        "name" => "Name",
        "organization" => "Organization",
        "local_only" => "Keep meeting content on this device",
        "protected_mode" => "Require protected storage and approved destinations",
        "require_recording_notice" => "Confirm participant notice before each recording",
        "recording_notice_local" => "Notice when everything stays on this computer",
        "recording_notice_external" => {
            "Notice when a cloud service is used ({services} says what is sent where)"
        }
        "automatic_retention" => "Automatically delete expired meetings",
        "retention_days" => "Meeting retention in days (0 keeps until manually deleted)",
        "diagnostic_retention_days" => "Diagnostic log retention in days",
        "audit_retention_days" => "Security event retention in days",
        "approved_hosts_text" => "Approved service hostnames, separated by commas",
        "recording_basis" => "Documented basis for recording",
        "privacy_contact" => "Privacy contact",
        "agreement_reference" => "Service agreement references",
        "reference" => "Authorization record or document reference",
        "basis" => "Authorization basis",
        "consent" => "How they agreed",
        "consent_note" => "Where their agreement is recorded (optional if they agreed in this meeting)",
        "confirmed" => "They explicitly agreed to Overtalk recognising their voice on this computer",
        "voice_recognition_automatic" => {
            "Recognise remembered voices after each transcription (this also analyses the other speakers' voices)"
        }
        "days" => "Expires after this many days",
        "python" => "Python executable",
        "diarization_python" => "Speaker analysis Python executable",
        "enabled" => "Enabled",
        "maxTokens" => "Maximum output tokens (blank uses the provider default)",
        "temperature" => "Temperature (blank uses the provider default)",
        "topP" => "Top P (blank uses the provider default)",
        "apiKey" => "API key",
        "api_token" => "API access token",
        "cli_token" => "Command line access token",
        "mcp_token" => "MCP access token",
        "webhook_secret" => "Webhook signing secret",
        "device" => "Compute device (auto, cpu, cuda)",
        "api_enabled" => "Local HTTP API",
        "api_port" => "Local API port",
        "cli_enabled" => "Command line access",
        "mcp_enabled" => "MCP access",
        "record_control" => "Allow recording control",
        "write_notes" => "Allow note editing",
        "webhook_enabled" => "Send webhook events",
        "webhook_url" => "Webhook URL",
        "read_meetings" => "Allow reading meetings",
        "read_transcripts" => "Allow reading transcripts",
        "read_summaries" => "Allow reading notes",
        _ => return key.replace('_', " "),
    }
    .into()
}

fn connection_form_rows(value: &Value) -> Vec<FormRow> {
    if value["custom_app"] == true {
        return vec![
            FormRow {
                key: "/name".into(),
                label: "Connection name".into(),
                value: text(value, "name").into(),
                kind: 0,
            },
            FormRow {
                key: "/client_id".into(),
                label: "Public client ID".into(),
                value: text(value, "client_id").into(),
                kind: 0,
            },
        ];
    }
    if value["browser"] == true {
        return vec![FormRow {
            key: "/name".into(),
            label: "Connection name".into(),
            value: text(value, "name").into(),
            kind: 0,
        }];
    }
    let provider = text(value, "provider");
    let fields: &[(&str, &str, i32)] = match provider.as_str() {
        "linear" => &[
            ("name", "Connection name", 0),
            ("api_key", "Personal API key", 2),
        ],
        "google" => &[
            ("name", "Connection name", 0),
            ("client_id", "OAuth client ID", 0),
            ("client_secret", "OAuth client secret (optional)", 2),
        ],
        _ => &[
            ("name", "Connection name", 0),
            ("base_url", "Workspace URL", 0),
            ("username", "Account email", 0),
            ("api_key", "API token", 2),
        ],
    };
    fields
        .iter()
        .map(|(key, label, kind)| FormRow {
            key: format!("/{key}").into(),
            label: (*label).into(),
            value: text(value, key).into(),
            kind: *kind,
        })
        .collect()
}

fn collect_form_rows(value: &Value, prefix: &str, rows: &mut Vec<FormRow>) {
    if let Some(fields) = value.as_object() {
        for (key, value) in fields {
            let path = format!("{prefix}/{}", key.replace('~', "~0").replace('/', "~1"));
            if value.is_object() {
                collect_form_rows(value, &path, rows);
            } else if !value.is_array() {
                let label = path
                    .trim_start_matches('/')
                    .split('/')
                    .map(field_label)
                    .collect::<Vec<_>>()
                    .join(" · ");
                rows.push(FormRow {
                    key: path.into(),
                    label: label.into(),
                    value: value
                        .as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| {
                            if value.is_null() {
                                String::new()
                            } else {
                                value.to_string()
                            }
                        })
                        .into(),
                    kind: if value.is_boolean() {
                        1
                    } else if key.ends_with("_token")
                        || key.ends_with("_secret")
                        || key == "apiKey"
                        || key == "api_key"
                    {
                        2
                    } else {
                        0
                    },
                });
            }
        }
    }
}

#[cfg(test)]
mod connection_form_tests {
    use super::*;

    #[test]
    fn account_signin_has_no_end_user_credentials() {
        for provider in [
            "linear_mcp",
            "atlassian_mcp",
            "google",
            "microsoft",
            "slack",
            "notion_mcp",
            "github",
        ] {
            let rows =
                connection_form_rows(&json!({"provider":provider,"name":"Team","browser":true}));
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].key, "/name");
        }
    }
    #[test]
    fn connection_forms_expose_only_the_fields_the_provider_needs() {
        let data = json!({"provider":"linear","name":"Design team","api_key":"test-token","username":"","base_url":"https://api.linear.app"});
        let rows = connection_form_rows(&data);
        assert_eq!(
            rows.iter().map(|r| r.key.as_str()).collect::<Vec<_>>(),
            ["/name", "/api_key"]
        );
        assert_eq!(rows[1].kind, 2);
        assert_eq!(rows[1].value, "test-token");
        let rows = connection_form_rows(&json!({"provider":"jira"}));
        assert_eq!(
            rows.iter().map(|r| r.key.as_str()).collect::<Vec<_>>(),
            ["/name", "/base_url", "/username", "/api_key"]
        );
        let rows = connection_form_rows(&json!({"provider":"google"}));
        assert_eq!(
            rows.iter().map(|r| r.key.as_str()).collect::<Vec<_>>(),
            ["/name", "/client_id", "/client_secret"]
        );
        assert_eq!(rows[2].kind, 2);
    }
}
