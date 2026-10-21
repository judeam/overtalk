use super::*;
impl Controller {
    pub(super) fn cancel_account_signin(&mut self, ui: &AppWindow) {
        let st = ui.global::<State>();
        let attempt = st.get_connection_attempt().to_string();
        if !attempt.is_empty() {
            st.set_connection_attempt("".into());
            self.request(
                ui,
                "workspace_account_auth",
                json!({"action":{"op":"cancel","attempt":attempt}}),
                "ignored",
                "",
            );
        }
        st.set_connection_busy(false);
    }

    fn render_connection_choices(&self, ui: &AppWindow, query: &str) {
        let query = query.to_lowercase();
        let mut rows = [
            ("google", "Google Workspace"),
            ("microsoft", "Microsoft 365"),
            ("slack", "Slack"),
            ("notion_mcp", "Notion"),
            ("linear_mcp", "Linear"),
            ("atlassian_mcp", "Jira & Confluence"),
            ("github", "GitHub"),
        ];
        rows.sort_by_key(|(id, _)| !self.account_ready(id));
        ui.global::<State>().set_modal_choices(model(
            rows.iter()
                .filter(|(_, name)| name.to_lowercase().contains(&query))
                .map(|(id, name)| DetailRow {
                    id: (*id).into(),
                    title: (*name).into(),
                    value: if self.account_ready(id) {
                        "Ready".into()
                    } else {
                        "App setup needed".into()
                    },
                    ..Default::default()
                })
                .collect(),
        ));
    }
    fn account_ready(&self, provider: &str) -> bool {
        matches!(provider, "linear_mcp" | "atlassian_mcp" | "notion_mcp")
            || self.settings.get("account_auth").is_some_and(|v| {
                array(&v["providers"])
                    .iter()
                    .any(|p| p["id"] == provider && p["configured"] == true)
            })
    }
    pub(super) fn hub_extra_action(&mut self, ui: &AppWindow, action: &str, value: &str) {
        let st = ui.global::<State>();
        match action {
            "hub-share-notes" => {
                if st.get_notes().trim().is_empty() {
                    st.set_error("Generate or write notes before sending them.".into());
                } else {
                    self.request(
                        ui,
                        "workspace_connections",
                        json!({"action": {"op": "list"}}),
                        "hub-share-accounts",
                        "",
                    );
                }
            }
            "hub-workspace-tools" => self.choices(
                ui,
                "hub-workspace-tool",
                "Workspace tools",
                vec![
                    ("hub-reminders".into(), "Reminders".into()),
                    ("hub-backups".into(), "Backups".into()),
                    ("hub-connections".into(), "Connections".into()),
                ],
                Value::Null,
            ),
            "hub-reminders" => self.hub_action(ui, "hub-tab", "6"),
            "hub-backups" => self.hub_action(ui, "hub-tab", "7"),
            "hub-connections" => self.hub_action(ui, "hub-tab", "8"),
            "hub-meeting-tools" => {
                let mut rows = vec![
                    ("hub-history".into(), "Notes history".into()),
                    ("hub-learn".into(), "Learn project context".into()),
                    ("hub-redact".into(), "Export a redacted document".into()),
                    (
                        "hub-redact-audio".into(),
                        "Export audio with muted passages".into(),
                    ),
                    ("hub-assign".into(), "Assign project".into()),
                    ("share-meeting".into(), "Share a copy…".into()),
                ];
                if st.get_has_video() {
                    rows.push(("hub-video-open".into(), "Open saved video".into()));
                }
                self.choices(ui, "hub-meeting-tool", "Meeting tools", rows, Value::Null);
            }
            "hub-redact-audio" => {
                self.dialog(
                    ui,
                    "hub-redact-audio-save",
                    "Mute passages in an audio copy",
                    "Time ranges, one per line, e.g. 00:12–00:24. The original stays unchanged.",
                    "edit",
                    Value::Null,
                );
                st.set_modal_value(
                    format!(
                        "{}–{}",
                        time(self.position, false),
                        time(self.position + 10., false)
                    )
                    .into(),
                );
            }
            "hub-series" => {
                let item = self
                    .hub_rows
                    .iter()
                    .find(|r| text(r, "id") == value)
                    .cloned()
                    .unwrap_or(Value::Null);
                self.dialog(
                    ui,
                    "hub-series-save",
                    "Recurring meeting series",
                    "A series groups recurring meetings within the project.",
                    "field",
                    item.clone(),
                );
                st.set_modal_value(text(&item, "series").into());
            }
            "hub-add-fact" => self.dialog(
                ui,
                "hub-fact-save",
                "Add project knowledge",
                "Write a confirmed project fact. Inferred facts from meetings are reviewed \
                 separately.",
                "edit",
                Value::Null,
            ),
            "hub-correct" => {
                let issue = self
                    .hub_rows
                    .iter()
                    .find(|r| text(r, "id") == value)
                    .cloned()
                    .unwrap_or(Value::Null);
                if text(&issue, "kind").starts_with("notes_") {
                    self.open(ui, &text(&issue, "meeting_id"));
                    self.action(ui, "meeting-tab", "1");
                } else {
                    self.open(ui, &text(&issue, "meeting_id"));
                    self.choices(
                        ui,
                        "hub-review-kind",
                        "Correct this passage",
                        vec![
                            ("words".into(), "Correct the words".into()),
                            ("speaker".into(), "Assign a speaker".into()),
                        ],
                        issue,
                    );
                }
            }
            "hub-video-toggle" => {
                if st.get_video_recording() || st.get_video_starting() {
                    self.request(
                        ui,
                        "workspace_video",
                        json!({"action": {"op": "stop"}}),
                        "hub-video-stopped",
                        "",
                    );
                } else if st.get_recording() {
                    self.hub_action(ui, "hub-video-start", "");
                } else {
                    st.set_video_enabled(!st.get_video_enabled());
                }
            }
            "hub-video-start" => {
                st.set_video_starting(true);
                self.request(
                    ui,
                    "workspace_video",
                    json!({"action": {"op": "targets"}}),
                    "hub-video-targets",
                    "",
                );
            }
            "hub-video-open" => self.request(
                ui,
                "workspace_video",
                json!({"action": {"op": "open", "meeting_id": self.active}}),
                "ignored",
                "",
            ),
            "hub-calendar-sync" => self.request(
                ui,
                "workspace_connections",
                json!({"action": {"op": "list"}}),
                "hub-calendar-accounts",
                "",
            ),
            "hub-calendar-details" => {
                if let Some(event) = self
                    .results
                    .iter()
                    .find(|e| text(e, "id") == value)
                    .cloned()
                {
                    let people: Value =
                        serde_json::from_str(&text(&event, "participants")).unwrap_or(Value::Null);
                    let names = array(&people)
                        .iter()
                        .map(|p| {
                            let name = text(p, "name");
                            format!(
                                "{} · {}",
                                if name.is_empty() {
                                    text(p, "email")
                                } else {
                                    name
                                },
                                text(p, "status")
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    self.dialog(
                        ui,
                        "diagnostics",
                        &text(&event, "title"),
                        "Calendar event",
                        "read",
                        Value::Null,
                    );
                    st.set_modal_value(
                        format!(
                            "{}\n\n{}\n\nParticipants\n{}",
                            text(&event, "starts_at"),
                            text(&event, "description"),
                            names
                        )
                        .into(),
                    );
                    st.set_modal_submit("Close".into());
                }
            }
            "hub-watch-toggle" => self.request(
                ui,
                "workspace_meeting_watch",
                json!({
                    "settings": {
                        "enabled": !st.get_watch_enabled(),
                        "show_widget": !st.get_watch_enabled()
                    }
                }),
                "hub-watch-status",
                "",
            ),
            "hub-widget-join" => self.request(
                ui,
                "open_external_url",
                json!({"url": st.get_widget_url().as_str()}),
                "ignored",
                "",
            ),
            "hub-task-reminder" => {
                let task = self
                    .results
                    .iter()
                    .find(|v| result_id(v) == value)
                    .cloned()
                    .unwrap_or(Value::Null);
                self.dialog(
                    ui,
                    "hub-remind",
                    "Set a reminder",
                    "Local date and time: YYYY-MM-DD HH:MM. Delivered while Overtalk is running.",
                    "field",
                    task,
                );
                st.set_modal_value(
                    (chrono::Local::now() + chrono::Duration::hours(1))
                        .format("%Y-%m-%d %H:%M")
                        .to_string()
                        .into(),
                );
            }
            "hub-reading-options" => self.choices(
                ui,
                "hub-reading-mode",
                "Transcript reading",
                vec![
                    ("original".into(), "Original script".into()),
                    ("latin".into(), "Romanize Devanagari".into()),
                    ("translate".into(), "Translate to another language".into()),
                    (
                        "cached".into(),
                        "Show saved translation beside original".into(),
                    ),
                ],
                Value::Null,
            ),
            "hub-job-options" => self.choices(
                ui,
                "hub-job-kind",
                "Queue work for the open meeting",
                vec![
                    ("notes".into(), "Generate notes".into()),
                    ("transcribe".into(), "Transcribe & identify speakers".into()),
                    ("learn".into(), "Propose project context".into()),
                    ("translate".into(), "Translate transcript".into()),
                    ("pause".into(), "Pause queued work".into()),
                    ("resume".into(), "Resume queued work".into()),
                    ("low-memory".into(), "Low memory mode".into()),
                    ("balanced".into(), "Balanced performance".into()),
                ],
                Value::Null,
            ),
            "hub-backup-options" => self.choices(
                ui,
                "hub-backup-kind",
                "Backup & recovery",
                vec![
                    ("key".into(), "Create a recovery key".into()),
                    ("configure".into(), "Schedule backups".into()),
                    ("disable".into(), "Disable scheduled backups".into()),
                    ("restore".into(), "Restore a backup".into()),
                ],
                Value::Null,
            ),
            "hub-connection-options" => {
                self.cancel_account_signin(ui);
                self.request(
                    ui,
                    "workspace_account_auth",
                    json!({"action": {"op": "capabilities"}}),
                    "hub-auth-capabilities",
                    "",
                );
                self.choices(
                    ui,
                    "hub-connection-kind",
                    "Connect your tools",
                    vec![
                        ("linear_mcp".into(), "Linear".into()),
                        ("atlassian_mcp".into(), "Jira & Confluence".into()),
                        ("google".into(), "Google Workspace".into()),
                    ],
                    Value::Null,
                );
                st.set_connection_filter("".into());
                self.render_connection_choices(ui, "");
                st.set_modal("connection-picker".into());
            }
            "hub-connection-search" => {
                st.set_connection_filter(value.into());
                self.render_connection_choices(ui, value);
            }
            "hub-connection-code-copy" => {
                ui.invoke_copy_text(st.get_connection_code());
                st.set_message("Sign-in code copied".into());
            }
            "hub-connection-advanced" => {
                self.cancel_account_signin(ui);
                self.choices(
                    ui,
                    "hub-connection-kind",
                    "Advanced connections",
                    vec![
                        ("linear".into(), "Linear · API key".into()),
                        ("jira".into(), "Jira · API token".into()),
                        ("confluence".into(), "Confluence · API token".into()),
                        ("google-custom".into(), "Google · custom OAuth app".into()),
                        (
                            "microsoft-custom".into(),
                            "Microsoft · public client ID".into(),
                        ),
                        ("slack-custom".into(), "Slack · public client ID".into()),
                        ("github-custom".into(), "GitHub · public client ID".into()),
                    ],
                    json!({"advanced": true}),
                );
                st.set_modal("connection-picker".into());
            }
            "hub-connection-help" => {
                let url = match st.get_connection_provider().as_str() {
                    "linear" => "https://linear.app/settings/api",
                    "jira" | "confluence" => {
                        "https://id.atlassian.com/manage-profile/security/api-tokens"
                    }
                    "microsoft" => ENTRA_APP_REGISTRATIONS_URL,
                    "slack" => "https://api.slack.com/apps",
                    "github" => "https://github.com/settings/apps/new",
                    _ => "https://console.cloud.google.com/apis/credentials",
                };
                self.request(ui, "open_external_url", json!({"url":url}), "ignored", "");
            }
            "hub-connection-edit" => {
                let row = self
                    .hub_rows
                    .iter()
                    .find(|r| text(r, "id") == value)
                    .cloned()
                    .unwrap_or(Value::Null);
                let mut choices = vec![
                    ("browse".into(), "Browse destinations".into()),
                    ("read".into(), "Read a ticket or document".into()),
                    ("draft".into(), "Prepare a new ticket or document".into()),
                    ("update".into(), "Prepare an update".into()),
                ];
                if matches!(
                    text(&row, "description").as_str(),
                    "linear_mcp"
                        | "atlassian_mcp"
                        | "notion_mcp"
                        | "microsoft"
                        | "slack"
                        | "github"
                        | "google"
                ) {
                    choices.push(("reconnect".into(), "Sign in again".into()));
                }
                choices.push((
                    "disconnect".into(),
                    "Disconnect and remove credentials".into(),
                ));
                self.choices(
                    ui,
                    "hub-connection-work",
                    "Connection actions",
                    choices,
                    json!({
                        "id": value,
                        "provider": row["description"],
                        "name": row["name"]
                    }),
                );
            }
            "hub-reading-original" => {
                st.set_reading_mode(0);
                st.set_latin_devanagari(false);
                self.translations.clear();
                for m in self.cache.values_mut() {
                    m.rows = None;
                }
                self.render_meeting(ui);
            }
            "hub-reading-latin" => {
                st.set_reading_mode(1);
                st.set_latin_devanagari(true);
                self.translations.clear();
                for m in self.cache.values_mut() {
                    m.rows = None;
                }
                self.render_meeting(ui);
            }
            "hub-reading-translated" => self.request(
                ui,
                "workspace_reading",
                json!({
                    "meetingId": self.active,
                    "language": st.get_reading_language().as_str()
                }),
                "hub-reading-data",
                &self.active,
            ),
            "hub-redact" => {
                self.dialog(
                    ui,
                    "hub-redact-config",
                    "Redacted export",
                    "One literal name or phrase per line. Only the exported copy changes.",
                    "edit",
                    Value::Null,
                );
                st.set_modal_secondary_label("Format: pdf, docx, markdown, text, json".into());
                st.set_modal_secondary("pdf".into());
            }
            _ => {}
        }
    }
    pub(super) fn hub_extra_reply(
        &mut self,
        ui: &AppWindow,
        tag: &str,
        context: &str,
        value: Value,
    ) {
        let st = ui.global::<State>();
        match tag {
            "hub-share-accounts" => {
                if array(&value).is_empty() {
                    st.set_error(
                        "Add a connection in Projects → Connections, then send your notes.".into(),
                    );
                } else {
                    self.choices(
                        ui,
                        "hub-share-account",
                        "Send notes to…",
                        array(&value)
                            .iter()
                            .map(|r| (text(r, "id"), text(r, "name")))
                            .collect(),
                        value,
                    );
                }
            }
            "hub-share-destinations" => {
                st.set_message("".into());
                self.choices(
                    ui,
                    "hub-remote-destination",
                    "Choose destination",
                    array(&value)
                        .iter()
                        .map(|r| (text(r, "id"), text(r, "name")))
                        .collect(),
                    json!({
                        "connection_id": context,
                        "operation": "draft",
                        "destinations": value
                    }),
                );
            }
            "hub-review-voices" => self.choices(
                ui,
                "hub-review-speaker",
                "Who said this?",
                array(&value)
                    .iter()
                    .map(|s| (text(s, "id"), text(s, "name")))
                    .collect(),
                serde_json::from_str(context).unwrap_or(Value::Null),
            ),
            "hub-video-presence" => {
                if context == self.active {
                    st.set_has_video(value["has_video"] == true);
                }
            }
            "hub-video-targets" => {
                let mut rows = array(&value)
                    .iter()
                    .map(|v| (text(v, "id"), text(v, "title")))
                    .collect::<Vec<_>>();
                if rows.is_empty() {
                    rows.push(("system-picker".into(), "Choose a window or screen".into()));
                }
                self.choices(
                    ui,
                    "hub-video-target",
                    "Select video source",
                    rows,
                    Value::Null,
                );
            }
            "hub-video-stopped" => {
                st.set_video_recording(false);
                st.set_video_starting(false);
                if !context.is_empty() {
                    self.request(
                        ui,
                        "workspace_video",
                        json!({"action": {"op": "attach", "meeting_id": context}}),
                        "hub-video-attached",
                        "",
                    );
                }
            }
            "hub-video-attached" => {
                if value["attached"].as_u64().unwrap_or(0) > 0 {
                    st.set_message(
                        if value["silent"].as_u64().unwrap_or(0) > 0 {
                            "Video saved without embedded audio. The original audio remains in the \
                             meeting."
                        } else {
                            "Meeting video saved with the recording."
                        }
                        .into(),
                    );
                }
            }
            "hub-calendar-accounts" => {
                let rows = array(&value)
                    .iter()
                    .filter(|c| c["description"] == "google" || c["description"] == "microsoft")
                    .map(|c| (text(c, "id"), text(c, "name")))
                    .collect::<Vec<_>>();
                if rows.is_empty() {
                    self.choices(
                        ui,
                        "hub-workspace-tool",
                        "Connect a calendar",
                        vec![(
                            "hub-connections".into(),
                            "Connect Google or Microsoft".into(),
                        )],
                        Value::Null,
                    );
                } else {
                    self.choices(
                        ui,
                        "hub-calendar-account",
                        "Sync calendar",
                        rows,
                        Value::Null,
                    );
                }
            }
            "hub-calendar-synced" => {
                self.request(ui, "workspace_calendar_events", json!({}), "calendar", "");
                st.set_message(format!("Synced {} calendar events", value).into());
            }
            "hub-watch-status" => {
                st.set_watch_enabled(value["enabled"] == true);
                if !st.get_watch_enabled() {
                    st.set_widget_visible(false);
                }
            }
            "hub-restore-revision" => self.request(
                ui,
                "workspace_review",
                json!({
                    "action": {
                        "op": "restore",
                        "meeting_id": self.active,
                        "version_id": serde_json::from_str::<Value>(context)
                            .unwrap_or(Value::Null),
                        "expected_hash": value
                    }
                }),
                "hub-restored",
                "",
            ),
            "hub-restored" => {
                st.set_modal("".into());
                self.reload_active(ui);
                st.set_message("Notes restored. The previous version remains in history.".into());
            }
            "hub-backups" => {
                self.hub_options = value.clone();
                st.set_hub_rows(model(vec![DetailRow {
                    id: "schedule".into(),
                    title: if value["enabled"] == true {
                        "Scheduled backups enabled".into()
                    } else {
                        "Scheduled backups disabled".into()
                    },
                    description: if value["last_error"].is_string() {
                        format!("Backup failed: {}", text(&value, "last_error")).into()
                    } else {
                        text(&value, "directory").into()
                    },
                    value: format!(
                        "Every {} hours · Last success: {}",
                        value["interval_hours"],
                        if value["last_success"].is_string() {
                            text(&value, "last_success")
                        } else {
                            "Not yet".into()
                        }
                    )
                    .into(),
                    ..Default::default()
                }]));
            }
            "hub-backup-key" => {
                if !value.is_null() {
                    self.hub_options["recipient"] = value["recipient"].clone();
                    st.set_message(
                        "Recovery key saved. Keep it separate from your backup folder.".into(),
                    );
                    self.hub_action(ui, "hub-backup-options", "");
                }
            }
            "hub-backup-folder" => {
                if let Some(path) = value.as_str() {
                    self.hub_options["directory"] = json!(path);
                    self.form(
                        ui,
                        "hub-backup-config",
                        "Schedule encrypted backups",
                        json!({"interval_hours": 24, "include_audio": false}),
                    );
                }
            }
            "hub-restore-file" => {
                if let Some(path) = value.as_str() {
                    self.dialog(
                        ui,
                        "hub-restore-password",
                        "Restore encrypted backup",
                        "Enter its passphrase or paste your recovery key. Your current workspace \
                         is preserved before activation.",
                        "password",
                        json!({"path": path}),
                    );
                }
            }
            "hub-restore-ready" => {
                st.set_modal("".into());
                st.set_message(text(&value, "message").into());
            }
            "hub-auth-capabilities" => {
                st.set_connection_google_ready(value["google_configured"] == true);
                self.settings.insert("account_auth".into(), value);
                if st.get_modal() == "connection-picker"
                    && st.get_modal_title() != "Advanced connections"
                {
                    self.render_connection_choices(ui, st.get_connection_filter().as_str());
                }
                if st.get_connection_browser() && self.dialog.1["custom_app"] != true {
                    st.set_connection_ready(
                        self.account_ready(st.get_connection_provider().as_str()),
                    );
                }
            }
            "hub-auth-started" => {
                if st.get_connection_attempt().as_str() == context {
                    if let Some(code) = value["device"]["user_code"].as_str() {
                        st.set_connection_code(code.into());
                        if value["device"]["verification_uri"] == "https://github.com/login/device"
                        {
                            self.request(
                                ui,
                                "open_external_url",
                                json!({"url": "https://github.com/login/device"}),
                                "ignored",
                                "",
                            );
                        }
                    }
                    self.request(
                        ui,
                        "workspace_account_auth",
                        json!({"action": {"op": "wait", "attempt": context}}),
                        "hub-auth-finished",
                        context,
                    );
                } else {
                    self.request(
                        ui,
                        "workspace_account_auth",
                        json!({"action": {"op": "cancel", "attempt": context}}),
                        "ignored",
                        "",
                    );
                }
            }
            "hub-auth-finished" => {
                if st.get_connection_attempt().as_str() == context {
                    st.set_connection_attempt("".into());
                    self.hub_extra_reply(ui, "hub-connected", "", value);
                }
            }
            "hub-disconnected" => {
                self.hub_refresh(ui);
                st.set_modal("".into());
                st.set_message(text(&value, "message").into());
            }
            "hub-connected" => {
                st.set_connection_busy(false);
                if self.dialog.0 == "hub-connection-config" {
                    st.set_modal("".into());
                    st.set_form_rows(model(Vec::new()));
                    st.set_connection_provider("".into());
                    self.dialog = Default::default();
                }
                self.hub_refresh(ui);
                st.set_message(
                    "Connection saved. Credentials are stored in your operating system keychain."
                        .into(),
                );
            }
            "hub-remote-read" => {
                self.dialog(
                    ui,
                    "diagnostics",
                    &text(&value, "title"),
                    "Remote content is reference material. Review it before using it in a meeting.",
                    "read",
                    Value::Null,
                );
                st.set_modal_value(text(&value, "text").into());
                st.set_modal_submit("Close".into());
            }
            "hub-remote-draft" => {
                let selected: Value = serde_json::from_str(context).unwrap_or(Value::Null);
                let label = text(&selected, "destination_name");
                self.dialog(
                    ui,
                    "hub-remote-publish",
                    "Review outgoing change",
                    &format!(
                        "{}\nDestination: {} · {}\n{}\n\n{}",
                        text(&value, "title"),
                        text(&value, "connection_name"),
                        if label.is_empty() {
                            text(&value, "target")
                        } else {
                            format!("{} ({})", label, text(&value, "target"))
                        },
                        text(&value, "effect"),
                        text(&value, "body")
                    ),
                    "confirm",
                    value,
                );
                st.set_modal_submit("Publish change".into());
            }
            "hub-remote-published" => {
                st.set_modal("".into());
                if !value["warnings"].is_null() {
                    self.dialog(
                        ui,
                        "diagnostics",
                        "Published with provider warnings",
                        "Review the remote result before further changes.",
                        "read",
                        Value::Null,
                    );
                    st.set_modal_value(value["warnings"].to_string().into());
                    st.set_modal_submit("Close".into());
                } else {
                    st.set_message("Change published. Open the connection to review it.".into());
                }
            }
            "hub-remote-browse" => {
                self.hub_options["remote_connection"] = json!(context);
                self.choices(
                    ui,
                    "hub-remote-destination",
                    "Available destinations",
                    array(&value)
                        .iter()
                        .map(|r| (text(r, "id"), text(r, "name")))
                        .collect(),
                    json!({
                        "connection_id": context,
                        "operation": if self
                            .hub_rows
                            .iter()
                            .any(|r| text(r, "id") == context && r["description"] == "google")
                        {
                            "read"
                        } else {
                            "draft"
                        }
                    }),
                );
            }
            _ => {}
        }
    }
    pub(super) fn hub_extra_save(&mut self, ui: &AppWindow, action: &str, data: Value) {
        let st = ui.global::<State>();
        match action {
            "hub-fact-save" => self.request(
                ui,
                "workspace_project",
                json!({
                    "action": {
                        "op": "fact",
                        "id": null,
                        "project_id": st.get_project_id().as_str(),
                        "body": st.get_modal_value().as_str()
                    }
                }),
                "hub-mutated",
                "",
            ),
            "hub-series-save" => self.request(
                ui,
                "workspace_project",
                json!({
                    "action": {
                        "op": "assign",
                        "meeting_id": data["id"],
                        "project_id": st.get_project_id().as_str(),
                        "series": st.get_modal_value().as_str()
                    }
                }),
                "hub-mutated",
                "",
            ),
            "hub-review-words" => self.request(
                ui,
                "workspace_correct_transcript",
                json!({
                    "change": {
                        "meeting_id": data["meeting_id"],
                        "segment_id": data["segment_id"],
                        "text": st.get_modal_value().as_str(),
                        "speaker_id": data["speaker_id"],
                        "expected_text": data["text"],
                        "expected_speaker_id": data["speaker_id"]
                    }
                }),
                "mutation",
                "",
            ),
            "hub-redact-audio-save" => {
                let spans = match super::share::ranges(&st.get_modal_value()) {
                    Ok(spans) => spans,
                    Err(error) => {
                        st.set_error(error.into());
                        return;
                    }
                };
                self.request(
                    ui,
                    "workspace_redacted_audio",
                    json!({"meetingId": self.active, "spans": spans}),
                    "exported",
                    "",
                );
                st.set_modal("".into());
            }
            "hub-backup-config" => {
                let mut settings = self.hub_options.clone();
                settings["enabled"] = json!(true);
                settings["interval_hours"] = data["interval_hours"].clone();
                settings["include_audio"] = data["include_audio"].clone();
                self.request(
                    ui,
                    "workspace_backup_settings",
                    json!({"settings": settings}),
                    "hub-backups",
                    "",
                );
                st.set_modal("".into());
            }
            "hub-restore-password" => self.request(
                ui,
                "workspace_schedule_restore",
                json!({
                    "path": data["path"],
                    "passphrase": st.get_modal_value().as_str()
                }),
                "hub-restore-ready",
                "",
            ),
            "hub-redact-config" => {
                let format = st.get_modal_secondary().to_string();
                let terms = st
                    .get_modal_value()
                    .lines()
                    .filter(|s| !s.trim().is_empty())
                    .map(str::to_owned)
                    .collect::<Vec<_>>();
                self.request(
                    ui,
                    "native_export",
                    json!({
                        "meetingId": self.active,
                        "format": format,
                        "redactions": {
                            "terms": terms,
                            "contacts": true,
                            "omit_notes": false
                        }
                    }),
                    "exported",
                    "",
                );
                st.set_modal("".into());
            }
            "hub-connection-config" => {
                if st.get_connection_busy() {
                    return;
                }
                let provider = text(&data, "provider");
                if data["browser"] == true {
                    if text(&data, "name").trim().is_empty() {
                        st.set_error("Give this connection a name.".into());
                        return;
                    }
                    let attempt = format!(
                        "{:032x}",
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_nanos()
                    );
                    st.set_connection_attempt(attempt.clone().into());
                    st.set_connection_busy(true);
                    st.set_error("".into());
                    self.request(
                        ui,
                        "workspace_account_auth",
                        json!({
                            "action": {
                                "op": "start",
                                "attempt": attempt,
                                "provider": provider,
                                "name": data["name"],
                                "connection_id": data["connection_id"],
                                "client_id": data["client_id"].as_str().unwrap_or(""),
                                "project_id": if st.get_project_id().is_empty() {
                                    None
                                } else {
                                    Some(st.get_project_id().to_string())
                                }
                            }
                        }),
                        "hub-auth-started",
                        &attempt,
                    );
                    return;
                }
                let required = if provider == "google" {
                    vec![("client_id", "Enter your Google Desktop OAuth client ID.")]
                } else if provider == "linear" {
                    vec![("api_key", "Enter your Linear API key.")]
                } else {
                    vec![
                        ("base_url", "Enter your Atlassian workspace URL."),
                        ("username", "Enter your Atlassian account email."),
                        ("api_key", "Enter your Atlassian API token."),
                    ]
                };
                for (field, message) in required {
                    if text(&data, field).trim().is_empty() {
                        st.set_error(message.into());
                        return;
                    }
                }
                if text(&data, "name").trim().is_empty() {
                    st.set_error("Give this connection a name.".into());
                    return;
                }
                st.set_error("".into());
                st.set_connection_busy(true);
                if provider == "google" {
                    let attempt = format!(
                        "{:032x}",
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_nanos()
                    );
                    st.set_connection_attempt(attempt.clone().into());
                    self.request(
                        ui,
                        "workspace_account_auth",
                        json!({
                            "action": {
                                "op": "start",
                                "attempt": attempt,
                                "provider": "google",
                                "name": data["name"],
                                "client_id": data["client_id"],
                                "client_secret": data["client_secret"]
                            }
                        }),
                        "hub-auth-started",
                        &attempt,
                    );
                } else {
                    self.request(
                        ui,
                        "workspace_connections",
                        json!({
                            "action": {
                                "op": "save",
                                "provider": provider,
                                "name": data["name"],
                                "base_url": data["base_url"],
                                "username": data["username"],
                                "secret": data["api_key"],
                                "project_id": if st.get_project_id().is_empty() {
                                    None
                                } else {
                                    Some(st.get_project_id().to_string())
                                }
                            }
                        }),
                        "hub-connected",
                        "",
                    );
                }
            }
            "hub-remote-target" => {
                let mut data = data;
                data["target"] = json!(st.get_modal_value().as_str());
                if data["operation"] == "read" {
                    self.request(
                        ui,
                        "workspace_connections",
                        json!({
                            "action": {
                                "op": "read",
                                "id": data["id"],
                                "target": data["target"]
                            }
                        }),
                        "hub-remote-read",
                        "",
                    );
                } else {
                    self.dialog(
                        ui,
                        "hub-remote-content",
                        "Prepare change",
                        "Review the content before publishing. No remote write happens until you \
                         approve the next screen.",
                        "edit",
                        data,
                    );
                    st.set_modal_value(st.get_notes());
                    st.set_modal_secondary_label("Title".into());
                    st.set_modal_secondary(st.get_title());
                }
            }
            "hub-remote-content" => self.request(
                ui,
                "workspace_connections",
                json!({
                    "action": {
                        "op": "draft",
                        "source_meeting_id": if self.active.is_empty() {
                            Value::Null
                        } else {
                            json!(self.active)
                        },
                        "id": data["id"],
                        "target": data["target"],
                        "update": data["operation"] == "update",
                        "title": st.get_modal_secondary().as_str(),
                        "body": st.get_modal_value().as_str()
                    }
                }),
                "hub-remote-draft",
                &data.to_string(),
            ),
            "hub-remote-publish" => self.request(
                ui,
                "workspace_connections",
                json!({
                    "action": {
                        "op": "publish",
                        "id": data["id"],
                        "expected_hash": data["hash"]
                    }
                }),
                "hub-remote-published",
                "",
            ),
            _ => {}
        }
    }
    pub(super) fn hub_extra_choice(
        &mut self,
        ui: &AppWindow,
        action: &str,
        data: Value,
        value: &str,
    ) {
        let st = ui.global::<State>();
        match action {
            "hub-share-account" => {
                if array(&data)
                    .iter()
                    .any(|c| c["id"] == value && c["description"] == "google")
                {
                    self.dialog(
                        ui,
                        "hub-remote-content",
                        "Prepare Google document",
                        "Review these notes before publishing.",
                        "edit",
                        json!({"id": value, "target": "", "operation": "draft"}),
                    );
                    st.set_modal_value(st.get_notes());
                    st.set_modal_secondary_label("Title".into());
                    st.set_modal_secondary(st.get_title());
                } else {
                    st.set_message("Loading destinations…".into());
                    self.request(
                        ui,
                        "workspace_connections",
                        json!({"action": {"op": "browse", "id": value}}),
                        "hub-share-destinations",
                        value,
                    );
                }
            }
            "hub-meeting-tool" | "hub-workspace-tool" => self.action(ui, value, ""),
            "hub-review-kind" => {
                if value == "speaker" {
                    self.request(
                        ui,
                        "list_meeting_speakers",
                        json!({"meetingId": data["meeting_id"]}),
                        "hub-review-voices",
                        &data.to_string(),
                    );
                } else {
                    self.dialog(
                        ui,
                        "hub-review-words",
                        "Correct transcript",
                        "Listen to the source before saving. The previous transcript is retained \
                         for recovery.",
                        "edit",
                        data.clone(),
                    );
                    st.set_modal_value(text(&data, "text").into());
                }
            }
            "hub-review-speaker" => self.request(
                ui,
                "workspace_correct_transcript",
                json!({
                    "change": {
                        "meeting_id": data["meeting_id"],
                        "segment_id": data["segment_id"],
                        "text": data["text"],
                        "speaker_id": value,
                        "expected_text": data["text"],
                        "expected_speaker_id": data["speaker_id"]
                    }
                }),
                "mutation",
                "",
            ),
            "hub-video-target" => self.request(
                ui,
                "workspace_video",
                json!({"action": {"op": "start", "target": value}}),
                "hub-video-started",
                "",
            ),
            "hub-calendar-account" => self.request(
                ui,
                "workspace_sync_calendar",
                json!({"id": value}),
                "hub-calendar-synced",
                "",
            ),
            "hub-reading-mode" => self.hub_action(
                ui,
                match value {
                    "original" => "hub-reading-original",
                    "latin" => "hub-reading-latin",
                    "cached" => "hub-reading-translated",
                    _ => "hub-reading",
                },
                "",
            ),
            "hub-job-kind" => {
                if value == "low-memory" || value == "balanced" {
                    self.request(
                        ui,
                        "workspace_jobs",
                        json!({
                            "action": {
                                "op": "resources",
                                "low_resource": value == "low-memory"
                            }
                        }),
                        "hub-mutated",
                        "",
                    );
                } else if value == "translate" {
                    self.hub_action(ui, "hub-reading", "");
                } else if value == "pause" || value == "resume" {
                    self.request(
                        ui,
                        "workspace_jobs",
                        json!({
                            "action": {"op": "pause", "paused": value == "pause"}
                        }),
                        "hub-mutated",
                        "",
                    );
                } else {
                    self.request(
                        ui,
                        "workspace_jobs",
                        json!({
                            "action": {
                                "op": "submit",
                                "kind": value,
                                "meeting_id": self.active
                            }
                        }),
                        "hub-mutated",
                        "",
                    );
                }
            }
            "hub-backup-kind" => match value {
                "key" => self.request(ui, "workspace_backup_key", json!({}), "hub-backup-key", ""),
                "configure" => self.request(
                    ui,
                    "workspace_pick_file",
                    json!({"kind": "restore-folder"}),
                    "hub-backup-folder",
                    "",
                ),
                "disable" => {
                    let mut settings = self.hub_options.clone();
                    settings["enabled"] = json!(false);
                    self.request(
                        ui,
                        "workspace_backup_settings",
                        json!({"settings": settings}),
                        "hub-backups",
                        "",
                    );
                }
                "restore" => self.request(
                    ui,
                    "workspace_pick_file",
                    json!({"kind": "backup-open"}),
                    "hub-restore-file",
                    "",
                ),
                _ => {}
            },
            "hub-connection-kind" => {
                let custom = value.ends_with("-custom");
                let provider = value.strip_suffix("-custom").unwrap_or(value);
                if !matches!(
                    provider,
                    "linear"
                        | "linear_mcp"
                        | "atlassian_mcp"
                        | "jira"
                        | "confluence"
                        | "google"
                        | "notion_mcp"
                        | "microsoft"
                        | "slack"
                        | "github"
                ) {
                    return;
                }
                let browser = !(matches!(provider, "linear" | "jira" | "confluence")
                    || custom && provider == "google");
                let name = connection_name(provider);
                let config = if custom && provider != "google" {
                    json!({
                        "provider": provider,
                        "name": name,
                        "browser": true,
                        "custom_app": true,
                        "client_id": ""
                    })
                } else if browser {
                    json!({"provider":provider,"name":name,"browser":true})
                } else if provider == "google" {
                    json!({
                        "provider": provider,
                        "name": name,
                        "client_id": "",
                        "client_secret": ""
                    })
                } else {
                    json!({
                        "provider": provider,
                        "name": name,
                        "base_url": if provider == "linear" {
                            "https://api.linear.app"
                        } else {
                            ""
                        },
                        "username": "",
                        "api_key": ""
                    })
                };
                self.form(
                    ui,
                    "hub-connection-config",
                    &format!("Connect {name}"),
                    config,
                );
                st.set_connection_provider(provider.into());
                st.set_connection_browser(browser);
                st.set_connection_ready(custom || self.account_ready(provider));
                st.set_modal("connection-setup".into());
                st.set_modal_submit(if browser {
                    format!(
                        "Continue with {}",
                        if provider == "atlassian_mcp" {
                            "Atlassian"
                        } else {
                            name
                        }
                    )
                    .into()
                } else if provider == "google" {
                    "Continue with Google".into()
                } else {
                    format!("Connect {name}").into()
                });
            }
            "hub-connection-work" => {
                if value == "reconnect" {
                    let provider = text(&data, "provider");
                    self.form(
                        ui,
                        "hub-connection-config",
                        "Sign in again",
                        json!({
                            "provider": provider,
                            "name": data["name"],
                            "browser": true,
                            "connection_id": data["id"]
                        }),
                    );
                    st.set_connection_provider(provider.into());
                    st.set_connection_browser(true);
                    st.set_connection_ready(true);
                    st.set_connection_google_ready(true);
                    st.set_modal("connection-setup".into());
                    st.set_modal_submit("Sign in again".into());
                } else if value == "disconnect" {
                    self.request(
                        ui,
                        "workspace_connections",
                        json!({"action": {"op": "remove", "id": data["id"]}}),
                        "hub-disconnected",
                        "",
                    );
                } else if value == "browse" {
                    self.request(
                        ui,
                        "workspace_connections",
                        json!({"action": {"op": "browse", "id": data["id"]}}),
                        "hub-remote-browse",
                        data["id"].as_str().unwrap_or(""),
                    );
                } else if value == "draft" {
                    if data["provider"] == "google" {
                        self.dialog(
                            ui,
                            "hub-remote-content",
                            "Prepare Google document",
                            "Review these notes before publishing.",
                            "edit",
                            json!({"id": data["id"], "target": "", "operation": "draft"}),
                        );
                        st.set_modal_value(st.get_notes());
                        st.set_modal_secondary_label("Title".into());
                        st.set_modal_secondary(st.get_title());
                    } else {
                        st.set_message("Loading destinations…".into());
                        self.request(
                            ui,
                            "workspace_connections",
                            json!({"action": {"op": "browse", "id": data["id"]}}),
                            "hub-share-destinations",
                            text(&data, "id").as_str(),
                        );
                    }
                } else {
                    let hint = match text(&data, "provider").as_str() {
                        "github" => {
                            "Paste the GitHub issue link. Pull-request links can also be read."
                        }
                        "slack" => {
                            "Paste a Slack message link. You can update messages sent by your \
                             connected account."
                        }
                        "microsoft" => {
                            "Paste a OneDrive or SharePoint link to a Word, Markdown or text \
                             document."
                        }
                        "notion_mcp" => "Paste a Notion page link.",
                        "google" => "Enter the Google document ID from its link.",
                        "linear" | "linear_mcp" => "Enter the issue identifier, such as ENG-123.",
                        _ => {
                            "Enter the ticket or page ID. Atlassian accounts use \
                             jira:site-id:issue-key or confluence:site-id:page-id; Browse shows \
                             your site's destinations."
                        }
                    };
                    self.dialog(
                        ui,
                        "hub-remote-target",
                        if value == "read" {
                            "Open remote content"
                        } else {
                            "Update remote content"
                        },
                        hint,
                        "field",
                        json!({"id": data["id"], "operation": value}),
                    );
                }
            }
            "hub-remote-destination" => {
                let destination_name = array(&data["destinations"])
                    .iter()
                    .find(|r| r["id"] == value)
                    .map(|r| text(r, "name"))
                    .unwrap_or_default();
                if data["operation"] == "read" {
                    self.request(
                        ui,
                        "workspace_connections",
                        json!({
                            "action": {
                                "op": "read",
                                "id": data["connection_id"],
                                "target": value
                            }
                        }),
                        "hub-remote-read",
                        "",
                    );
                } else {
                    self.dialog(
                        ui,
                        "hub-remote-content",
                        "Prepare outgoing notes",
                        "Review the title and content. Nothing is published until you approve the \
                         next screen.",
                        "edit",
                        json!({
                            "id": data["connection_id"],
                            "target": value,
                            "operation": "draft",
                            "destination_name": destination_name
                        }),
                    );
                    st.set_modal_value(st.get_notes());
                    st.set_modal_secondary_label("Title".into());
                    st.set_modal_secondary(st.get_title());
                }
            }
            _ => {
                st.set_modal("".into());
            }
        }
    }
}

fn connection_name(provider: &str) -> &str {
    match provider {
        "linear" | "linear_mcp" => "Linear",
        "atlassian_mcp" => "Jira & Confluence",
        "jira" => "Jira",
        "confluence" => "Confluence",
        "google" => "Google",
        "microsoft" => "Microsoft 365",
        "slack" => "Slack",
        "notion_mcp" => "Notion",
        "github" => "GitHub",
        _ => "Account",
    }
}

const ENTRA_APP_REGISTRATIONS_URL: &str =
    "https://entra.microsoft.com/#view/Microsoft_AAD_RegisteredApps/ApplicationsListBlade";
