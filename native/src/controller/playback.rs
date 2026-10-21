//! Playback filters, active passages and the playback clock.

use super::*;

impl Controller {
    pub(super) fn update_speaker_filter(&mut self, ui: &AppWindow) {
        if self.solo_speaker.is_none() && self.muted_speakers.is_empty() {
            self.playback_filter.clear();
        } else {
            let turns = self
                .cache
                .get(&self.active)
                .and_then(|m| m.timeline.as_ref())
                .map(|timeline| array(&timeline["turns"]))
                .unwrap_or_default();
            self.playback_filter.set(
                turns
                    .iter()
                    .filter(|turn| {
                        let id = text(turn, "speaker_id");
                        self.solo_speaker.as_ref().is_none_or(|solo| solo == &id)
                            && !self.muted_speakers.contains(&id)
                    })
                    .map(|turn| {
                        (
                            turn["start"].as_f64().unwrap_or(0.0),
                            turn["end"].as_f64().unwrap_or(0.0),
                        )
                    })
                    .collect(),
            );
        }
        let rows = ui
            .global::<State>()
            .get_lanes()
            .iter()
            .map(|mut lane| {
                lane.solo = self.solo_speaker.as_deref() == Some(lane.id.as_str());
                lane.muted = lane.kind == 0
                    && (self.muted_speakers.contains(lane.id.as_str())
                        || self
                            .solo_speaker
                            .as_ref()
                            .is_some_and(|id| id != lane.id.as_str()));
                lane
            })
            .collect();
        ui.global::<State>().set_lanes(model(rows));
    }

    // Binary search keeps playback ticks independent of the meeting length.
    pub(super) fn update_playback_passage(&mut self, ui: &AppWindow) {
        let st = ui.global::<State>();
        let Some(meeting) = self.cache.get(&self.active) else {
            return;
        };
        let lines = meeting.lines.as_deref().unwrap_or_default();
        let index = crate::playback::passage_at(lines, self.position);
        st.set_active_transcript_index(index.map(|i| i as i32).unwrap_or(-1));
        st.set_active_transcript_id(
            index
                .map(|i| text(&lines[i], "id"))
                .unwrap_or_default()
                .into(),
        );
        st.set_active_word_start(-1);
        st.set_active_word_end(-1);
        if st.get_follow_playback() && st.get_query().is_empty() {
            if let Some((line_index, row)) =
                index.and_then(|i| st.get_transcripts().row_data(i).map(|row| (i, row)))
            {
                let line = &lines[line_index];
                let id = text(line, "id");
                if row.id == id {
                    let start = crate::playback::passage_start(line);
                    let end = line["audio_end_time"]
                        .as_f64()
                        .or_else(|| {
                            lines
                                .get(line_index + 1)
                                .map(crate::playback::passage_start)
                        })
                        .unwrap_or(start);
                    if self
                        .word_track
                        .as_ref()
                        .is_none_or(|(cached_id, body, track)| {
                            *cached_id != id
                                || body != row.body.as_str()
                                || !track.covers(start, end)
                        })
                    {
                        self.word_track = Some((
                            id,
                            row.body.to_string(),
                            crate::playback::WordTrack::new(&row.body, start, end),
                        ));
                    }
                    if let Some((_, _, track)) = &self.word_track {
                        if let Some((start, end)) = track.at(self.position) {
                            st.set_active_word_start(start as i32);
                            st.set_active_word_end(end as i32);
                        }
                    }
                }
            }
        }
        let needs_more = st.get_follow_playback()
            && st.get_query().is_empty()
            && !meeting.loading_more
            && lines.len() < meeting.total
            && lines.last().is_some_and(|line| {
                line["audio_end_time"]
                    .as_f64()
                    .unwrap_or_else(|| crate::playback::passage_start(line))
                    <= self.position
            });
        if needs_more {
            self.load_more(ui);
        }
    }

    pub(super) fn update_speaking(&self, ui: &AppWindow) {
        let speaker = self
            .cache
            .get(&self.active)
            .and_then(|m| m.timeline.as_ref())
            .and_then(|timeline| timeline["turns"].as_array())
            .and_then(|turns| {
                turns.iter().find(|turn| {
                    turn["start"].as_f64().unwrap_or(f64::MAX) <= self.position
                        && turn["end"].as_f64().unwrap_or(0.0) > self.position
                })
            })
            .map(|turn| text(turn, "speaker_id"))
            .unwrap_or_default();
        ui.global::<State>().set_speaking_id(speaker.into());
    }

    pub(super) fn start_playback_clock(&self, ui: &AppWindow) {
        let weak = ui.as_weak();
        self.tick_timer.start(
            slint::TimerMode::Repeated,
            Duration::from_millis(50),
            move || {
                if let Some(ui) = weak.upgrade() {
                    with_controller(|c| {
                        c.request(&ui, "native_audio", json!({"action":"status"}), "audio", "")
                    });
                }
            },
        );
    }
}
