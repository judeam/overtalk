//! Speaker lanes, waveform images and timeline markers.

use super::*;

impl Controller {
    pub(super) fn render_timeline(&mut self, ui: &AppWindow) {
        let scale = ui.global::<Theme>().get_scale();
        let window_width = ui.global::<State>().get_window_width();
        let sidebar = if window_width >= 2300.0 {
            300.0
        } else if window_width >= 1280.0 {
            264.0
        } else {
            232.0
        };
        let notes_mode = ui.global::<State>().get_meeting_tab() == 1;
        self.lane_width = (window_width
            - (sidebar + 48.0 + if notes_mode { 216.0 } else { 160.0 }) * scale)
            .clamp(1.0, 7680.0) as usize;
        let Some(meeting) = self.cache.get_mut(&self.active) else {
            return;
        };
        let Some(timeline) = &meeting.timeline else {
            ui.global::<State>().set_lanes(model(Vec::new()));
            return;
        };
        self.duration = timeline["duration"].as_f64().unwrap_or(1.0).max(1.0);
        ui.global::<State>()
            .set_timeline_duration(self.duration as f32);
        let major = [
            5.0, 10.0, 15.0, 30.0, 60.0, 120.0, 300.0, 600.0, 900.0, 1800.0, 3600.0,
        ]
        .into_iter()
        .find(|step| step * self.lane_width as f64 / self.duration >= 110.0)
        .unwrap_or(3600.0);
        ui.global::<State>().set_ruler_ticks(model(
            (0..=(self.duration / (major / 2.0)).floor().min(4096.0) as usize)
                .map(|index| {
                    let seconds = index as f64 * major / 2.0;
                    TickRow {
                        position: (seconds / self.duration) as f32,
                        label: if index % 2 == 0 {
                            time(seconds, false).into()
                        } else {
                            "".into()
                        },
                    }
                })
                .collect(),
        ));
        let scale = ui.global::<Theme>().get_scale();
        let height = ((if notes_mode {
            28.0
        } else if ui.global::<State>().get_window_height() <= 800.0 {
            40.0
        } else {
            64.0
        }) * scale)
            .round() as u32;
        let dark = ui.global::<State>().get_dark();
        if let Some((width, cached_height, cached_dark, rows)) = &meeting.lane_images {
            if *width == self.lane_width && *cached_height == height && *cached_dark == dark {
                ui.global::<State>().set_lanes(rows.clone());
                self.update_speaker_filter(ui);
                return;
            }
        }
        let speakers = array(&timeline["speakers"]);
        let turns = array(&timeline["turns"]);
        let mut rows = Vec::new();
        for (index, speaker) in speakers.iter().enumerate() {
            let color = tone(index);
            let id = text(speaker, "id");
            let spans: Vec<_> = turns
                .iter()
                .filter(|turn| text(turn, "speaker_id") == id)
                .map(|turn| {
                    let start = turn["start"].as_f64().unwrap_or(0.0);
                    (start, turn["end"].as_f64().unwrap_or(start))
                })
                .collect();
            let waveform = crate::waveform::render(
                crate::waveform::RenderOptions {
                    width: self.lane_width,
                    height,
                    scale,
                    duration: self.duration,
                    index,
                    dark,
                },
                &spans,
                &meeting.peaks,
            );
            rows.push(LaneRow {
                id: id.into(),
                name: text(speaker, "name").into(),
                tone: color,
                waveform,
                ..Default::default()
            });
        }
        if let Some(summary) = meeting.summary.as_ref() {
            let notes =
                crate::timeline::note_clips(&summary_markdown(summary), &timeline["sources"]);
            for (kind, name, color) in [
                (1, "Decisions", Color::from_rgb_u8(239, 181, 2)),
                (2, "Actions", Color::from_rgb_u8(144, 104, 205)),
            ] {
                rows.push(LaneRow {
                    id: format!("note-{kind}").into(),
                    name: name.into(),
                    tone: color,
                    kind,
                    clips: model(
                        crate::timeline::group_notes(
                            &notes,
                            kind,
                            self.duration,
                            self.lane_width as f64,
                            scale as f64,
                        )
                        .into_iter()
                        .map(|group| {
                            let first = group.notes[0];
                            let count = group.notes.len();
                            ClipRow {
                                id: first.id.clone().into(),
                                label: if count == 1 {
                                    first.label.clone()
                                } else {
                                    format!("{} {}", count, name.to_lowercase())
                                }
                                .into(),
                                body: group
                                    .notes
                                    .iter()
                                    .map(|n| n.body.as_str())
                                    .collect::<Vec<_>>()
                                    .join("\n\n")
                                    .into(),
                                start: first.start as f32,
                                end: first.end as f32,
                                position: group.position as f32,
                                span: group.span as f32,
                                members: serde_json::to_string(
                                    &group.notes.iter().map(|n| &n.id).collect::<Vec<_>>(),
                                )
                                .unwrap_or_default()
                                .into(),
                                count: count as i32,
                            }
                        })
                        .collect(),
                    ),
                    ..Default::default()
                });
            }
        }
        let rows = model(rows);
        ui.global::<State>().set_lanes(rows.clone());
        meeting.lane_images = Some((self.lane_width, height, dark, rows));
        self.update_speaker_filter(ui);
    }
}
