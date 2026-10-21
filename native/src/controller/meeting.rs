//! Meeting cache, incremental transcripts and selection.

use super::*;

impl Controller {
    pub(super) fn prefetch(&mut self, ui: &AppWindow, id: &str) {
        self.recent.retain(|old| old != id);
        self.recent.push_back(id.into());
        if self.cache.contains_key(id) {
            return;
        }
        self.cache.insert(id.into(), Meeting::default());
        while self.cache.len() > 6 {
            if let Some(old) = self.recent.pop_front() {
                if old != self.active {
                    self.cache.remove(&old);
                } else {
                    self.recent.push_back(old);
                }
            } else {
                break;
            }
        }
        self.request(
            ui,
            "api_get_meeting_metadata",
            json!({"meetingId":id}),
            "metadata",
            id,
        );
        self.request(
            ui,
            "api_get_meeting_transcripts",
            json!({"meetingId":id,"limit":100,"offset":0}),
            "transcripts",
            id,
        );
    }

    pub(super) fn open(&mut self, ui: &AppWindow, id: &str) {
        self.workflow_open(ui, id);
        crate::perf::meeting_opened();
        self.save_notes(ui);
        self.tick_timer.stop();
        self.request(
            ui,
            "native_audio",
            json!({"action":"stop"}),
            "audio-stop",
            "",
        );
        self.active = id.into();
        ui.global::<State>().set_has_video(false);
        self.request(
            ui,
            "workspace_video",
            json!({"action":{"op":"status","meeting_id":id}}),
            "hub-video-presence",
            id,
        );
        self.update_meeting_project(ui);
        self.translations.clear();
        ui.global::<State>().set_reading_mode(0);
        ui.global::<State>().set_reading_ready(false);
        self.selected = None;
        self.position = 0.0;
        self.audio_loaded = false;
        self.solo_speaker = None;
        self.muted_speakers.clear();
        self.playback_filter.clear();
        let st = ui.global::<State>();
        self.note_edit_base = None;
        st.set_note_edit_index(-1);
        st.set_notes_context("".into());
        st.set_notes_language_code(st.get_default_summary_language());
        st.set_notes_language_index(
            ["auto", "en", "pt", "de", "es", "fr"]
                .iter()
                .position(|code| *code == st.get_notes_language_code().as_str())
                .unwrap_or(6) as i32,
        );
        st.set_view(1);
        st.set_selected_id(id.into());
        st.set_meeting_tab(0);
        st.set_playing(false);
        st.set_busy(false);
        st.set_notes_generating(false);
        st.set_speaking_id("".into());
        st.set_clock(time(0.0, true).into());
        st.set_query("".into());
        st.set_error("".into());
        st.set_transcript_scroll_y(0.0);
        st.set_active_transcript_index(-1);
        st.set_active_transcript_id("".into());
        st.set_selected_note_id("".into());
        st.set_inspector_position(0.0);
        st.set_playhead(0.0);
        st.set_markers(model(Vec::new()));
        st.set_meeting_notice("".into());
        st.set_inspector_title("Meeting details".into());
        st.set_inspector_body(
            "Select a passage or a moment in the timeline to review it here.".into(),
        );
        self.prefetch(ui, id);
        self.render_meeting(ui);
        self.render_timeline(ui);
        self.select_default_note(ui);
        if self.cache.get(id).is_some_and(|m| m.summary.is_none()) {
            self.request(
                ui,
                "api_get_summary",
                json!({"meetingId":id}),
                "summary",
                id,
            );
        }
        if self.cache.get(id).is_some_and(|m| m.timeline.is_none()) {
            self.request(
                ui,
                "meeting_timeline",
                json!({"meetingId":id}),
                "timeline",
                id,
            );
        }
        if self.cache.get(id).is_some_and(|m| !m.peaks_loaded) {
            self.request(ui, "meeting_waveform", json!({"meetingId":id}), "peaks", id);
        }
        self.request(ui, "meeting_marks", json!({"meetingId":id}), "marks", id);
        self.request(
            ui,
            "api_get_meeting_summary_language",
            json!({"meetingId":id}),
            "notes-language",
            id,
        );
    }

    pub(super) fn reload_active(&mut self, ui: &AppWindow) {
        if self.active.is_empty() {
            return;
        }
        let id = self.active.clone();
        self.cache.remove(&id);
        self.prefetch(ui, &id);
        self.request(
            ui,
            "api_get_summary",
            json!({"meetingId": id}),
            "summary",
            &id,
        );
        self.request(ui, "meeting_marks", json!({"meetingId":id}), "marks", &id);
        self.request(
            ui,
            "meeting_timeline",
            json!({"meetingId": id}),
            "timeline",
            &id,
        );
        self.request(
            ui,
            "meeting_waveform",
            json!({"meetingId": id}),
            "peaks",
            &id,
        );
    }

