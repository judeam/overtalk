//! Notes rendering, source selection and draft persistence.

use super::*;

impl Controller {
    pub(super) fn render_notes(&self, ui: &AppWindow) {
        let st = ui.global::<State>();
        let Some(meeting) = self.cache.get(&self.active) else {
            return;
        };
        if let Some(notes) = self.drafts.unsaved(&self.active) {
            st.set_notes(notes.into());
        } else {
            match meeting
                .summary
                .as_ref()
                .map(|summary| overtalk_documents::notes_markdown(&summary["data"]))
                .transpose()
            {
                Ok(notes) => st.set_notes(notes.unwrap_or_default().into()),
                Err(error) => {
                    st.set_notes("".into());
                    st.set_notes_status(error.clone().into());
                    st.set_error(error.into());
                    st.set_notes_preview(slint::StyledText::from_plain_text(""));
                    st.set_note_blocks(model(Vec::new()));
                    st.set_notes_highlights(model(Vec::new()));
                    return;
                }
            }
            st.set_notes_status(
                if meeting.summary.is_none() {
                    "Loading saved notes…"
                } else {
                    "All changes saved"
                }
                .into(),
            );
        }
        st.set_notes_preview(crate::notes::preview(&st.get_notes()));
        st.set_notes_highlights(model(
            crate::timeline::note_clips(
                &st.get_notes(),
                &meeting.timeline.as_ref().unwrap_or(&Value::Null)["sources"],
            )
            .into_iter()
            .map(|note| NoteHighlight {
                id: note.id.into(),
                body: note.body.into(),
                stamp: time(note.start, false).into(),
                kind: note.kind,
            })
            .collect(),
        ));
        st.set_note_blocks(model(
            crate::notes::blocks(&st.get_notes())
                .into_iter()
                .enumerate()
                .map(|(index, block)| NoteBlock {
                    id: index as i32,
                    kind: block.kind,
                    text: block.text.clone().into(),
                    rich: crate::notes::preview(&block.text),
                    table: model(if block.kind == 10 {
                        crate::notes::table_rows(&block.text)
                            .into_iter()
                            .map(|cells| crate::NoteTableRow {
                                cells: model(cells.into_iter().map(Into::into).collect()),
                            })
                            .collect()
                    } else {
                        Vec::new()
                    }),
                })
                .collect(),
        ));
    }

    pub(super) fn select_default_note(&mut self, ui: &AppWindow) {
        if self.selected.is_some() {
            return;
        }
        let id = self.cache.get(&self.active).and_then(|meeting| {
            Some(
                crate::timeline::note_clips(
                    &summary_markdown(meeting.summary.as_ref()?),
                    &meeting.timeline.as_ref()?["sources"],
                )
                .into_iter()
                .find(|note| note.kind == 1)?
                .id,
            )
        });
        if let Some(id) = id {
            self.select_note(ui, &id);
        }
    }

    pub(super) fn select_note(&mut self, ui: &AppWindow, id: &str) {
        let Some(meeting) = self.cache.get(&self.active) else {
            return;
        };
        let Some(timeline) = &meeting.timeline else {
            return;
        };
        let Some(summary) = &meeting.summary else {
            return;
        };
        let Some(note) =
            crate::timeline::note_clips(&summary_markdown(summary), &timeline["sources"])
                .into_iter()
                .find(|note| note.id == id)
        else {
            return;
        };
        let evidence: Vec<_> = meeting
            .lines
            .as_deref()
            .unwrap_or_default()
            .iter()
            .filter(|line| {
                let at = line["audio_start_time"].as_f64().unwrap_or(-1.0);
                at >= note.start && at <= note.end
            })
            .cloned()
            .collect();
        let source_position = evidence
            .last()
            .and_then(|line| line["audio_start_time"].as_f64())
            .unwrap_or(note.start);
        self.selected = Some(json!({"note_id":id}));
        let st = ui.global::<State>();
        st.set_selected_note_id(id.into());
        st.set_inspector_position(source_position as f32);
        st.set_inspector_kind(if note.kind == 1 { "decision" } else { "action" }.into());
        st.set_inspector_stamp(time(source_position, false).into());
        st.set_inspector_title(
            format!(
                "{}  ·  {}",
                if note.kind == 1 { "Decision" } else { "Action" },
                time(source_position, false)
            )
            .into(),
        );
        st.set_inspector_body(note.body.into());
        st.set_inspector_evidence(model(
            evidence
                .iter()
                .map(|line| {
                    let at = line["audio_start_time"].as_f64().unwrap_or(0.0);
                    DetailRow {
                        id: at.to_string().into(),
                        title: format!("Source at {}", time(at, false)).into(),
                        description: text(line, "text").into(),
                        ..Default::default()
                    }
                })
                .collect(),
        ));
        st.set_inspector_owner(
            evidence
                .last()
                .map(|line| text(line, "speaker_name"))
                .unwrap_or_default()
                .into(),
        );
        st.set_inspector_tone(tone(0));
    }

    pub(crate) fn notes_edited(&mut self, ui: &AppWindow, notes: String) {
        if let Some(summary) = self
            .cache
            .get(&self.active)
            .and_then(|m| m.summary.as_ref())
        {
            if let Err(error) = overtalk_documents::notes_markdown(&summary["data"]) {
                ui.global::<State>().set_error(error.into());
                return;
            }
        }
        self.drafts.edit(&self.active, notes.clone());
        ui.global::<State>().set_notes_status("Saving…".into());
        if let Some(meeting) = self.cache.get_mut(&self.active) {
            meeting.summary = Some(json!({"data":{"markdown":notes}}));
            meeting.lane_images = None;
        }
        let weak = ui.as_weak();
        self.save_timer.start(
            slint::TimerMode::SingleShot,
            Duration::from_millis(800),
            move || {
                if let Some(ui) = weak.upgrade() {
                    with_controller(|c| c.save_notes(&ui));
                }
            },
        );
    }

    pub(super) fn save_notes(&mut self, ui: &AppWindow) {
        if self.active.is_empty() {
            return;
        }
        self.save_timer.stop();
        let id = self.active.clone();
        self.save_draft(ui, &id);
    }

    pub(super) fn save_draft(&mut self, ui: &AppWindow, id: &str) {
        if let Some((revision, notes)) = self.drafts.begin_save(id) {
            self.request(
                ui,
                "api_save_meeting_summary",
                json!({"meetingId":id,"summary":{"markdown":notes}}),
                "saved-notes",
                &json!({"id":id,"revision":revision}).to_string(),
            );
        }
    }
    pub(crate) fn request_close(&mut self, ui: &AppWindow) -> bool {
        self.workflow_save_all(ui);
        if ui.global::<State>().get_recording_save_pending() {
            ui.global::<State>().set_error("The completed recording still needs saving. Use Retry saving before closing Overtalk.".into());
            ui.global::<State>().set_view(0);
            return false;
        }
        if ui.global::<State>().get_recording() {
            ui.global::<State>()
                .set_error("Stop the recording before closing Overtalk.".into());
            return false;
        }
        let dirty = self.drafts.dirty_ids();
        if dirty.is_empty() && self.workflow.drafts.dirty_ids().is_empty() {
            return true;
        }
        self.closing = true;
        for id in dirty {
            self.save_draft(ui, &id);
        }
        false
    }
}
