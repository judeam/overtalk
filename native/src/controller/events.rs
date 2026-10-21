//! Live service events and recording/summary timers.

use super::*;

impl Controller {
    pub(super) fn start_recording_clock(&self, ui: &AppWindow) {
        let weak = ui.as_weak();
        self.recording_timer.start(
            slint::TimerMode::Repeated,
            Duration::from_millis(500),
            move || {
                if let Some(ui) = weak.upgrade() {
                    with_controller(|c| {
                        c.request(&ui, "get_recording_state", json!({}), "record-state", "")
                    });
                }
            },
        );
    }
    pub(super) fn poll_summary(&self, ui: &AppWindow) {
        let weak = ui.as_weak();
        let id = self.active.clone();
        slint::Timer::single_shot(Duration::from_millis(600), move || {
            if let Some(ui) = weak.upgrade() {
                with_controller(|c| {
                    c.request(
                        &ui,
                        "api_get_summary",
                        json!({"meetingId":id}),
                        "generation-poll",
                        &id,
                    )
                });
            }
        });
    }

    pub(crate) fn event(&mut self, ui: &AppWindow, event: &str, payload: Value) {
        if event == "workspace-jobs-changed" && !self.active.is_empty() {
            self.workflow_load(ui, &self.active);
            if payload["kind"] == "notes"
                && payload["state"] == "completed"
                && payload["meeting_id"] == self.active
            {
                self.reload_active(ui);
            }
        }
        let st = ui.global::<State>();
        match event {
            "library-data-changed" => {
                self.library_dirty = true;
                if !st.get_recording() && !st.get_transcribing() && !st.get_notes_generating() {
                    self.library_dirty = false;
                    let active = self.active.clone();
                    self.cache.retain(|id, _| id == &active);
                    self.refresh_list(ui);
                    self.reload_active(ui);
                } else if !self.library_refresh_pending {
                    // Defer during an active job without losing the update. The
                    // timer stops once that update has been consumed.
                    self.library_refresh_pending = true;
                    let weak = ui.as_weak();
                    slint::Timer::single_shot(Duration::from_secs(1), move || {
                        if let Some(ui) = weak.upgrade() {
                            with_controller(|c| {
                                c.library_refresh_pending = false;
                                if c.library_dirty {
                                    c.event(&ui, "library-data-changed", Value::Null);
                                }
                            });
                        }
                    });
                }
            }
            "engine-resync" => {
                self.refresh_list(ui);
                if st.get_recording() {
                    self.request(
                        ui,
                        "get_transcript_history",
                        json!({}),
                        "record-history",
                        "",
                    );
                }
                if !self.active.is_empty() {
                    let id = self.active.clone();
                    self.request(
                        ui,
                        "api_get_meeting_transcripts",
                        json!({"meetingId":id,"offset":0,"limit":100}),
                        "transcripts",
                        &id,
                    );
                }
            }
            "engine-disconnected" => {
                st.set_engine_ready(false);
                st.set_error("The meeting engine stopped. Reopen Overtalk to reconnect.".into());
            }
            "recording-stopped" => {
                st.set_recording(false);
                st.set_recording_paused(false);
                self.recording_timer.stop();
            }
            "recording-paused" => st.set_recording_paused(true),
            "recording-resumed" => st.set_recording_paused(false),
            "retranscription-complete" | "transcription-complete" | "speech-analysis-finished" => {
                st.set_transcribing(false);
                st.set_message("Transcript updated".into());
                self.refresh_list(ui);
                self.reload_active(ui);
            }
            "audio-levels" => {
                for level in array(&payload["levels"]) {
                    let peak = level["peak_level"].as_f64().unwrap_or(0.).min(1.) as f32;
                    if level["device_type"] == "input" {
                        st.set_microphone_level(peak);
                    } else {
                        st.set_system_level(peak);
                    }
                }
                let peak = array(&payload["levels"])
                    .iter()
                    .filter_map(|v| v["peak_level"].as_f64())
                    .fold(0.0_f64, f64::max);
                st.set_audio_level(peak.min(1.0) as f32);
            }
            "meeting-updated"
            | "meeting-speakers-updated"
            | "workspace-meetings-changed"
            | "recording-saved"
            | "import-complete" => self.refresh_list(ui),
            "workspace-reminder" => {
                st.set_message(format!("Reminder: {}", text(&payload, "title")).into())
            }
            "workspace-video-status" => {
                if payload["ready"] == true {
                    st.set_video_recording(true);
                    st.set_video_starting(false);
                }
                if let Some(error) = payload["error"].as_str() {
                    st.set_video_recording(false);
                    st.set_video_starting(false);
                    st.set_error(error.into());
                }
                if payload["stopped"] == true || payload["completed"] == true {
                    st.set_video_recording(false);
                    st.set_video_starting(false);
                }
            }
            "workspace-meeting-detected" => {
                if payload.is_null() {
                    if !st.get_recording() {
                        st.set_widget_visible(false);
                    }
                } else if st.get_watch_enabled() {
                    st.set_widget_title(text(&payload, "title").into());
                    st.set_widget_description(text(&payload, "description").into());
                    st.set_widget_url(text(&payload, "url").into());
                    st.set_widget_visible(payload["show_widget"] == true);
                }
            }
            "workspace-projects-changed" => {
                self.request(
                    ui,
                    "workspace_project",
                    json!({"action":{"op":"list"}}),
                    "hub-projects",
                    "",
                );
                if st.get_view() == 8 {
                    self.hub_refresh(ui);
                }
                self.workflow_calendar_reload(ui);
            }
            // The engine syncs calendars itself; show what changed without syncing again.
            "workspace-calendar-changed" => self.workflow_calendar_reload(ui),
            "workspace-brief-ready" => self.automation_brief_ready(ui, &payload),
            "workspace-jobs-changed" => {
                if st.get_view() == 8 {
                    self.hub_refresh(ui);
                }
                if payload["kind"] == "translate"
                    && payload["state"] == "completed"
                    && payload["meeting_id"] == self.active
                {
                    self.request(ui,"workspace_reading",json!({"meetingId":self.active,"language":st.get_reading_language().as_str()}),"hub-reading-data",&self.active);
                }
            }
            "recording-error"
            | "import-error"
            | "transcription-error"
            | "retranscription-error" => {
                st.set_transcribing(false);
                if payload["save_pending"].as_bool().unwrap_or(false) {
                    st.set_recording_save_pending(true);
                }
                st.set_busy(false);
                st.set_error(
                    payload
                        .as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| text(&payload, "message"))
                        .into(),
                );
            }
            "transcript-update" => {
                if st.get_recording() {
                    self.upsert_live_transcript(ui, &payload);
                }
            }
            // A recording that started before its speech model: say what happens next.
            "recording-speech-notice" => {
                let message = text(&payload, "message");
                if !message.is_empty() {
                    st.set_message(message.into());
                }
            }
            "parakeet-model-download-progress" | "builtin-ai-download-progress" => {
                let speech = event == "parakeet-model-download-progress";
                let percent = payload["progress"].as_f64().unwrap_or(0.0);
                let total = payload["total_mb"].as_f64().filter(|mb| *mb > 0.0);
                let status = text(&payload, "status");
                let what = if speech {
                    "the speech model"
                } else {
                    "the notes model"
                };
                let line = match (status.as_str(), total) {
                    ("completed", _) => format!("Downloaded {what}"),
                    ("error", _) => format!("Could not download {what}. Try again in Settings."),
                    (_, Some(total)) => {
                        format!("Downloading {what} · {percent:.0}% of {total:.0} MB")
                    }
                    _ => format!("Downloading {what} · {percent:.0}%"),
                };
                st.set_message(line.clone().into());
                if speech {
                    st.set_setup_speech_detail(line.into());
                } else {
                    st.set_setup_notes_detail(line.into());
                }
                // A finished download changes readiness; check it again.
                if status == "completed" && st.get_setup_needed() {
                    self.workflow_action(ui, "workflow-check", "");
                }
            }
            "parakeet-model-download-complete" => {
                if st.get_setup_needed() {
                    self.workflow_action(ui, "workflow-check", "");
                }
            }
            event if event.ends_with("progress") => {
                let message = text(&payload, "message");
                let progress = payload["progress"]
                    .as_f64()
                    .or_else(|| payload["percent"].as_f64());
                st.set_message(
                    if message.is_empty() {
                        progress
                            .map(|p| format!("Processing… {:.0}%", p))
                            .unwrap_or_else(|| "Processing…".into())
                    } else {
                        message
                    }
                    .into(),
                );
            }
            _ => {}
        }
    }

    pub(super) fn upsert_live_transcript(&mut self, ui: &AppWindow, payload: &Value) {
        let Some(sequence) = payload["sequence_id"].as_u64() else {
            return;
        };
        if payload["is_partial"].as_bool().unwrap_or(false)
            && self.live_completed.contains(&sequence)
        {
            return;
        }
        if !payload["is_partial"].as_bool().unwrap_or(false) {
            self.live_completed.insert(sequence);
        }
        let line = json!({"id":format!("seg_{sequence}"),"text":text(payload,"text"),"audio_start_time":payload["audio_start_time"],"speaker_name":text(payload,"speaker_name")});
        let Some(row) = self
            .transcript_rows(
                &[line],
                ui.global::<State>().get_dark(),
                ui.global::<State>().get_latin_devanagari(),
            )
            .pop()
        else {
            return;
        };
        let rows = ui.global::<State>().get_transcripts();
        if let Some(list) = rows.as_any().downcast_ref::<VecModel<TranscriptRow>>() {
            let existing = list.iter().position(|old| old.id == row.id);
            if let Some(index) = existing {
                list.set_row_data(index, row);
            } else {
                // Live review keeps a bounded tail. The complete transcript stays
                // in the recording engine and is saved independently of this UI.
                if list.row_count() >= 256 {
                    if let Some(old) = list.row_data(0).and_then(|row| {
                        row.id
                            .strip_prefix("seg_")
                            .and_then(|id| id.parse::<u64>().ok())
                    }) {
                        self.live_completed.remove(&old);
                    }
                    list.remove(0);
                }
                list.push(row);
            }
        }
    }
}