    pub(super) fn render_meeting(&mut self, ui: &AppWindow) {
        let st = ui.global::<State>();
        let Some(meeting) = self.cache.get(&self.active) else {
            return;
        };
        let metadata = meeting
            .metadata
            .as_ref()
            .or_else(|| self.overviews.iter().find(|v| text(v, "id") == self.active));
        if let Some(metadata) = metadata {
            st.set_title(text(metadata, "title").into());
            let overview = self.overviews.iter().find(|v| text(v, "id") == self.active);
            let duration = overview.and_then(|v| v["duration"].as_f64()).unwrap_or(0.0);
            let mut people = overview.map(|v| array(&v["speakers"])).unwrap_or_default();
            if people.is_empty() {
                people = meeting
                    .timeline
                    .as_ref()
                    .map(|v| array(&v["speakers"]))
                    .unwrap_or_default();
            }
            if people.is_empty() {
                let mut seen = HashSet::new();
                people = meeting
                    .lines
                    .as_deref()
                    .unwrap_or_default()
                    .iter()
                    .filter_map(|line| {
                        let name = text(line, "speaker_name");
                        (!name.is_empty() && seen.insert(name.clone()))
                            .then(|| json!({"name":name}))
                    })
                    .collect();
            }
            let names = people
                .iter()
                .map(|v| text(v, "name"))
                .collect::<Vec<_>>()
                .join(", ");
            st.set_meeting_date(short_date(&text(metadata, "created_at")).into());
            st.set_meeting_duration(time(duration, false).into());
            st.set_meeting_people(format!("{} speakers: {}", people.len(), names).into());
            st.set_meeting_notice(text(metadata, "recording_notice").into());
            st.set_subtitle(
                format!(
                    "{}  ·  {}  ·{}{} speakers: {}",
                    short_date(&text(metadata, "created_at")),
                    time(duration, false),
                    if st.get_window_width() < 1440.0 {
                        "\n"
                    } else {
                        "  "
                    },
                    people.len(),
                    names
                )
                .into(),
            );
        }
        st.set_loading(meeting.lines.is_none());
        st.set_has_devanagari(
            meeting
                .lines
                .as_deref()
                .unwrap_or_default()
                .iter()
                .any(|line| crate::transliteration::contains_devanagari(&text(line, "text"))),
        );
        st.set_has_more(meeting.lines.as_ref().map_or(0, Vec::len) < meeting.total);
        let rows = meeting.rows.clone().unwrap_or_else(|| {
            model(self.transcript_rows(
                meeting.lines.as_deref().unwrap_or_default(),
                st.get_dark(),
                st.get_latin_devanagari(),
            ))
        });
        st.set_transcripts(rows.clone());
        if let Some(meeting) = self.cache.get_mut(&self.active) {
            meeting.rows = Some(rows);
        }
        self.update_playback_passage(ui);
        self.render_notes(ui);
    }

    pub(super) fn transcript_rows(
        &self,
        lines: &[Value],
        dark: bool,
        latin: bool,
    ) -> Vec<TranscriptRow> {
        let speakers = self
            .cache
            .get(&self.active)
            .and_then(|m| m.timeline.as_ref())
            .map(|v| array(&v["speakers"]))
            .unwrap_or_default();
        lines
            .iter()
            .map(|line| {
                let speaker_id = text(line, "speaker_id");
                let index = speakers
                    .iter()
                    .position(|v| text(v, "id") == speaker_id)
                    .unwrap_or(0);
                TranscriptRow {
                    id: text(line, "id").into(),
                    stamp: time(
                        line["audio_start_time"]
                            .as_f64()
                            .or_else(|| line["start_time"].as_f64())
                            .unwrap_or(0.0),
                        false,
                    )
                    .into(),
                    speaker: if text(line, "speaker_name").is_empty() {
                        "Unassigned".into()
                    } else {
                        text(line, "speaker_name").into()
                    },
                    translation: self
                        .translations
                        .get(&text(line, "id"))
                        .cloned()
                        .unwrap_or_default()
                        .into(),
                    body: if latin && self.translations.is_empty() {
                        crate::transliteration::latin(&text(line, "text")).into()
                    } else {
                        text(line, "text").into()
                    },
                    tone: tone(index),
                    ink: crate::waveform::ink(index, dark),
                }
            })
            .collect()
    }

    pub(crate) fn refresh_style(&mut self, ui: &AppWindow) {
        for meeting in self.cache.values_mut() {
            meeting.rows = None;
        }
        self.render_meeting(ui);
        self.render_timeline(ui);
    }

    pub(super) fn load_more(&mut self, ui: &AppWindow) {
        let Some(meeting) = self.cache.get_mut(&self.active) else {
            return;
        };
        let count = meeting.lines.as_ref().map_or(0, Vec::len);
        if meeting.loading_more || count >= meeting.total {
            return;
        }
        meeting.loading_more = true;
        self.request(
            ui,
            "api_get_meeting_transcripts",
            json!({"meetingId":self.active,"offset":count,"limit":100}),
            "more",
            &self.active,
        );
    }
}
