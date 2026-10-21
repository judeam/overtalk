//! Service reply routing. Domain rendering lives in sibling modules.

use super::*;

impl Controller {
    pub(crate) fn reply(
        &mut self,
        ui: &AppWindow,
        tag: &str,
        context: &str,
        result: Result<Value, String>,
    ) {
        if tag.starts_with("workflow-") {
            self.workflow_reply(ui, tag, context, result);
            return;
        }
        if tag.starts_with("automation-") {
            self.automation_reply(ui, tag, context, result);
            return;
        }
        if tag.starts_with("share-") {
            self.share_reply(ui, tag, context, result);
            return;
        }
        let st = ui.global::<State>();
        if tag.starts_with("cloud-") {
            st.set_cloud_loading(false);
            match result {
                Err(error) => st.set_cloud_notice(error.into()),
                Ok(value) if tag == "cloud-link" => {
                    if let Some(url) = value["url"].as_str() {
                        self.request(ui, "open_external_url", json!({"url":url}), "ignored", "");
                        st.set_cloud_notice(
                            "Finish in your browser, then refresh your credits. Opening checkout \
                             does not add credits or charge your card."
                                .into(),
                        );
                    }
                }
                Ok(value) if value["route"] == "end-to-end" => {
                    st.set_cloud_available(value["configured"].as_bool().unwrap_or(false));
                    let connected = value["connected"].as_bool().unwrap_or(false);
                    let entitled = value["entitled"] == true;
                    st.set_cloud_connected(connected);
                    st.set_cloud_end_to_end(true);
                    st.set_cloud_can_use(entitled);
                    st.set_cloud_held("".into());
                    st.set_cloud_balance(
                        if entitled {
                            format!(
                                "{}+ hours a month of private AI summaries",
                                value["monthly_hours"].as_u64().unwrap_or(200)
                            )
                        } else {
                            "Not part of your plan yet".into()
                        }
                        .into(),
                    );
                    st.set_cloud_notice(
                        if entitled {
                            "Your computer encrypts each summary for Privatemode with your own \
                             key. Overtalk never receives meeting text."
                        } else if connected {
                            "A monthly or annual subscription adds private AI summaries. Local \
                             processing stays available, and everything else is free."
                        } else {
                            "Choose a plan to see its price in Stripe Checkout. Local processing \
                             stays available."
                        }
                        .into(),
                    );
                }
                Ok(value) => {
                    st.set_cloud_available(value["configured"].as_bool().unwrap_or(false));
                    let connected = value["connected"].as_bool().unwrap_or(false);
                    st.set_cloud_connected(connected);
                    st.set_cloud_end_to_end(false);
                    let available = value["balance"]["available_micro_usd"]
                        .as_i64()
                        .unwrap_or(0);
                    let held = value["balance"]["held_micro_usd"].as_i64().unwrap_or(0);
                    st.set_cloud_can_use(available > 0 && value["balance"]["suspended"] != true);
                    st.set_cloud_balance(
                        format!("${:.2} available", available as f64 / 1_000_000.0).into(),
                    );
                    st.set_cloud_held(if held > 0 {
                        format!(
                            "${:.2} reserved for pending requests. If this remains after \
                             generation, contact billing support.",
                            held as f64 / 1_000_000.0
                        )
                        .into()
                    } else {
                        "".into()
                    });
                    st.set_cloud_notice(
                        if value["balance"]["suspended"] == true {
                            "Cloud spending is paused for a billing review."
                        } else if !st.get_cloud_available() {
                            "Cloud plans are not available yet. Local models and your own provider \
                             connections keep working."
                        } else if !connected {
                            "Choose an option to see its price in Stripe Checkout. No provider API \
                             key is needed."
                        } else {
                            "GLM-5.3-Flash · usage includes input, notes and reasoning. Your \
                             balance is enforced by the server."
                        }
                        .into(),
                    );
                }
            }
            return;
        }
        if (tag.starts_with("audio") || tag == "generation") && context != self.active {
            return;
        }
        if matches!(tag, "search" | "question")
            && (context != self.library_revision.to_string() || st.get_view() != 2)
        {
            return;
        }
        if (tag == "tasks" && st.get_view() != 6) || (tag == "calendar" && st.get_view() != 7) {
            return;
        }
        let value = match result {
            Ok(value) => value,
            Err(error) => {
                if matches!(tag, "hub-auth-started" | "hub-auth-finished") {
                    if st.get_connection_attempt().as_str() != context {
                        return;
                    }
                    st.set_connection_attempt("".into());
                    st.set_connection_busy(false);
                }
                if tag == "hub-connected" {
                    st.set_connection_busy(false);
                }
                if tag.starts_with("hub-") {
                    st.set_hub_loading(false);
                }
                if tag.starts_with("hub-video-") {
                    st.set_video_starting(false);
                }
                if tag.starts_with("voice-") {
                    st.set_voice_busy(false);
                }
                if tag == "generation" || tag == "generation-poll" {
                    st.set_notes_generating(false);
                    st.set_busy(false);
                }
                if tag == "more" {
                    if let Some(meeting) = self.cache.get_mut(context) {
                        meeting.loading_more = false;
                    }
                }
                if tag == "record-stop" && !st.get_recording() {
                    st.set_recording_save_pending(true);
                }
                if tag == "saved-notes" {
                    let saved: Value = serde_json::from_str(context).unwrap_or_default();
                    self.drafts.finish_save(
                        &text(&saved, "id"),
                        saved["revision"].as_u64().unwrap_or(0),
                        false,
                    );
                    self.closing = false;
                }
                // A usage count that could not be saved is not the person's problem.
                if tag != "peaks" && tag != "audio-stop" && tag != "usage" {
                    if tag == "transcribing" {
                        st.set_transcribing(false);
                    }
                    st.set_error(error.into());
                    st.set_loading(false);
                    st.set_busy(false);
                }
                return;
            }
        };
        if tag.starts_with("template-") {
            self.template_reply(ui, tag, value);
            return;
        }
        if tag.starts_with("privacy-") {
            self.privacy_reply(ui, tag, context, value);
            return;
        }
        if tag.starts_with("hub-") {
            self.hub_reply(ui, tag, context, value);
            return;
        }
        match tag {
            "voice-status" => {
                st.set_voice_ready(value["ready"].as_bool().unwrap_or(false));
                st.set_voice_profiles(model(
                    array(&value["profiles"])
                        .iter()
                        .map(|p| DetailRow {
                            id: text(p, "id").into(),
                            title: text(p, "name").into(),
                            ..Default::default()
                        })
                        .collect(),
                ));
            }
            "voice-setup" | "voice-saved" | "voice-matched" | "voice-forgotten"
            | "voice-consolidated" => {
                st.set_voice_busy(false);
                st.set_modal("".into());
                st.set_message(match tag {
                    "voice-consolidated" => format!(
                        "Reunited {} split speaker labels; uncertain voices left unchanged",
                        value.as_u64().unwrap_or(0)
                    )
                    .into(),
                    "voice-setup" => "Voice memory is ready. Confirm a speaker’s name to remember \
                                      their voice."
                        .into(),
                    "voice-saved" => "Voice remembered on this device".into(),
                    "voice-matched" => format!(
                        "Recognized {} speaker labels; uncertain voices left unchanged",
                        value.as_u64().unwrap_or(0)
                    )
                    .into(),
                    _ => "Voice profile deleted".into(),
                });
                self.request(ui, "voice_memory_status", json!({}), "voice-status", "");
                if !self.active.is_empty() {
                    self.reload_active(ui);
                    self.request(
                        ui,
                        "list_meeting_speakers",
                        json!({"meetingId":self.active}),
                        "speakers",
                        "",
                    );
                }
            }
            "overviews" => {
                self.overviews = array(&value);
                if self.settings.contains_key("get_onboarding_status") {
                    st.set_setup_needed(
                        self.overviews.is_empty()
                            && !self.settings["get_onboarding_status"]["completed"]
                                .as_bool()
                                .unwrap_or(false),
                    );
                }
                self.filter_list(ui);
                let ids: Vec<_> = self
                    .overviews
                    .iter()
                    .take(3)
                    .map(|v| text(v, "id"))
                    .collect();
                for id in ids {
                    self.prefetch(ui, &id);
                }
            }
            "organization" => {
                self.organization = array(&value)
                    .into_iter()
                    .map(|v| (text(&v, "meeting_id"), v))
                    .collect();
                self.filter_list(ui);
                if st.get_view() == 5 {
                    self.results = self.trashed_meetings();
                    self.render_results(ui);
                }
            }
            "metadata" | "transcripts" | "timeline" | "summary" | "peaks" => {
                if let Some(meeting) = self.cache.get_mut(context) {
                    match tag {
                        "metadata" => meeting.metadata = Some(value),
                        "transcripts" => {
                            meeting.total = value["total_count"].as_u64().unwrap_or(0) as usize;
                            meeting.lines = Some(array(&value["transcripts"]));
                            meeting.rows = None;
                        }
                        "timeline" => {
                            meeting.timeline = Some(value);
                            meeting.lane_images = None;
                            meeting.rows = None;
                        }
                        "summary" => {
                            meeting.summary = Some(if let Some(draft) = self.drafts.text(context) {
                                json!({"data":{"markdown":draft}})
                            } else {
                                value
                            })
                        }
                        "peaks" => {
                            meeting.peaks_loaded = true;
                            meeting.lane_images = None;
                            meeting.peaks = array(&value)
                                .into_iter()
                                .filter_map(|v| v.as_u64().map(|n| n as u8))
                                .collect()
                        }
                        _ => {}
                    }
                    if context == self.active {
                        match tag {
                            "summary" => {
                                self.render_notes(ui);
                                if self
                                    .cache
                                    .get(context)
                                    .and_then(|meeting| meeting.summary.as_ref())
                                    .is_some_and(|summary| {
                                        matches!(
                                            summary["status"].as_str(),
                                            Some("processing" | "pending" | "running")
                                        )
                                    })
                                {
                                    st.set_busy(true);
                                    st.set_notes_generating(true);
                                    st.set_notes_status("Generating notes…".into());
                                    self.poll_summary(ui);
                                }
                                if let Some(m) = self.cache.get_mut(context) {
                                    m.lane_images = None;
                                }
                                self.render_timeline(ui);
                                self.select_default_note(ui);
                            }
                            "peaks" => self.render_timeline(ui),
                            "timeline" => {
                                self.render_meeting(ui);
                                self.render_timeline(ui);
                                self.select_default_note(ui);
                            }
                            _ => self.render_meeting(ui),
                        }
                    }
                }
            }
            "notes-language" if context == self.active => {
                let code = value["language"]
                    .as_str()
                    .unwrap_or(st.get_default_summary_language().as_str())
                    .to_owned();
                st.set_notes_language_index(
                    ["auto", "en", "pt", "de", "es", "fr"]
                        .iter()
                        .position(|known| *known == code)
                        .unwrap_or(6) as i32,
                );
                st.set_notes_language_code(code.into());
            }
            "marks" => {
                if let Some(meeting) = self.cache.get_mut(context) {
                    meeting.marks = array(&value);
                }
                if context == self.active {
                    st.set_markers(model(
                        array(&value)
                            .iter()
                            .map(|row| ClipRow {
                                id: row["id"].to_string().into(),
                                label: text(row, "label").into(),
                                body: format!(
                                    "{} · {}",
                                    time(row["at"].as_f64().unwrap_or(0.0), false),
                                    text(row, "label")
                                )
                                .into(),
                                start: row["at"].as_f64().unwrap_or(0.0) as f32,
                                end: row["at"].as_f64().unwrap_or(0.0) as f32,
                                ..Default::default()
                            })
                            .collect(),
                    ));
                }
            }
            "mark-deleted" => self.request(
                ui,
                "meeting_marks",
                json!({"meetingId":context}),
                "marks",
                context,
            ),
            "more" => {
                if let Some(meeting) = self.cache.get_mut(context) {
                    meeting.rows = None;
                    meeting.loading_more = false;
                    let page = array(&value["transcripts"]);
                    let lines = meeting.lines.get_or_insert_default();
                    meeting.total = value["total_count"]
                        .as_u64()
                        .map(|n| n as usize)
                        .unwrap_or(meeting.total);
                    // A library edit can shorten a meeting while a page is in flight.
                    // Do not repeatedly request an empty tail while playback advances.
                    if page.is_empty() {
                        meeting.total = meeting.total.min(lines.len());
                    }
                    lines.extend(page);
                }
                if context == self.active {
                    self.render_meeting(ui);
                }
            }
            "setting" => {
                if context == "get_onboarding_status" {
                    st.set_setup_needed(
                        self.overviews.is_empty() && !value["completed"].as_bool().unwrap_or(false),
                    );
                }
                if context == "get_notification_settings" {
                    st.set_notifications(
                        value["recording_notifications"].as_bool().unwrap_or(false),
                    );
                }
                self.settings.insert(context.into(), value);
                self.apply_settings(ui);
                if st.get_view() == 3
                    && st.get_provider().is_empty()
                    && ((st.get_settings_tab() == 2 && context == "api_get_transcript_config")
                        || (st.get_settings_tab() == 3 && context == "api_get_model_config"))
                {
                    self.settings_view(ui, st.get_settings_tab());
                }
                if context == "api_list_templates" {
                    if let Some(selected) = self
                        .settings
                        .get(context)
                        .and_then(Value::as_array)
                        .and_then(|templates| {
                            templates
                                .iter()
                                .find(|template| text(template, "id") == self.selected_template)
                        })
                    {
                        st.set_selected_template(text(selected, "name").into());
                    }
                    if st.get_modal() == "template" {
                        self.render_template(ui);
                    }
                }
            }
            "settings-saved" => {
                st.set_busy(false);
                st.set_modal("".into());
                self.dialog = Default::default();
                st.set_message("Settings saved".into());
                self.initialize(ui);
            }
            "monitor" => {
                st.set_monitoring(context == "start");
                if context == "stop" {
                    st.set_audio_level(0.0);
                }
            }
            "custom-provider-saved" => {
                self.request(
                    ui,
                    "api_save_model_config",
                    serde_json::from_str(context).unwrap_or(Value::Null),
                    "settings-saved",
                    "summary",
                );
            }
            "setup-finished" => {
                st.set_setup_needed(false);
                st.set_message(
                    "Workspace ready. Join your next meeting or start recording.".into(),
                );
            }
            "clipboard" => {
                if let Some(text) = value.as_str() {
                    ui.invoke_copy_text(if st.get_latin_devanagari() {
                        crate::transliteration::latin(text).into()
                    } else {
                        text.into()
                    });
                    st.set_message("Transcript copied".into());
                }
            }
            "transcribing" => {
                st.set_message("Transcribing the saved recording…".into());
            }
            "transcription-cancelled" => {
                st.set_transcribing(false);
                st.set_message(
                    "Transcription cancelled. Your previous transcript is preserved.".into(),
                );
            }
            "models" => self.show_model_rows(ui, context, value),
            "models-init" => self.model_list(ui, context),
            "model-changed" => {
                st.set_message("Model storage updated".into());
                if st.get_view() == 3 && st.get_provider() == context {
                    self.show_models(ui);
                }
            }
            "diagnostics" => {
                self.dialog(
                    ui,
                    "diagnostics",
                    "Diagnostics",
                    "Local runtime status",
                    "edit",
                    Value::Null,
                );
                st.set_modal_value(
                    serde_json::to_string_pretty(&value)
                        .unwrap_or_default()
                        .into(),
                );
                st.set_modal_submit("Close".into());
            }
            "backup-path" => {
                if let Some(path) = value.as_str() {
                    self.request(
                        ui,
                        "workspace_create_backup",
                        json!({
                            "path": path,
                            "passphrase": self.dialog.1["passphrase"],
                            "includeAudio": true
                        }),
                        "backup-complete",
                        "",
                    );
                    self.dialog = Default::default();
                    st.set_modal("".into());
                    st.set_message("Creating encrypted backup…".into());
                }
            }
            "backup-complete" => {
                st.set_message("Encrypted backup created".into());
            }
            "restore-path" => {
                if let Some(path) = value.as_str() {
                    self.dialog(
                        ui,
                        "restore-backup",
                        "Restore encrypted backup",
                        "Enter the backup passphrase. You will choose an empty recovery folder; \
                         your current library will be preserved.",
                        "password",
                        json!({"path": path}),
                    );
                    st.set_modal_submit("Choose recovery folder".into());
                }
            }
            "restore-directory" => {
                if let Some(path) = value.as_str() {
                    self.request(
                        ui,
                        "workspace_restore_backup",
                        json!({
                            "path": self.dialog.1["path"],
                            "passphrase": self.dialog.1["passphrase"],
                            "directory": path
                        }),
                        "restore-complete",
                        "",
                    );
                    self.dialog = Default::default();
                    st.set_modal("".into());
                    st.set_message("Restoring backup…".into());
                }
            }
            "restore-complete" => {
                st.set_message(
                    format!(
                        "Backup restored to a separate recovery folder. {}",
                        serde_json::to_string(&value).unwrap_or_default()
                    )
                    .into(),
                );
            }
            "exported" => {
                if value.is_string() {
                    st.set_message(
                        format!("Exported to {}", value.as_str().unwrap_or_default()).into(),
                    );
                }
            }
            "search" => {
                self.results = array(&value["sources"]);
                st.set_loading(false);
                self.render_results(ui);
            }
            "transcript-search" => {
                if context == self.active {
                    let rows: Vec<Value> = array(&value["sources"])
                        .into_iter()
                        .map(|mut v| {
                            v["text"] = v["body"].clone();
                            v["id"] = v["source_id"].clone();
                            v
                        })
                        .collect();
                    st.set_transcripts(model(self.transcript_rows(
                        &rows,
                        st.get_dark(),
                        st.get_latin_devanagari(),
                    )));
                }
            }
            "tasks" | "calendar" => {
                self.results = array(&value);
                st.set_loading(false);
                self.render_results(ui);
            }
            "speakers" => {
                st.set_details(model(
                    array(&value)
                        .iter()
                        .map(|v| DetailRow {
                            id: text(v, "id").into(),
                            title: text(v, "name").into(),
                            description: format!(
                                "{} passages{}",
                                v["segment_count"],
                                match v["identity_source"].as_str() {
                                    Some("matched") => " · Recognized voice · check this name",
                                    Some("remembered") => " · Remembered on this device",
                                    _ => "",
                                }
                            )
                            .into(),
                            ..Default::default()
                        })
                        .collect(),
                ));
            }
            "saved-notes" => {
                let saved: Value = serde_json::from_str(context).unwrap_or_default();
                let id = text(&saved, "id");
                self.drafts
                    .finish_save(&id, saved["revision"].as_u64().unwrap_or(0), true);
                if self.drafts.unsaved(&id).is_some() {
                    self.save_draft(ui, &id);
                } else if id == self.active {
                    st.set_notes_status("All changes saved".into());
                }
                if self.closing
                    && self.drafts.dirty_ids().is_empty()
                    && self.workflow.drafts.dirty_ids().is_empty()
                {
                    let _ = slint::quit_event_loop();
                }
                if self.drafts.dirty_ids().is_empty() {
                    if let Some(args) = self.pending_export.take() {
                        self.request(ui, "native_export", args, "exported", "");
                    }
                }
            }
            "record-start" => {
                self.workflow_record_started(ui, &value);
                self.automation_capture_started(ui, &value);
                st.set_busy(false);
                st.set_recording(true);
                if self.sound_check_active {
                    let generation = self.sound_check_generation;
                    let weak = ui.as_weak();
                    slint::Timer::single_shot(Duration::from_secs(10), move || {
                        if let Some(ui) = weak.upgrade() {
                            with_controller(|c| {
                                if c.sound_check_active
                                    && c.sound_check_generation == generation
                                    && ui.global::<State>().get_recording()
                                {
                                    c.action(&ui, "stop-recording", "");
                                }
                            });
                        }
                    });
                }
                if st.get_video_enabled() && !self.sound_check_active {
                    self.hub_action(ui, "hub-video-start", "");
                }
                st.set_recording_paused(false);
                st.set_transcripts(model(Vec::new()));
                self.live_completed.clear();
                self.request(
                    ui,
                    "get_transcript_history",
                    json!({}),
                    "record-history",
                    "",
                );
                self.start_recording_clock(ui);
            }
            "record-stop" => {
                st.set_recording_save_pending(false);
                st.set_recording(false);
                st.set_busy(false);
                self.recording_timer.stop();
                self.refresh_list(ui);
                if let Some(id) = value["meeting_id"].as_str() {
                    self.request(
                        ui,
                        "workspace_video",
                        json!({"action":{"op":"stop"}}),
                        "hub-video-stopped",
                        id,
                    );
                    self.open(ui, id);
                    let was_sound_check = self.sound_check_playback;
                    if was_sound_check {
                        self.sound_check_playback = false;
                        self.action(ui, "toggle-play", "");
                    }
                    if was_sound_check {
                        st.set_setup_test_done(true);
                    }
                }
            }
            "record-state" => {
                self.request(
                    ui,
                    "workspace_recording_health",
                    json!({}),
                    "record-health",
                    "",
                );
                st.set_clock(
                    time(value["recording_duration"].as_f64().unwrap_or(0.0), true).into(),
                );
                st.set_recording_paused(value["is_paused"].as_bool().unwrap_or(false));
            }
            "record-history" => {
                if st.get_recording() {
                    for row in array(&value) {
                        self.upsert_live_transcript(ui, &row);
                    }
                }
            }
            "record-health" => {
                if self.sound_check_active {
                    self.workflow_record_levels(ui, &value);
                }
                st.set_microphone_level(value["microphone"].as_f64().unwrap_or(0.) as f32);
                st.set_system_level(value["system"].as_f64().unwrap_or(0.) as f32);
                st.set_recording_health(
                    if value["clipping"] == true {
                        "Audio is clipping. Lower the input level."
                    } else if value["microphone_connected"] == false
                        && value["system_connected"] == false
                    {
                        "Waiting for audio from the selected sources."
                    } else {
                        ""
                    }
                    .into(),
                );
            }
            "audio" => {
                self.position = value["position"].as_f64().unwrap_or(self.position);
                let playing = value["playing"].as_bool().unwrap_or(false);
                if playing {
                    match self.playback_filter.next_position(self.position) {
                        Some(next) if next > self.position + 0.001 => {
                            self.position = next;
                            self.request(
                                ui,
                                "native_audio",
                                json!({"action":"seek", "value":next}),
                                "audio",
                                "",
                            );
                        }
                        None => {
                            self.request(
                                ui,
                                "native_audio",
                                json!({"action":"pause"}),
                                "audio",
                                "",
                            );
                            self.tick_timer.stop();
                            st.set_playing(false);
                            return;
                        }
                        _ => {}
                    }
                }
                st.set_playing(playing);
                self.update_speaking(ui);
                self.update_playback_passage(ui);
                st.set_clock(time(self.position, true).into());
                st.set_playhead((self.position / self.duration.max(1.0)) as f32);
                if !playing {
                    self.tick_timer.stop();
                }
            }
            "audio-start" => {
                self.audio_loaded = true;
                st.set_playing(true);
                self.start_playback_clock(ui);
            }
            "generation" => {
                self.process_id = Some(text(&value, "process_id"));
                st.set_notes_generating(true);
                st.set_busy(true);
                st.set_notes_status("Generating notes…".into());
                self.poll_summary(ui);
            }
            "generation-poll" => {
                if context == self.active {
                    let status = text(&value, "status");
                    if matches!(
                        status.as_str(),
                        "completed" | "failed" | "cancelled" | "error"
                    ) {
                        st.set_busy(false);
                        st.set_notes_generating(false);
                        if status == "completed" {
                            self.drafts.clear_saved(context);
                            if let Some(meeting) = self.cache.get_mut(context) {
                                meeting.summary = Some(value);
                                meeting.lane_images = None;
                            }
                            self.render_meeting(ui);
                            self.render_timeline(ui);
                            self.select_default_note(ui);
                        } else {
                            if status != "cancelled" {
                                st.set_error(text(&value, "error").into());
                            }
                            st.set_notes_status(
                                if status == "cancelled" {
                                    "Generation cancelled · previous notes kept"
                                } else {
                                    "Generation failed · previous notes kept"
                                }
                                .into(),
                            );
                        }
                    } else {
                        self.poll_summary(ui);
                    }
                }
            }
            "question" => {
                st.set_busy(false);
                st.set_loading(false);
                self.show_answer(ui, &value);
            }
            "answers" => {
                self.answers = array(&value);
                let choices = self
                    .answers
                    .iter()
                    .map(|answer| (text(answer, "id"), text(answer, "question")))
                    .collect();
                self.choices(ui, "saved-answer", "Saved answers", choices, Value::Null);
            }
            "answer-deleted" => {
                st.set_answer_id("".into());
                st.set_answer_text("".into());
                st.set_message("Saved answer deleted".into());
            }
            "mutation" => {
                st.set_modal("".into());
                st.set_message("Saved".into());
                self.refresh_list(ui);
                if !self.active.is_empty() {
                    let id = self.active.clone();
                    self.cache.remove(&id);
                    self.recent.retain(|v| v != &id);
                    self.prefetch(ui, &id);
                    self.request(
                        ui,
                        "api_get_summary",
                        json!({"meetingId":id}),
                        "summary",
                        &id,
                    );
                    self.request(
                        ui,
                        "meeting_timeline",
                        json!({"meetingId":id}),
                        "timeline",
                        &id,
                    );
                    if st.get_meeting_tab() == 2 {
                        self.request(
                            ui,
                            "list_meeting_speakers",
                            json!({"meetingId":id}),
                            "speakers",
                            "",
                        );
                    }
                }
                match st.get_view() {
                    6 => self.request(ui, "workspace_tasks", json!({}), "tasks", ""),
                    7 => self.request(ui, "workspace_calendar_events", json!({}), "calendar", ""),
                    5 => {
                        self.results.retain(|v| text(v, "meeting_id") != context);
                        self.render_results(ui);
                    }
                    _ => {}
                }
            }
            "picked-recording-folder" => {
                if value.is_string() {
                    self.request(
                        ui,
                        "get_recording_preferences",
                        json!({}),
                        "setting",
                        "get_recording_preferences",
                    );
                }
            }
            "picked-calendar" => {
                if let Some(path) = value.as_str() {
                    self.request(
                        ui,
                        "workspace_import_calendar",
                        json!({"path":path}),
                        "mutation",
                        "",
                    );
                }
            }
            "picked-import" => {
                if let Some(path) = value["path"].as_str() {
                    self.request(
                        ui,
                        "start_import_audio_command",
                        json!({"sourcePath":path,"title":text(&value,"filename")}),
                        "mutation",
                        "",
                    );
                }
            }
            "show-json" => {
                self.dialog = (context.into(), json!({}));
                st.set_modal("json".into());
                st.set_modal_title("Configuration".into());
                st.set_modal_value(
                    serde_json::to_string_pretty(&value)
                        .unwrap_or_default()
                        .into(),
                );
            }
            _ => {}
        }
    }
}
