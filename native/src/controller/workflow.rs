use super::*;

#[derive(Default)]
pub(super) struct Workflow {
    pub drafts: crate::drafts::Drafts,
    revisions: HashMap<String, i64>,
    key: String,
    capture: String,
    event: String,
    pub events: Vec<Value>,
    timer: slint::Timer,
    /// A calendar change arrived while the dashboard was already loading.
    reload_pending: bool,
    answer: Value,
    question: String,
    serial: u64,
    mic_peak: f32,
    system_peak: f32,
}

impl Controller {
    pub(super) fn workflow_record_levels(&mut self, ui: &AppWindow, value: &Value) {
        self.workflow.mic_peak = self
            .workflow
            .mic_peak
            .max(value["microphone"].as_f64().unwrap_or(0.) as f32);
        self.workflow.system_peak = self
            .workflow
            .system_peak
            .max(value["system"].as_f64().unwrap_or(0.) as f32);
        ui.global::<State>().set_setup_audio_detail(
            format!(
                "Microphone: {} · call audio: {}. Listen to the saved check before confirming.",
                if self.workflow.mic_peak > 0.002 {
                    "signal received"
                } else {
                    "no signal yet"
                },
                if self.workflow.system_peak > 0.002 {
                    "signal received"
                } else {
                    "play some call audio"
                }
            )
            .into(),
        );
    }
    pub(super) fn workflow_initialize(&mut self, ui: &AppWindow) {
        if self.workflow.key.is_empty() {
            self.workflow_open(ui, "scratchpad");
        }
        // The engine syncs calendars on its own schedule and announces changes.
        self.workflow_calendar(ui, false);
    }
    pub(super) fn workflow_load(&self, ui: &AppWindow, id: &str) {
        self.request(
            ui,
            "workspace_workflow",
            json!({"action":{"op":"load","id":id}}),
            "workflow-pad",
            id,
        );
    }
    pub(super) fn workflow_open(&mut self, ui: &AppWindow, id: &str) {
        self.workflow_save_all(ui);
        if !self.workflow.question.is_empty() {
            self.request(
                ui,
                "workspace_cancel_question",
                json!({"jobId":self.workflow.question}),
                "ignored",
                "",
            );
            self.workflow.question.clear();
        }
        ui.global::<State>().set_assist_busy(false);
        ui.global::<State>().set_assist_status("".into());
        self.workflow.key = id.into();
        let st = ui.global::<State>();
        st.set_personal_notes(self.workflow.drafts.text(id).unwrap_or("").into());
        st.set_completion_status("".into());
        st.set_completion_retry(false);
        st.set_followup_draft("".into());
        st.set_followup_ready(false);
        st.set_assist_answer("".into());
        st.set_assist_sources(model(Vec::new()));
        self.workflow_load(ui, id);
        if id != "scratchpad" {
            self.workflow_load(ui, &format!("followup:{id}"));
        }
    }
    pub(super) fn workflow_save_all(&mut self, ui: &AppWindow) {
        for id in self.workflow.drafts.dirty_ids() {
            if let Some((revision, body)) = self.workflow.drafts.begin_save(&id) {
                let stored = *self.workflow.revisions.get(&id).unwrap_or(&0);
                self.request(
                    ui,
                    "workspace_workflow",
                    json!({"action":{"op":"save","id":id,"body":body,"revision":stored}}),
                    "workflow-saved",
                    &json!({"id":id,"revision":revision}).to_string(),
                );
            }
        }
    }
    fn workflow_edit(&mut self, ui: &AppWindow, id: &str, body: &str) {
        self.workflow.drafts.edit(id, body.into());
        ui.global::<State>().set_personal_status("Saving…".into());
        let weak = ui.as_weak();
        self.workflow.timer.start(
            slint::TimerMode::SingleShot,
            Duration::from_millis(450),
            move || {
                if let Some(ui) = weak.upgrade() {
                    with_controller(|c| c.workflow_save_all(&ui));
                }
            },
        );
    }
    fn workflow_calendar(&self, ui: &AppWindow, refresh: bool) {
        if ui.global::<State>().get_calendar_syncing() {
            return;
        }
        ui.global::<State>().set_calendar_syncing(true);
        self.request(
            ui,
            "workspace_workflow",
            json!({"action":{"op":"dashboard","refresh":refresh}}),
            "workflow-calendar",
            if refresh { "refresh" } else { "cache" },
        );
    }
    /// Show calendar changes the engine already synced, without syncing again.
    pub(super) fn workflow_calendar_reload(&mut self, ui: &AppWindow) {
        if ui.global::<State>().get_calendar_syncing() {
            self.workflow.reload_pending = true;
            return;
        }
        ui.global::<State>().set_calendar_syncing(true);
        self.request(
            ui,
            "workspace_workflow",
            json!({"action":{"op":"dashboard","refresh":false}}),
            "workflow-calendar",
            "changed",
        );
    }
    pub(super) fn workflow_capture_options(&self, ui: &AppWindow, sound_check: bool) -> Value {
        let st = ui.global::<State>();
        let mut options = json!({
            "auto_notes": !sound_check && st.get_auto_summary(),
            "event_id": if self.workflow.event.is_empty() {
                None
            } else {
                Some(self.workflow.event.clone())
            },
            "link_event": !sound_check,
            "template_id": self.selected_template,
            "language": st.get_default_summary_language().as_str(),
            "personal_notes": if sound_check || self.workflow.key != "scratchpad" {
                String::new()
            } else {
                st.get_personal_notes().to_string()
            }
        });
        self.automation_capture_options(&mut options, sound_check);
        options
    }
    pub(super) fn workflow_record_started(&mut self, ui: &AppWindow, value: &Value) {
        let id = text(value, "recording_id");
        self.workflow.mic_peak = 0.;
        self.workflow.system_peak = 0.;
        if id.is_empty() {
            return;
        }
        let body = if self.sound_check_active || self.workflow.key != "scratchpad" {
            String::new()
        } else {
            ui.global::<State>().get_personal_notes().to_string()
        };
        self.workflow.capture = id.clone();
        self.workflow.key = id.clone();
        self.workflow
            .revisions
            .insert(id, if body.is_empty() { 0 } else { 1 });
        if !self.sound_check_active && !body.is_empty() {
            // The service has durably copied this draft into the capture ID.
            self.workflow_edit(ui, "scratchpad", "");
        }

        ui.global::<State>().set_personal_notes(body.into());
        self.workflow.event.clear();
    }
    pub(super) fn workflow_action(&mut self, ui: &AppWindow, action: &str, value: &str) {
        let st = ui.global::<State>();
        match action {
            "workflow-refresh" => self.workflow_calendar(ui, true),
            "workflow-home" => {
                let key = if st.get_recording() {
                    self.workflow.capture.clone()
                } else {
                    "scratchpad".into()
                };
                self.workflow_open(ui, &key);
                self.workflow_calendar(ui, true);
            }
            "workflow-personal" => {
                let id = self.workflow.key.clone();
                self.workflow_edit(ui, &id, value);
            }
            "workflow-followup-edit" => {
                st.set_followup_ready(!value.trim().is_empty());
                let id = format!("followup:{}", self.active);
                self.workflow_edit(ui, &id, value);
            }
            "workflow-retry-save" => self.workflow_save_all(ui),
            "workflow-retry" => self.request(
                ui,
                "workspace_workflow",
                json!({"action":{"op":"retry","meeting_id":self.active}}),
                "workflow-pad",
                &self.active,
            ),
            "workflow-check" => {
                if st.get_setup_checking() {
                    return;
                }
                st.set_setup_checking(true);
                self.request(
                    ui,
                    "workspace_workflow",
                    json!({"action":{"op":"readiness"}}),
                    "workflow-readiness",
                    "",
                );
            }
            "workflow-confirm-audio" => {
                st.set_setup_audio_confirmed(true);
                st.set_message(
                    "Sound check confirmed. Your audio choices are kept for the next call.".into(),
                );
            }
            "workflow-download" => {
                st.set_setup_checking(true);
                self.request(
                    ui,
                    "workspace_workflow",
                    json!({"action":{"op":"first_run_downloads"}}),
                    "workflow-downloads",
                    "",
                );
            }
            "workflow-finish" => {
                // Models may still be downloading: a recording made meanwhile is kept
                // and transcribed when the speech model is ready.
                let speech = st.get_setup_speech_ready() || st.get_setup_speech_downloading();
                let notes = st.get_setup_notes_ready() || st.get_setup_notes_downloading();
                if !speech || !notes || !st.get_setup_audio_confirmed() {
                    st.set_error(
                        "Download or check both models and confirm your sound check before \
                         finishing setup."
                            .into(),
                    );
                    return;
                }
                let state = |ready: bool| if ready { "downloaded" } else { "downloading" };
                self.request(
                    ui,
                    "save_onboarding_status_cmd",
                    json!({
                        "status": {
                            "version": "2.0",
                            "completed": true,
                            "current_step": 4,
                            "model_status": {
                                "parakeet": state(st.get_setup_speech_ready()),
                                "summary": state(st.get_setup_notes_ready())
                            },
                            "last_updated": chrono::Utc::now().to_rfc3339()
                        }
                    }),
                    "setup-finished",
                    "",
                );
            }
            "workflow-later" => st.set_setup_needed(false),
            "workflow-join" => {
                if st.get_recording() || st.get_busy() {
                    return;
                }
                if let Some(event) = self
                    .workflow
                    .events
                    .iter()
                    .find(|e| e["id"] == value)
                    .cloned()
                {
                    st.set_busy(true);
                    self.workflow.event = value.into();
                    st.set_recording_title(text(&event, "title").into());
                    self.request(
                        ui,
                        "workspace_join_event",
                        json!({"id":value}),
                        "workflow-joined",
                        value,
                    );
                }
            }
            "workflow-attendees" => {
                if let Some(event) = self
                    .workflow
                    .events
                    .iter()
                    .find(|e| e["id"] == value)
                    .cloned()
                {
                    let people: Value =
                        serde_json::from_str(event["participants"].as_str().unwrap_or("[]"))
                            .unwrap_or_default();
                    let body = array(&people)
                        .iter()
                        .map(|p| {
                            let n = text(p, "name");
                            let email = text(p, "email");
                            if n.is_empty() {
                                email
                            } else if email.is_empty() {
                                n
                            } else {
                                format!("{n} · {email}")
                            }
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    self.dialog(
                        ui,
                        "diagnostics",
                        &text(&event, "title"),
                        if body.is_empty() {
                            "This calendar did not include a participant list."
                        } else {
                            &body
                        },
                        "confirm",
                        Value::Null,
                    );
                    st.set_modal_submit("Close".into());
                }
            }
            "workflow-panel" => {
                let tab = value.parse().unwrap_or(2);
                let open = !st.get_companion_open() || st.get_companion_tab() != tab;
                st.set_companion_tab(tab);
                st.set_companion_open(open);
                st.set_notes_highlights_open(true);
            }
            "workflow-ask" | "workflow-followup" => {
                if self.active.is_empty() || st.get_assist_busy() {
                    return;
                }
                let followup = action == "workflow-followup";
                let query = if followup {
                    "List the confirmed decisions and agreed action items for a meeting follow-up. \
                     Include owners and deadlines only when explicitly stated, distinguish \
                     proposals and unresolved questions, and exclude retracted commitments."
                } else {
                    value.trim()
                };
                if query.is_empty() {
                    return;
                }
                self.workflow.serial += 1;
                self.workflow.question = format!(
                    "native-assist-{}-{}",
                    std::process::id(),
                    self.workflow.serial
                );
                st.set_assist_busy(true);
                st.set_assist_status(
                    if followup {
                        "Preparing a source-backed follow-up…"
                    } else {
                        "Checking this meeting’s sources…"
                    }
                    .into(),
                );
                self.request(
                    ui,
                    "workspace_ask",
                    json!({
                        "request": {
                            "query": query,
                            "meeting_id": self.active,
                            "limit": 12
                        },
                        "jobId": self.workflow.question
                    }),
                    if followup {
                        "workflow-followup"
                    } else {
                        "workflow-answer"
                    },
                    &json!({"meeting": self.active, "job": self.workflow.question}).to_string(),
                );
            }
            "workflow-cancel-ask" => {
                self.request(
                    ui,
                    "workspace_cancel_question",
                    json!({"jobId":self.workflow.question}),
                    "ignored",
                    "",
                );
            }
            "workflow-copy-followup" => {
                ui.invoke_copy_text(st.get_followup_draft());
                st.set_message("Follow-up copied. Review recipients before sending.".into());
            }
            "workflow-source" => {
                if let Some(source) = array(&self.workflow.answer["sources"])
                    .iter()
                    .find(|s| s["id"] == value)
                {
                    st.set_inspector_title(format!("Source · {}", text(source, "title")).into());
                    st.set_inspector_body(text(source, "body").into());
                    st.set_inspector_position(source["start_time"].as_f64().unwrap_or(0.) as f32);
                    self.action(ui, "show-inspector", "");
                }
            }
            _ => {}
        }
    }
    pub(super) fn workflow_reply(
        &mut self,
        ui: &AppWindow,
        tag: &str,
        context: &str,
        result: Result<Value, String>,
    ) {
        let st = ui.global::<State>();
        if matches!(tag, "workflow-answer" | "workflow-followup") {
            let request: Value = serde_json::from_str(context).unwrap_or_default();
            if request["meeting"] != self.active || request["job"] != self.workflow.question {
                return;
            }
        }
        let value = match result {
            Ok(value) => value,
            Err(error) => {
                match tag {
                    "workflow-calendar" => {
                        st.set_calendar_syncing(false);
                        st.set_calendar_status(format!("Showing saved events. {error}").into());
                        if std::mem::take(&mut self.workflow.reload_pending) {
                            self.workflow_calendar_reload(ui);
                        }
                    }
                    "workflow-readiness" => {
                        st.set_setup_checking(false);
                        st.set_setup_speech_detail(error.clone().into());
                    }
                    "workflow-downloads" => {
                        st.set_setup_checking(false);
                        st.set_error(error.clone().into());
                    }
                    "workflow-joined" => {
                        st.set_busy(false);
                        self.workflow.event.clear();
                    }
                    "workflow-answer" | "workflow-followup" => {
                        st.set_assist_busy(false);
                        st.set_assist_status(error.clone().into());
                    }
                    "workflow-saved" => {
                        let saved: Value = serde_json::from_str(context).unwrap_or_default();
                        self.workflow.drafts.finish_save(
                            &text(&saved, "id"),
                            saved["revision"].as_u64().unwrap_or(0),
                            false,
                        );
                        self.closing = false;
                        st.set_personal_status(format!("Not saved · {error}").into());
                    }
                    _ => {}
                }
                st.set_error(error.into());
                return;
            }
        };
        match tag {
            "workflow-calendar" => {
                st.set_calendar_syncing(false);
                self.workflow.events = array(&value["events"]);
                let accounts = array(&value["accounts"]);
                st.set_calendar_connected(!accounts.is_empty());
                let errors = array(&value["sync"])
                    .iter()
                    .filter(|s| s["error"].is_string())
                    .count();
                st.set_calendar_status(if accounts.is_empty() {
                    "Connect Google or Microsoft for your upcoming meetings.".into()
                } else if errors > 0 {
                    format!("Saved events · {errors} account(s) need reconnection in Connections")
                        .into()
                } else if context == "refresh" || context == "changed" {
                    "Up to date · next 7 days".into()
                } else {
                    "Saved events · refreshing…".into()
                });
                st.set_upcoming_meetings(model(
                    self.workflow
                        .events
                        .iter()
                        .take(3)
                        .map(|e| {
                            let when = chrono::DateTime::parse_from_rfc3339(&text(e, "starts_at"))
                                .ok()
                                .map(|d| {
                                    d.with_timezone(&chrono::Local)
                                        .format("%a %-d %b · %H:%M")
                                        .to_string()
                                })
                                .unwrap_or_default();
                            let people: Value =
                                serde_json::from_str(e["participants"].as_str().unwrap_or("[]"))
                                    .unwrap_or_default();
                            // The person's choice, an automatic one, or a suggestion.
                            let project = &e["project"];
                            let name = clip(&text(project, "name"), 32);
                            let (label, state) = match text(project, "origin").as_str() {
                                _ if name.is_empty() => (String::new(), ""),
                                "automatic" => (format!("{name} · automatic"), "automatic"),
                                "suggested" => (format!("{name}?"), "suggested"),
                                _ => (name, "manual"),
                            };
                            EventCard {
                                id: text(e, "id").into(),
                                title: text(e, "title").into(),
                                description: format!(
                                    "{when} · {} · {} invited",
                                    if text(e, "source").starts_with("microsoft:") {
                                        "Microsoft"
                                    } else if text(e, "source").starts_with("google:") {
                                        "Google"
                                    } else {
                                        "Calendar"
                                    },
                                    array(&people).len()
                                )
                                .into(),
                                provider: if text(e, "source").starts_with("microsoft:") {
                                    "microsoft"
                                } else if text(e, "source").starts_with("google:") {
                                    "google"
                                } else {
                                    "calendar"
                                }
                                .into(),
                                joinable: e["meeting_url"].as_str().is_some_and(|s| !s.is_empty()),
                                project: label.into(),
                                project_state: state.into(),
                                reason: if state.is_empty() {
                                    String::new()
                                } else {
                                    text(project, "reason")
                                }
                                .into(),
                                brief: text(e, "brief").into(),
                            }
                        })
                        .collect(),
                ));
                if context == "cache" && !accounts.is_empty() {
                    self.workflow_calendar(ui, true);
                } else if std::mem::take(&mut self.workflow.reload_pending) {
                    self.workflow_calendar_reload(ui);
                }
            }
            "workflow-pad" => {
                let revision = value["revision"].as_i64().unwrap_or(0);
                self.workflow
                    .revisions
                    .entry(context.into())
                    .and_modify(|r| *r = (*r).max(revision))
                    .or_insert(revision);
                let body = self
                    .workflow
                    .drafts
                    .unsaved(context)
                    .map(str::to_owned)
                    .unwrap_or_else(|| text(&value, "body"));
                self.workflow.drafts.clear_saved(context);
                if context == self.workflow.key {
                    st.set_personal_notes(body.clone().into());
                    st.set_personal_status("Saved on this device".into());
                }
                if context == format!("followup:{}", self.active) {
                    st.set_followup_ready(!body.is_empty());
                    st.set_followup_draft(body.into());
                }
                if context == self.active {
                    let jobs = array(&value["jobs"]);
                    let failed = jobs.iter().find(|j| {
                        matches!(
                            j["state"].as_str(),
                            Some("failed" | "interrupted" | "cancelled")
                        )
                    });
                    let current = failed.or_else(|| {
                        jobs.iter()
                            .find(|j| matches!(j["state"].as_str(), Some("running" | "queued")))
                    });
                    st.set_completion_retry(failed.is_some());
                    st.set_completion_status(
                        current
                            .map(|j| text(j, "detail"))
                            .unwrap_or_else(|| {
                                if jobs.is_empty() {
                                    String::new()
                                } else {
                                    "Transcript and notes are ready".into()
                                }
                            })
                            .into(),
                    );
                }
            }
            "workflow-saved" => {
                let saved: Value = serde_json::from_str(context).unwrap_or_default();
                let id = text(&saved, "id");
                self.workflow
                    .revisions
                    .insert(id.clone(), value["revision"].as_i64().unwrap_or(0));
                self.workflow.drafts.finish_save(
                    &id,
                    saved["revision"].as_u64().unwrap_or(0),
                    true,
                );
                if self.workflow.drafts.unsaved(&id).is_some() {
                    self.workflow_save_all(ui);
                } else {
                    st.set_personal_status("Saved on this device".into());
                }
                if self.closing
                    && self.workflow.drafts.dirty_ids().is_empty()
                    && self.drafts.dirty_ids().is_empty()
                {
                    let _ = slint::quit_event_loop();
                }
            }
            "workflow-downloads" => {
                st.set_setup_checking(false);
                let downloading = |part: &str| value[part]["state"] == "downloading";
                let size = |part: &str| {
                    value[part]["size_mb"]
                        .as_f64()
                        .map(|mb| {
                            if mb >= 1000.0 {
                                format!(" · {:.1} GB", mb / 1000.0)
                            } else {
                                format!(" · {mb:.0} MB")
                            }
                        })
                        .unwrap_or_default()
                };
                if downloading("speech") {
                    st.set_setup_speech_downloading(true);
                    st.set_setup_speech_detail(
                        format!("Downloading {}{}", label(&value["speech"]), size("speech")).into(),
                    );
                }
                if downloading("notes") {
                    st.set_setup_notes_downloading(true);
                    st.set_setup_notes_detail(
                        format!("Downloading {}{}", label(&value["notes"]), size("notes")).into(),
                    );
                }
                st.set_message(
                    if downloading("speech") || downloading("notes") {
                        "Downloading. You can record now; the transcript follows when the speech model is ready."
                    } else {
                        "Nothing needed downloading. Check readiness to confirm."
                    }
                    .into(),
                );
            }
            "workflow-readiness" => {
                st.set_setup_checking(false);
                let speech = value["speech"]["ready"] == true;
                st.set_setup_speech_ready(speech);
                st.set_setup_notes_ready(value["notes_configured"] == true);
                st.set_setup_speech_downloading(!speech && value["speech_progress"].is_u64());
                st.set_setup_notes_downloading(
                    value["notes_configured"] != true && value["notes_progress"].is_u64(),
                );
                st.set_setup_speech_detail(if speech {
                    "Speech runtime and model files are ready".into()
                } else if let Some(progress) = value["speech_progress"].as_u64() {
                    // Progress events refine this; the model list itself may report 0.
                    if progress == 0 {
                        "Downloading the speech model…".into()
                    } else {
                        format!("Downloading the speech model · {progress}%").into()
                    }
                } else {
                    value["speech_error"]
                        .as_str()
                        .unwrap_or(
                            "Speech model or runtime is missing. Open speech settings to fix it.",
                        )
                        .into()
                });
                st.set_setup_notes_detail(if value["notes_ready"] == true {
                    format!("{} · downloaded and ready", label(&value)).into()
                } else if let Some(progress) = value["notes_progress"].as_u64() {
                    format!("Downloading {} · {progress}%", label(&value)).into()
                } else if value["remote"] == true {
                    format!(
                        "{} · provider configured; checked on first use",
                        text(&value, "model")
                    )
                    .into()
                } else {
                    "Notes model missing · choose and download one in Settings".into()
                });
            }
            "workflow-joined" => {
                st.set_busy(false);
                self.action(ui, "home", "");
                self.action(ui, "start-recording", "");
            }
            "workflow-answer" | "workflow-followup" => {
                st.set_assist_busy(false);
                self.workflow.question.clear();
                let claims = array(&value["claims"]);
                let body = claims
                    .iter()
                    .map(|c| format!("• {}", text(c, "text")))
                    .collect::<Vec<_>>()
                    .join("\n\n");
                st.set_assist_status(if claims.is_empty(){"The available sources do not support an answer. Try a more specific question."}else{"Grounded in this meeting · review sources below"}.into());
                st.set_assist_answer(body.clone().into());
                st.set_assist_sources(model(
                    array(&value["sources"])
                        .iter()
                        .enumerate()
                        .map(|(i, s)| DetailRow {
                            id: text(s, "id").into(),
                            title: format!(
                                "Source {} · {}",
                                i + 1,
                                time(s["start_time"].as_f64().unwrap_or(0.), false)
                            )
                            .into(),
                            description: text(s, "body")
                                .chars()
                                .take(140)
                                .collect::<String>()
                                .into(),
                            ..Default::default()
                        })
                        .collect(),
                ));
                self.workflow.answer = value;
                if tag == "workflow-followup" && !body.is_empty() {
                    let draft=format!("Meeting follow-up: {}\n\nThanks for the discussion. Here are the confirmed outcomes and next steps:\n\n{body}\n\nPlease reply with any corrections.",st.get_title());
                    st.set_followup_draft(draft.clone().into());
                    st.set_followup_ready(true);
                    self.workflow_edit(ui, &format!("followup:{}", self.active), &draft);
                }
            }
            _ => {}
        }
    }
}

/// A model's readable name from the engine, or its id from an older engine.
fn label(value: &Value) -> String {
    ["model_label", "label", "model"]
        .iter()
        .map(|key| text(value, key))
        .find(|name| !name.is_empty())
        .unwrap_or_default()
}
