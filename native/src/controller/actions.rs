use super::*;

impl Controller {
    pub(crate) fn action(&mut self, ui: &AppWindow, action: &str, value: &str) {
        if action.starts_with("privacy-") {
            self.privacy_action(ui, action, value);
            return;
        }
        if action.starts_with("workflow-") {
            self.workflow_action(ui, action, value);
            return;
        }
        if action == "guide" || action.starts_with("guide-") {
            self.guide_action(ui, action, value);
            return;
        }
        if action == "hub" || action.starts_with("hub-") {
            self.hub_action(ui, action, value);
            return;
        }
        if action.starts_with("template-") {
            self.template_action(ui, action, value);
            return;
        }
        if action.starts_with("automation-") {
            self.automation_action(ui, action, value);
            return;
        }
        if action.starts_with("share-") {
            self.share_action(ui, action, value);
            return;
        }
        if action.starts_with("assistant-") {
            self.assistant_action(ui, action, value);
            return;
        }
        let st = ui.global::<State>();
        match action {
            "toggle-script" => {
                st.set_latin_devanagari(!st.get_latin_devanagari());
                for meeting in self.cache.values_mut() {
                    meeting.rows = None;
                }
                self.render_meeting(ui);
                if !st.get_query().is_empty() {
                    self.action(ui, "transcript-query", st.get_query().as_str());
                }
                st.invoke_preferences_changed();
            }
            "refresh-meeting" => self.reload_active(ui),
            "form-copy" => {
                if let Some(secret) = self.dialog.1.pointer(value).and_then(Value::as_str) {
                    ui.invoke_copy_text(secret.into());
                    st.set_message("Copied. Keep access tokens private.".into());
                }
            }
            "form-reset" => {
                if let Some(secret) = self.dialog.1.pointer_mut(value) {
                    *secret = Value::String(String::new());
                }
                self.render_form(ui);
            }

            "quit" => {
                if self.request_close(ui) {
                    let _ = slint::quit_event_loop();
                } else {
                    let _ = ui.show();
                }
            }
            "solo-speaker" => {
                self.solo_speaker = if self.solo_speaker.as_deref() == Some(value) {
                    None
                } else {
                    Some(value.into())
                };
                self.muted_speakers.remove(value);
                self.update_speaker_filter(ui);
            }
            "mute-speaker" => {
                if !self.muted_speakers.remove(value) {
                    self.muted_speakers.insert(value.into());
                }
                self.update_speaker_filter(ui);
            }
            "select-note" => self.select_note(ui, value),
            "browse-note-highlights" => {
                let rows = st
                    .get_notes_highlights()
                    .iter()
                    .map(|note| {
                        (
                            note.id.to_string(),
                            format!("{} · {}", note.stamp, note.body),
                        )
                    })
                    .collect();
                self.choices(
                    ui,
                    "timeline-notes",
                    "Decisions & actions",
                    rows,
                    Value::Null,
                );
            }
            "select-note-group" => {
                let members: Vec<String> = serde_json::from_str(value).unwrap_or_default();
                if members.len() == 1 {
                    self.select_note(ui, &members[0]);
                } else if let Some(meeting) = self.cache.get(&self.active) {
                    let notes = meeting
                        .summary
                        .as_ref()
                        .zip(meeting.timeline.as_ref())
                        .map(|(summary, timeline)| {
                            crate::timeline::note_clips(
                                &summary_markdown(summary),
                                &timeline["sources"],
                            )
                        })
                        .unwrap_or_default();
                    let rows = notes
                        .into_iter()
                        .filter(|n| members.contains(&n.id))
                        .map(|n| (n.id, format!("{} · {}", time(n.start, false), n.body)))
                        .collect();
                    self.choices(
                        ui,
                        "timeline-notes",
                        "Moments close together",
                        rows,
                        Value::Null,
                    );
                }
            }
            "add-mark" => {
                self.dialog(
                    ui,
                    "add-mark",
                    "Mark this moment",
                    "Give this bookmark a short label.",
                    "field",
                    json!({"at": self.position}),
                );
                st.set_modal_value("Bookmark".into());
            }
            "marker-options" => {
                let mark = self
                    .cache
                    .get(&self.active)
                    .and_then(|meeting| meeting.marks.iter().find(|row| row["id"] == value))
                    .cloned();
                if let Some(mark) = mark {
                    self.choices(
                        ui,
                        "marker",
                        &text(&mark, "label"),
                        vec![
                            ("jump".into(), "Jump to this moment".into()),
                            ("delete".into(), "Remove bookmark".into()),
                        ],
                        mark,
                    );
                }
            }
            "open-note-link" => {
                if let Some(index) = value
                    .strip_prefix("#source-")
                    .and_then(|s| s.parse::<usize>().ok())
                    .and_then(|n| n.checked_sub(1))
                {
                    let at = self
                        .cache
                        .get(&self.active)
                        .and_then(|m| m.timeline.as_ref())
                        .and_then(|t| t["sources"].get(index))
                        .and_then(|span| span[0].as_f64());
                    if let Some(at) = at {
                        self.action(ui, "seek-absolute", &at.to_string());
                        self.action(ui, "meeting-tab", "0");
                    } else {
                        st.set_error(
                            "This source is no longer available in the transcript.".into(),
                        );
                    }
                } else if value.starts_with("https://") || value.starts_with("http://") {
                    self.request(
                        ui,
                        "open_external_url",
                        json!({"url": value}),
                        "ignored",
                        "",
                    );
                }
            }
            "copy-selection" => {
                ui.invoke_copy_text(st.get_inspector_body());
                st.set_message("Passage copied".into());
            }
            "select-result" => self.select_result(ui, value),
            "copy-passage" => {
                ui.invoke_copy_text(st.get_preview_body());
                st.set_message("Passage copied".into());
            }
            "saved-answers" => self.request(ui, "workspace_answers", json!({}), "answers", ""),
            "copy-answer" => {
                ui.invoke_copy_text(st.get_answer_text());
                st.set_message("Answer copied".into());
            }
            "delete-answer" => self.request(
                ui,
                "workspace_delete_answer",
                json!({"id": value}),
                "answer-deleted",
                "",
            ),
            "answer-source" => {
                if let Some(id) = value.strip_prefix("#answer-source-") {
                    self.select_result(ui, id);
                }
            }
            "row-options" => {
                self.open(ui, value);
                self.action(ui, "meeting-options", "");
            }
            "rename-direct" => {
                self.request(
                    ui,
                    "api_save_meeting_title",
                    json!({"meetingId":self.active,"title":value}),
                    "mutation",
                    "",
                );
            }
            "playback-rate" => {
                let index = value.parse::<usize>().unwrap_or(1).min(5);
                let rate = [0.75, 1.0, 1.25, 1.5, 1.75, 2.0][index];
                self.request(
                    ui,
                    "native_audio",
                    json!({"action":"speed","value":rate}),
                    "audio",
                    "",
                );
            }
            "audio-sources" => self.dialog(
                ui,
                "audio-sources",
                "Audio sources",
                "Choose the microphone and call audio to record.",
                "audio",
                Value::Null,
            ),
            "show-inspector" => {
                let body = st.get_inspector_body();
                self.dialog(
                    ui,
                    "diagnostics",
                    "Selected moment",
                    &body,
                    "confirm",
                    Value::Null,
                );
                st.set_modal_submit("Close".into());
            }
            "meeting-details" => {
                let notice = st.get_meeting_notice();
                let body = format!(
                    "Recorded {}  ·  length {}\n{}\n\n{}",
                    st.get_meeting_date(),
                    st.get_meeting_duration(),
                    st.get_meeting_people(),
                    if notice.is_empty() {
                        "No recording notice was saved for this meeting."
                    } else {
                        notice.as_str()
                    }
                );
                // Read-only, with one Close button, like a calendar event's details.
                self.dialog(
                    ui,
                    "diagnostics",
                    "Meeting details",
                    &st.get_title(),
                    "read",
                    Value::Null,
                );
                st.set_modal_value(body.into());
                st.set_modal_submit("Close".into());
            }
            "open-meeting" => self.open(ui, value),
            "new-recording" => {
                self.action(ui, "home", "");
                st.set_recording_setup(true);
            }
            "home" => {
                st.set_recording_setup(false);
                self.save_notes(ui);
                st.set_view(0);
                self.workflow_action(ui, "workflow-home", "");
            }
            "setup-audio" => {
                self.request(
                    ui,
                    "trigger_microphone_permission",
                    json!({}),
                    "ignored",
                    "",
                );
                self.request(
                    ui,
                    "trigger_system_audio_permission_command",
                    json!({}),
                    "ignored",
                    "",
                );
                self.action(ui, "audio-sources", "");
            }
            "setup-finish" => self.workflow_action(ui, "workflow-finish", ""),
            "filter-meetings" => self.filter_list(ui),
            "navigate" => {
                self.save_notes(ui);
                let view = value.parse().unwrap_or(2);
                st.set_view(view);
                st.set_loading(true);
                match view {
                    6 => self.request(ui, "workspace_tasks", json!({}), "tasks", ""),
                    7 => self.request(ui, "workspace_calendar_events", json!({}), "calendar", ""),
                    5 => {
                        self.results = self.trashed_meetings();
                        st.set_loading(false);
                        self.render_results(ui);
                    }
                    _ => self.search_library(ui, "", false),
                }
            }
            "search" => {
                self.search_library(ui, value, false);
            }
            "ask" => {
                if !value.trim().is_empty() {
                    self.search_library(ui, value, true);
                }
            }
            "transcript-query" => {
                if value.trim().is_empty() {
                    self.render_meeting(ui);
                } else {
                    let weak = ui.as_weak();
                    let query = value.to_owned();
                    self.search_timer.start(
                        slint::TimerMode::SingleShot,
                        Duration::from_millis(200),
                        move || {
                            if let Some(ui) = weak.upgrade() {
                                with_controller(|c| {
                                    c.request(
                                        &ui,
                                        "workspace_search",
                                        json!({
                                            "request": {
                                                "query": query,
                                                "meeting_id": c.active,
                                                "limit": 100
                                            }
                                        }),
                                        "transcript-search",
                                        &c.active,
                                    )
                                });
                            }
                        },
                    );
                }
            }
            "meeting-tab" => {
                self.save_notes(ui);
                let tab = value.parse().unwrap_or(0);
                st.set_meeting_tab(tab);
                self.render_timeline(ui);
                if tab == 2 {
                    self.request(ui, "voice_memory_status", json!({}), "voice-status", "");
                    self.request(
                        ui,
                        "list_meeting_speakers",
                        json!({"meetingId":self.active}),
                        "speakers",
                        "",
                    );
                }
            }
            "note-edit" => {
                let index = value.parse::<usize>().unwrap_or(usize::MAX);
                if let Some(block) = crate::notes::blocks(&st.get_notes()).get(index) {
                    self.note_edit_base = Some(st.get_notes().to_string());
                    st.set_note_edit_index(index as i32);
                    st.set_note_edit_value(block.text.clone().into());
                    st.set_notes_status("Editing…".into());
                }
            }
            "note-input" => {
                if let Some(edited) = self.note_edit_base.as_deref().and_then(|base| {
                    crate::notes::edit(base, st.get_note_edit_index() as usize, value)
                }) {
                    st.set_notes(edited.clone().into());
                    self.notes_edited(ui, edited);
                }
            }
            "note-save" => {
                if let Some(edited) = crate::notes::edit(
                    self.note_edit_base.as_deref().unwrap_or(&st.get_notes()),
                    st.get_note_edit_index() as usize,
                    &st.get_note_edit_value(),
                ) {
                    st.set_note_edit_index(-1);
                    self.note_edit_base = None;
                    self.notes_edited(ui, edited);
                    self.render_notes(ui);
                    self.render_timeline(ui);
                }
            }
            "notes-language" => {
                let index = value.parse::<usize>().unwrap_or(0);
                if index >= 6 {
                    self.dialog(
                        ui,
                        "notes-language",
                        "Notes language",
                        "Language code, such as it, ja or pt-BR",
                        "field",
                        Value::Null,
                    );
                    st.set_modal_value(st.get_notes_language_code());
                } else {
                    let code = ["auto", "en", "pt", "de", "es", "fr"][index];
                    st.set_notes_language_code(code.into());
                    self.request(
                        ui,
                        "api_save_meeting_summary_language",
                        json!({"meetingId": self.active, "summaryLanguage": code}),
                        "ignored",
                        "",
                    );
                }
            }
            "copy-notes" => {
                ui.invoke_copy_text(st.get_notes());
                st.set_message("Notes copied".into());
            }
            "notes-mode" => {
                self.save_notes(ui);
                st.set_editing_notes(!st.get_editing_notes());
                st.set_notes_preview(crate::notes::preview(&st.get_notes()));
                if !st.get_editing_notes() {
                    self.render_notes(ui);
                    self.render_timeline(ui);
                }
            }
            "load-more" => self.load_more(ui),
            "select-transcript" | "edit-transcript" => {
                let line = self
                    .cache
                    .get(&self.active)
                    .and_then(|m| m.lines.as_ref())
                    .and_then(|lines| lines.iter().find(|line| text(line, "id") == value))
                    .cloned();
                if let Some(line) = line {
                    self.selected = Some(line.clone());
                    let source_position = crate::playback::passage_start(&line);
                    st.set_inspector_position(source_position as f32);
                    st.set_selected_note_id("".into());
                    st.set_inspector_title(
                        format!(
                            "{} · {}",
                            text(&line, "speaker_name"),
                            time(source_position, false)
                        )
                        .into(),
                    );
                    st.set_inspector_body(if st.get_latin_devanagari() {
                        crate::transliteration::latin(&text(&line, "text")).into()
                    } else {
                        text(&line, "text").into()
                    });
                    st.set_inspector_kind("transcript".into());
                    st.set_inspector_stamp(time(source_position, false).into());
                    st.set_inspector_owner(text(&line, "speaker_name").into());
                    st.set_inspector_evidence(model(Vec::new()));
                    if action == "edit-transcript" {
                        self.dialog = (action.into(), line.clone());
                        st.set_modal("edit".into());
                        st.set_modal_title("Correct transcript".into());
                        st.set_modal_text("Your previous transcript is kept for recovery.".into());
                        st.set_modal_value(text(&line, "text").into());
                    }
                }
            }
            "toggle-play" | "play-selection" => {
                if action == "play-selection" {
                    // Counted for the person's own usage report; nothing is sent.
                    self.request(
                        ui,
                        "usage_record",
                        json!({"event":"source_played"}),
                        "usage",
                        "",
                    );
                }
                if st.get_playing() && action == "toggle-play" {
                    self.request(ui, "native_audio", json!({"action":"pause"}), "audio", "");
                } else {
                    let start = if action == "play-selection" {
                        st.get_inspector_position() as f64
                    } else {
                        self.position
                    };
                    let Some(position) = self.playback_filter.next_position(start) else {
                        st.set_message(
                            "No enabled speech remains. Change Only or Skip to keep listening."
                                .into(),
                        );
                        return;
                    };
                    self.position = position;
                    self.update_speaking(ui);
                    self.update_playback_passage(ui);
                    if action == "play-selection" && self.audio_loaded {
                        self.request(
                            ui,
                            "native_audio",
                            json!({"action": "seek", "value": position}),
                            "audio",
                            "",
                        );
                    }
                    self.request(
                        ui,
                        "native_audio",
                        json!({
                            "action": if self.audio_loaded { "play" } else { "load" },
                            "meetingId": self.active,
                            "value": self.position
                        }),
                        "audio-start",
                        "",
                    );
                }
            }
            "stop-playback" => {
                self.tick_timer.stop();
                self.audio_loaded = false;
                self.position = 0.0;
                st.set_playing(false);
                st.set_clock(time(0.0, true).into());
                st.set_playhead(0.0);
                self.update_speaking(ui);
                self.update_playback_passage(ui);
                self.request(
                    ui,
                    "native_audio",
                    json!({"action":"stop"}),
                    "audio-stop",
                    "",
                );
            }
            "seek-relative" | "seek-fraction" | "seek-absolute" => {
                let amount = value.parse::<f64>().unwrap_or(0.0);
                self.position = if action == "seek-absolute" {
                    amount
                } else if action == "seek-relative" {
                    self.position + amount
                } else {
                    amount * self.duration
                }
                .clamp(0.0, self.duration);
                st.set_clock(time(self.position, true).into());
                st.set_playhead((self.position / self.duration.max(1.0)) as f32);
                self.update_speaking(ui);
                self.update_playback_passage(ui);
                if self.audio_loaded {
                    self.request(
                        ui,
                        "native_audio",
                        json!({"action":"seek","value":self.position}),
                        "audio",
                        "",
                    );
                }
            }
            "start-recording" | "sound-check" => self.privacy_start(ui, action),
            "begin-recording" | "begin-sound-check" => {
                if st.get_recording() || st.get_busy() {
                    return;
                }
                self.workflow_save_all(ui);
                self.sound_check_generation = self.sound_check_generation.wrapping_add(1);
                self.sound_check_active = action == "begin-sound-check";
                if self.sound_check_active {
                    st.set_setup_test_done(false);
                    st.set_setup_audio_confirmed(false);
                }
                self.sound_check_playback = false;
                if st.get_recording_save_pending() {
                    st.set_error(
                        "Retry saving the previous recording before starting another.".into(),
                    );
                    return;
                }
                st.set_busy(true);
                if st.get_monitoring() {
                    self.request(ui, "stop_audio_level_monitoring", json!({}), "ignored", "");
                    st.set_monitoring(false);
                }
                self.request(
                    ui,
                    "start_recording_with_devices_and_meeting",
                    json!({
                        "micDeviceName": self.device_name(ui, true),
                        "systemDeviceName": self.device_name(ui, false),
                        "meetingName": if action == "begin-sound-check" {
                            "Sound check".to_owned()
                        } else {
                            st.get_recording_title().to_string()
                        },
                        "workflow": self
                            .workflow_capture_options(ui, action == "begin-sound-check")
                    }),
                    "record-start",
                    "",
                );
            }
            "stop-recording" => {
                self.sound_check_playback = self.sound_check_active;
                self.sound_check_active = false;
                self.sound_check_generation = self.sound_check_generation.wrapping_add(1);
                st.set_busy(true);
                self.request(
                    ui,
                    "stop_recording",
                    json!({"args":{"save_path":""}}),
                    "record-stop",
                    "",
                );
            }
            "retry-recording-save" => {
                st.set_busy(true);
                self.request(
                    ui,
                    "native_retry_recording_save",
                    json!({}),
                    "record-stop",
                    "",
                );
            }
            "pause-recording" => {
                if st.get_video_recording() {
                    self.request(
                        ui,
                        "workspace_video",
                        json!({
                            "action": {"op": "pause", "paused": !st.get_recording_paused()}
                        }),
                        "ignored",
                        "",
                    );
                }
                self.request(
                    ui,
                    if st.get_recording_paused() {
                        "resume_recording"
                    } else {
                        "pause_recording"
                    },
                    json!({}),
                    "mutation",
                    "",
                );
                st.set_recording_paused(!st.get_recording_paused());
            }
            "generate" => {
                if st.get_notes_generating() {
                    return;
                }
                self.save_notes(ui);
                st.set_notes_generating(true);
                st.set_notes_status("Reading the complete transcript…".into());
                st.set_busy(true);
                self.request(
                    ui,
                    "native_generate",
                    json!({
                        "meetingId": self.active,
                        "templateId": self.selected_template,
                        "customPrompt": if st.get_notes_context().trim().is_empty() {
                            None
                        } else {
                            Some(st.get_notes_context().to_string())
                        },
                        "summaryLanguage": st.get_notes_language_code().as_str()
                    }),
                    "generation",
                    &self.active,
                );
            }
            "cancel-generation" => {
                st.set_notes_generating(false);
                self.request(
                    ui,
                    "api_cancel_summary",
                    json!({"meetingId":self.active,"processId":self.process_id}),
                    "mutation",
                    "",
                );
                st.set_busy(false);
            }
            "settings" => {
                self.save_notes(ui);
                st.set_view(3);
                let tab = value.parse().unwrap_or(0);
                st.set_settings_tab(tab);
                self.settings_view(ui, tab);
            }
            "cloud-refresh" | "cloud-billing" | "cloud-checkout" => {
                if st.get_cloud_loading() {
                    return;
                }
                st.set_cloud_loading(true);
                let action = match action {
                    "cloud-checkout" => json!({"op":"checkout","plan":value}),
                    "cloud-billing" => json!({"op":"billing"}),
                    _ => json!({"op":"status"}),
                };
                self.request(
                    ui,
                    "cloud_account",
                    json!({"action": action}),
                    if action["op"] == "status" {
                        "cloud-status"
                    } else {
                        "cloud-link"
                    },
                    "",
                );
            }
            "cloud-use" => {
                st.set_provider("overtalk-cloud".into());
                st.set_model("glm-5.3-flash".into());
                st.set_api_key("".into());
                st.set_endpoint("".into());
                st.set_message(
                    "Private AI summaries selected. Save provider to use them for notes; choose a \
                     local model again at any time."
                        .into(),
                );
            }
            "cloud-restore" => {
                if st.get_cloud_loading() {
                    return;
                }
                st.set_cloud_loading(true);
                self.request(
                    ui,
                    "cloud_account",
                    json!({
                        "action": {
                            "op": "restore",
                            "account_token": st.get_cloud_recovery().as_str()
                        }
                    }),
                    "cloud-status",
                    "",
                );
                st.set_cloud_recovery("".into());
            }
            "reading-size" => st.set_reading_size(value.parse::<i32>().unwrap_or(0).clamp(0, 2)),
            "motion-mode" => {
                let mode = value.parse::<i32>().unwrap_or(0).clamp(0, 2);
                st.set_motion_mode(mode);
                st.set_reduced_motion(crate::motion::reduced(mode, st.get_system_reduced_motion()));
            }
            "open-database-folder" => {
                self.request(ui, "open_database_folder", json!({}), "ignored", "")
            }
            "open-models-folder" => {
                self.request(ui, "open_models_folder", json!({}), "ignored", "")
            }
            "notifications" => {
                if let Some(settings) = self.settings.get_mut("get_notification_settings") {
                    settings["recording_notifications"] = json!(value == "true");
                    if value == "true" {
                        settings["consent_given"] = true.into();
                        settings["notification_preferences"]["show_recording_started"] =
                            true.into();
                        settings["notification_preferences"]["show_recording_stopped"] =
                            true.into();
                    }
                    let settings = settings.clone();
                    self.request(
                        ui,
                        "set_notification_settings",
                        json!({"settings":settings}),
                        "ignored",
                        "",
                    );
                    if value == "true" {
                        self.request(
                            ui,
                            "request_notification_permission",
                            json!({}),
                            "ignored",
                            "",
                        );
                    }
                }
            }
            "theme" => st.set_dark(value == "dark"),
            "zoom" => st.set_zoom(value.parse().unwrap_or(1.0)),
            "advanced-provider" => {
                let mut config = self
                    .settings
                    .get("api_get_custom_openai_config")
                    .cloned()
                    .filter(Value::is_object)
                    .unwrap_or_else(|| json!({"maxTokens":null,"temperature":null,"topP":null}));
                config["endpoint"] = st.get_endpoint().as_str().into();
                config["model"] = st.get_model().as_str().into();
                self.form(
                    ui,
                    "custom-provider",
                    "Advanced generation settings",
                    config,
                );
            }
            "privatemode-help" => self.request(
                ui,
                "open_external_url",
                json!({
                    "url": "https://docs.privatemode.ai/api/proxy-configuration/"
                }),
                "ignored",
                "",
            ),
            "save-provider" => {
                if st.get_provider().trim().is_empty() || st.get_model().trim().is_empty() {
                    st.set_error("Choose a provider and model before saving.".into());
                    return;
                }
                let speech = st.get_settings_tab() == 2;
                let cfg = if speech {
                    "api_save_transcript_config"
                } else {
                    "api_save_model_config"
                };
                let key = st.get_api_key();
                let whisper = self
                    .settings
                    .get("api_get_model_config")
                    .and_then(|config| config["whisperModel"].as_str())
                    .unwrap_or("base");
                let config = json!({
                    "provider": st.get_provider().as_str(),
                    "model": st.get_model().as_str(),
                    "apiKey": if key.is_empty() {
                        Value::Null
                    } else {
                        Value::String(key.to_string())
                    },
                    "ollamaEndpoint": st.get_endpoint().as_str(),
                    "whisperModel": whisper
                });
                if !speech && st.get_provider() == "custom-openai" {
                    let mut custom = self
                        .settings
                        .get("api_get_custom_openai_config")
                        .cloned()
                        .filter(Value::is_object)
                        .unwrap_or_else(
                            || json!({"maxTokens": null, "temperature": null, "topP": null}),
                        );
                    custom["endpoint"] = st.get_endpoint().as_str().into();
                    custom["model"] = st.get_model().as_str().into();
                    if !key.is_empty() {
                        custom["apiKey"] = key.as_str().into();
                    }
                    self.request(
                        ui,
                        "api_save_custom_openai_config",
                        custom,
                        "custom-provider-saved",
                        &config.to_string(),
                    );
                } else {
                    self.request(
                        ui,
                        cfg,
                        config,
                        "settings-saved",
                        if speech { "transcript" } else { "summary" },
                    );
                }
                st.set_api_key("".into());
            }
            "open-recording-folder" => {
                self.request(ui, "open_recordings_folder", json!({}), "ignored", "")
            }
            "recording-folder" => self.request(
                ui,
                "select_recording_folder",
                json!({}),
                "picked-recording-folder",
                "",
            ),
            "import" => self.request(
                ui,
                "select_and_validate_audio_command",
                json!({}),
                "picked-import",
                "",
            ),
            "import-calendar" => self.request(
                ui,
                "workspace_pick_file",
                json!({"kind":"calendar"}),
                "picked-calendar",
                "",
            ),
            "index-library" => self.request(ui, "library_index", json!({}), "mutation", ""),
            "result-open" => {
                if let Some(row) = self.results.iter().find(|v| result_id(v) == value).cloned() {
                    if st.get_view() == 6 {
                        self.request(
                            ui,
                            "workspace_task_status",
                            json!({"id":value,"status":"done"}),
                            "mutation",
                            "",
                        );
                    } else if st.get_view() == 5 {
                        self.request(
                            ui,
                            "workspace_organise_meeting",
                            json!({
                                "change": {
                                    "meeting_id": text(&row, "meeting_id"),
                                    "trashed": false
                                }
                            }),
                            "mutation",
                            "",
                        );
                    } else {
                        self.open(ui, &text(&row, "meeting_id"));
                        self.position = row["start_time"].as_f64().unwrap_or(0.0);
                        st.set_clock(time(self.position, true).into());
                        st.set_inspector_title(
                            format!("Source · {}", time(self.position, false)).into(),
                        );
                        st.set_inspector_body(text(&row, "body").into());
                        if text(&row, "source_kind") == "summary" {
                            st.set_meeting_tab(1);
                        }
                    }
                }
            }
            "complete-task" | "reopen-task" => self.request(
                ui,
                "workspace_task_status",
                json!({
                    "id": value,
                    "status": if action == "complete-task" {
                        "done"
                    } else {
                        "open"
                    }
                }),
                "mutation",
                "",
            ),
            "modal-cancel" => {
                self.cancel_account_signin(ui);
                if self.dialog.0 == "hub-video-target" {
                    st.set_video_starting(false);
                }
                if self.dialog.0 == "hub-connection-config" {
                    st.set_form_rows(model(Vec::new()));
                    st.set_connection_provider("".into());
                    st.set_connection_busy(false);
                }
                st.set_modal("".into());
                self.dialog = Default::default();
            }
            "modal-save" => self.modal_save(ui),
            "choice-search" => {
                let query = value.to_lowercase();
                st.set_modal_choices(model(
                    self.choice_rows
                        .iter()
                        .filter(|row| {
                            format!("{} {}", row.title, row.description)
                                .to_lowercase()
                                .contains(&query)
                        })
                        .cloned()
                        .collect(),
                ));
            }
            "modal-choice" => self.modal_choice(ui, value),
            "choose-template" => {
                let rows = self
                    .settings
                    .get("api_list_templates")
                    .map(array)
                    .unwrap_or_default()
                    .iter()
                    .map(|v| (text(v, "id"), text(v, "name")))
                    .collect();
                self.choices(ui, "template", "Notes template", rows, Value::Null);
            }
            "meeting-options" => self.choices(
                ui,
                "meeting-options",
                "Meeting options",
                vec![
                    ("rename".into(), "Rename meeting".into()),
                    ("pin".into(), "Pin / unpin".into()),
                    ("archive".into(), "Archive / unarchive".into()),
                    ("transcribe".into(), "Transcribe recording again".into()),
                    ("trash".into(), "Move to trash".into()),
                ],
                Value::Null,
            ),
            "list-options" => {
                let mut rows: Vec<DetailRow> = [
                    ("active", "Active meetings"),
                    ("pinned", "Pinned meetings"),
                    ("archived", "Archived meetings"),
                ]
                .into_iter()
                .map(|(id, title)| menus::present("list-options", id.into(), title.into()))
                .collect();
                let (unfiled, suggested) = self.unfiled_counts();
                rows.push(DetailRow {
                    id: "needs-project".into(),
                    title: "Needs a project".into(),
                    description: match (unfiled, suggested) {
                        (0, _) => "Every meeting has a project".into(),
                        (1, 0) => "1 meeting".into(),
                        (n, 0) => format!("{n} meetings"),
                        (n, s) => format!(
                            "{n} meeting{} · {s} with a suggestion",
                            if n == 1 { "" } else { "s" }
                        ),
                    }
                    .into(),
                    value: "folder".into(),
                    checked: false,
                });
                self.rich_choices(ui, "list-options", "Show meetings", rows, Value::Null);
            }
            "rename-speaker" => {
                self.dialog(
                    ui,
                    "rename-speaker",
                    "Rename speaker",
                    "Speaker name",
                    "field",
                    value.into(),
                );
            }
            "voice-setup" => {
                st.set_voice_busy(true);
                st.set_message("Downloading the 28 MB voice model…".into());
                self.request(ui, "voice_memory_setup", json!({}), "voice-setup", "");
            }
            "voice-menu" => {
                let mut rows = vec![(
                    "voice-setup".into(),
                    if st.get_voice_ready() {
                        "Check voice model".into()
                    } else {
                        "Set up voice memory · 28 MB".into()
                    },
                )];
                if st.get_voice_ready() && st.get_voice_profiles().row_count() > 0 {
                    rows.push((
                        "recognize-voices".into(),
                        "Recognize voices in this meeting".into(),
                    ));
                    rows.push(("manage-voices".into(), "Manage remembered voices".into()));
                }
                self.choices(ui, "voice-menu", "Local voice memory", rows, Value::Null);
            }
            "remember-voice" => {
                let name = st
                    .get_details()
                    .iter()
                    .find(|row| row.id.as_str() == value)
                    .map(|row| row.title.to_string())
                    .filter(|title| !title.starts_with("Speaker "))
                    .unwrap_or_default();
                self.voice_consent_form(ui, value, &name, "");
            }
            "recognize-voices" => {
                st.set_voice_busy(true);
                st.set_message("Checking remembered voices…".into());
                self.request(
                    ui,
                    "recognize_meeting_speakers",
                    json!({"meetingId": self.active}),
                    "voice-matched",
                    &self.active,
                );
            }
            "manage-voices" => {
                let rows = st
                    .get_voice_profiles()
                    .iter()
                    .map(|r| (r.id.to_string(), r.title.to_string()))
                    .collect();
                self.choices(
                    ui,
                    "forget-voice-choice",
                    "Remembered voices · stored on this computer",
                    rows,
                    Value::Null,
                );
            }
            "merge-speaker" => {
                let rows = st
                    .get_details()
                    .iter()
                    .filter(|v| v.id.as_str() != value)
                    .map(|v| (v.id.to_string(), format!("Merge into {}", v.title)))
                    .collect();
                self.choices(
                    ui,
                    "merge-speaker",
                    "Combine speaker labels",
                    rows,
                    value.into(),
                );
            }
            "analyze-speakers" => {
                self.request(
                    ui,
                    "speech_analyze_meeting",
                    json!({
                        "meetingId": self.active,
                        "jobId": format!(
                            "native-speakers-{}",
                            chrono::Utc::now().timestamp_millis()
                        ),
                        "diarize": true
                    }),
                    "mutation",
                    "",
                );
                st.set_message("Analyzing speakers…".into());
            }
            "consolidate-speakers" => {
                st.set_voice_busy(true);
                st.set_message("Comparing voices…".into());
                self.request(
                    ui,
                    "consolidate_meeting_speakers",
                    json!({"meetingId": self.active}),
                    "voice-consolidated",
                    &self.active,
                );
            }
            "transcribe" => self.transcribe(ui),
            "cancel-transcription" => self.request(
                ui,
                "cancel_retranscription_command",
                json!({}),
                "transcription-cancelled",
                "",
            ),
            "check-audio" => {
                let active = st.get_monitoring();
                let devices = vec![self.device_name(ui, true), self.device_name(ui, false)]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>();
                let args = if active {
                    json!({})
                } else {
                    json!({"deviceNames":devices})
                };
                self.request(
                    ui,
                    if active {
                        "stop_audio_level_monitoring"
                    } else {
                        "start_audio_level_monitoring"
                    },
                    args,
                    "monitor",
                    if active { "stop" } else { "start" },
                );
            }
            "mic-device" | "system-device" => {
                st.set_setup_audio_confirmed(false);
                if st.get_monitoring() {
                    self.request(
                        ui,
                        "stop_audio_level_monitoring",
                        json!({}),
                        "monitor",
                        "stop",
                    );
                }
                self.save_audio_devices(ui);
            }
            "new-task" | "edit-task" => {
                let mut task = if action == "edit-task" {
                    self.results
                        .iter()
                        .find(|v| text(v, "id") == value)
                        .cloned()
                        .unwrap_or_default()
                } else {
                    json!({
                        "id": null,
                        "meeting_id": null,
                        "title": "",
                        "owner": "",
                        "due_date": null,
                        "status": "open",
                        "evidence": []
                    })
                };
                if task["evidence"].is_null() {
                    task["evidence"] = task["evidence_json"]
                        .as_str()
                        .and_then(|s| serde_json::from_str(s).ok())
                        .unwrap_or_else(|| json!([]));
                }
                let title = text(&task, "title");
                let owner = text(&task, "owner");
                let due = text(&task, "due_date");
                self.dialog(
                    ui,
                    "task",
                    "Meeting action",
                    "What needs to happen?",
                    "task",
                    task,
                );
                st.set_modal_value(title.into());
                st.set_modal_secondary(owner.into());
                st.set_modal_secondary_label("Owner (optional)".into());
                st.set_modal_has_date(true);
                st.set_modal_date(due.into());
            }
            "calendar-related" => {
                if let Some(row) = self.results.iter().find(|v| result_id(v) == value) {
                    let title = text(row, "title");
                    st.set_view(2);
                    st.set_query(title.clone().into());
                    st.set_filter_after("".into());
                    st.set_filter_before("".into());
                    st.set_filter_tag("".into());
                    self.search_library(ui, &title, false);
                }
            }
            "join-calendar" => self.request(
                ui,
                "workspace_join_event",
                json!({"id": value}),
                "ignored",
                "",
            ),
            "calendar-remove" => self.request(
                ui,
                "workspace_delete_calendar_event",
                json!({"id": value}),
                "mutation",
                "",
            ),
            "delete-permanently" => {
                let id = self
                    .results
                    .iter()
                    .find(|v| result_id(v) == value)
                    .map(|v| text(v, "meeting_id"))
                    .unwrap_or_default();
                self.dialog(
                    ui,
                    "delete-permanently",
                    "Permanently delete meeting?",
                    "The recording, transcript and notes will be removed. This cannot be undone.",
                    "confirm",
                    id.into(),
                );
                st.set_modal_submit("Delete permanently".into());
            }
            "create-backup" => {
                self.dialog(
                    ui,
                    "create-backup",
                    "Create encrypted backup",
                    "Choose a passphrase of at least 10 characters. Keep it somewhere safe; it \
                     cannot be recovered.",
                    "password",
                    json!({}),
                );
                st.set_modal_submit("Choose backup location".into());
            }
            "restore-backup" => self.request(
                ui,
                "workspace_pick_file",
                json!({"kind":"backup-open"}),
                "restore-path",
                "",
            ),
            "manage-models" => {
                if st.get_provider() == "localSpeech" {
                    self.action(ui, "setting-row", "speech");
                    return;
                }
                self.show_models(ui);
            }
            "choose-provider" => self.choose_provider(ui),
            "setting-row" => match value {
                "speech" | "library" | "connections" => {
                    let command = match value {
                        "speech" => "speech_get_settings",
                        "library" => "library_get_settings",
                        _ => "integrations_get_settings",
                    };
                    let config = self.settings.get(command).cloned().unwrap_or_default();
                    let config = if value == "connections" {
                        config["settings"].clone()
                    } else {
                        config
                    };
                    self.form(ui, value, &format!("Configure {value}"), config);
                }
                "gpu" => self.request(ui, "gpu_memory_report", json!({}), "diagnostics", ""),
                _ => self.request(ui, "workspace_diagnostics", json!({}), "diagnostics", ""),
            },
            "export" => {
                self.save_notes(ui);
                self.choices(
                    ui,
                    "export",
                    "Export meeting",
                    vec![
                        ("pdf".into(), "PDF document (.pdf)".into()),
                        ("docx".into(), "Word document (.docx)".into()),
                        ("markdown".into(), "Markdown (.md)".into()),
                        ("text".into(), "Plain text (.txt)".into()),
                        ("json".into(), "Structured data (.json)".into()),
                    ],
                    Value::Null,
                );
            }
            "copy-transcript" => self.request(
                ui,
                "native_transcript_text",
                json!({"meetingId":self.active}),
                "clipboard",
                "",
            ),
            "about" => {
                st.set_modal("about".into());
                st.set_modal_title("About Overtalk".into());
                st.set_modal_text(
                    "A local-first meeting notebook. Open source under the MIT licence.".into(),
                );
            }
            _ => {
                st.set_error("This action is unavailable.".into());
            }
        }
    }
}
