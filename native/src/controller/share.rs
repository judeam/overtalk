//! Share a copy of a meeting as a web page or an .overtalk file, and open one someone
//! shared. The engine builds, redacts and checks both; this module collects the
//! person's choices. Sending notes to a connected tool is a separate flow (hub-share-*).
use super::*;

/// Time ranges such as `00:12–00:24` or `1:02:03-1:02:10`, separated by commas or lines.
pub(super) fn ranges(input: &str) -> Result<Vec<[f64; 2]>, String> {
    let seconds = |part: &str| -> Option<f64> {
        part.trim().split(':').try_fold(0.0, |sum, n| {
            n.trim().parse::<f64>().ok().map(|n| sum * 60. + n)
        })
    };
    input
        .split([',', '\n'])
        .filter(|range| !range.trim().is_empty())
        .map(|range| {
            let (start, end) = range
                .split_once('–')
                .or_else(|| range.split_once('-'))
                .ok_or("Use time ranges such as 00:12–00:24")?;
            match (seconds(start), seconds(end)) {
                (Some(start), Some(end)) if end > start => Ok([start, end]),
                _ => Err("Use time ranges such as 00:12–00:24".to_owned()),
            }
        })
        .collect()
}

fn size(bytes: u64) -> String {
    if bytes < 1024 * 1024 {
        format!("{} KB", (bytes / 1024).max(1))
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

/// One line for the dialog: the size before saving and what it depends on.
fn estimate_line(estimate: &Value, clips: bool) -> String {
    let mut line = format!("About {}", size(estimate["bytes"].as_u64().unwrap_or(0)));
    let count = estimate["clips"].as_u64().unwrap_or(0);
    if clips && count > 0 {
        line.push_str(&format!(
            " · {count} audio clip{}",
            if count == 1 { "" } else { "s" }
        ));
    } else if clips {
        line.push_str(" · no clips: the notes cite no moments with audio");
    }
    if estimate["notes"] != true {
        line.push_str(" · without notes");
    }
    line
}

impl Controller {
    pub(super) fn share_action(&mut self, ui: &AppWindow, action: &str, value: &str) {
        let st = ui.global::<State>();
        match action {
            "share-meeting" => {
                if self.active.is_empty() {
                    st.set_error("Open a meeting to share a copy of it.".into());
                    return;
                }
                // The copy includes notes typed a moment ago.
                self.save_notes(ui);
                self.dialog(
                    ui,
                    "share-options",
                    "Share a copy",
                    "Nothing is uploaded. You choose where the copy is saved and who receives it.",
                    "share",
                    json!({"generation":0}),
                );
                st.set_modal_submit("Save copy…".into());
                st.set_share_kind(0);
                st.set_share_audio(0);
                st.set_share_terms("".into());
                st.set_share_contacts(false);
                st.set_share_omit_notes(false);
                st.set_share_ranges("".into());
                self.share_estimate(ui);
            }
            "share-kind" => {
                st.set_share_kind(value.parse().unwrap_or(0));
                st.set_share_audio(0);
                self.share_estimate(ui);
            }
            "share-audio" => {
                st.set_share_audio(value.parse().unwrap_or(0));
                self.share_estimate(ui);
            }
            "share-changed" => self.share_estimate(ui),
            "share-open" => self.request(
                ui,
                "workspace_pick_file",
                json!({"kind":"share-open"}),
                "share-picked-open",
                "",
            ),
            _ => {}
        }
    }

    fn share_options(&self, ui: &AppWindow) -> Result<Value, String> {
        let st = ui.global::<State>();
        let page = st.get_share_kind() == 0;
        let audio = match (page, st.get_share_audio()) {
            (true, 0) => "clips",
            (true, 1) => "full",
            (false, 0) => "original",
            (false, 1) => "compact",
            _ => "none",
        };
        let terms: Vec<String> = st
            .get_share_terms()
            .split([',', '\n'])
            .map(str::trim)
            .filter(|term| !term.is_empty())
            .map(str::to_owned)
            .collect();
        Ok(json!({
            "kind": if page { "page" } else { "file" },
            "audio": audio,
            "redactions": {
                "terms": terms,
                "contacts": st.get_share_contacts(),
                "omit_notes": st.get_share_omit_notes(),
                "spans": ranges(&st.get_share_ranges())?,
            },
        }))
    }

    /// Ask for the size again; only the latest answer is shown.
    fn share_estimate(&mut self, ui: &AppWindow) {
        let st = ui.global::<State>();
        let options = match self.share_options(ui) {
            Ok(options) => options,
            Err(error) => {
                st.set_share_estimate(error.into());
                return;
            }
        };
        let generation = self.dialog.1["generation"].as_u64().unwrap_or(0) + 1;
        self.dialog.1["generation"] = generation.into();
        st.set_share_estimate("Working out the size…".into());
        self.request(
            ui,
            "workspace_share_estimate",
            json!({"meetingId":self.active,"options":options}),
            "share-estimate",
            &generation.to_string(),
        );
    }

    /// The dialog's save button: choose where the copy goes, then make it.
    pub(super) fn share_save(&mut self, ui: &AppWindow) {
        let st = ui.global::<State>();
        let options = match self.share_options(ui) {
            Ok(options) => options,
            Err(error) => {
                st.set_error(error.into());
                return;
            }
        };
        // A redacted copy is not named after what it hides.
        let name = if !st.get_share_terms().trim().is_empty()
            || !st.get_share_ranges().trim().is_empty()
            || st.get_share_contacts()
        {
            "Shared meeting".to_owned()
        } else {
            st.get_title().to_string()
        };
        let kind = if st.get_share_kind() == 0 {
            "share-page"
        } else {
            "share-file"
        };
        let request = json!({"meetingId":self.active,"options":options});
        st.set_modal("".into());
        self.request(
            ui,
            "workspace_pick_file",
            json!({"kind":kind,"name":name}),
            "share-picked-save",
            &request.to_string(),
        );
    }

    pub(super) fn share_reply(
        &mut self,
        ui: &AppWindow,
        tag: &str,
        context: &str,
        result: Result<Value, String>,
    ) {
        let st = ui.global::<State>();
        if tag == "share-estimate" {
            let current = self.dialog.0 == "share-options"
                && self.dialog.1["generation"]
                    .as_u64()
                    .map(|g| g.to_string())
                    .as_deref()
                    == Some(context);
            if current {
                let clips = st.get_share_kind() == 0 && st.get_share_audio() == 0;
                st.set_share_estimate(
                    match result {
                        Ok(estimate) => estimate_line(&estimate, clips),
                        Err(error) => error,
                    }
                    .into(),
                );
            }
            return;
        }
        let value = match result {
            Ok(value) => value,
            Err(error) => {
                st.set_message("".into());
                st.set_error(error.into());
                return;
            }
        };
        match tag {
            "share-picked-save" => {
                let Some(path) = value.as_str() else {
                    return;
                };
                let mut request: Value = serde_json::from_str(context).unwrap_or_default();
                request["path"] = path.into();
                st.set_message("Preparing the copy…".into());
                self.request(ui, "workspace_share_meeting", request, "share-saved", "");
            }
            "share-saved" => {
                let name = std::path::Path::new(&text(&value, "path"))
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                st.set_message(
                    format!(
                        "Saved a copy: {name} ({})",
                        size(value["bytes"].as_u64().unwrap_or(0))
                    )
                    .into(),
                );
            }
            "share-picked-open" => {
                if let Some(path) = value.as_str() {
                    st.set_message("Opening the shared meeting…".into());
                    self.request(
                        ui,
                        "workspace_open_shared",
                        json!({"path":path}),
                        "share-opened",
                        "",
                    );
                }
            }
            "share-opened" => {
                let id = text(&value, "meeting_id");
                self.refresh_list(ui);
                self.action(ui, "open-meeting", &id);
                st.set_message(
                    format!(
                        "Opened “{}” from a shared file{}",
                        text(&value, "title"),
                        if value["audio"] == true {
                            ""
                        } else {
                            " (without audio)"
                        }
                    )
                    .into(),
                );
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_ranges_accept_dashes_hours_and_commas() {
        assert_eq!(
            ranges("00:12–00:24, 1:02:03-1:02:10\n").unwrap(),
            vec![[12.0, 24.0], [3723.0, 3730.0]]
        );
        assert!(ranges("").unwrap().is_empty());
        assert!(ranges("00:24–00:12").is_err());
        assert!(ranges("soon").is_err());
    }

    #[test]
    fn the_estimate_says_what_the_size_depends_on() {
        assert_eq!(
            estimate_line(&json!({"bytes":1_572_864,"clips":3,"notes":true}), true),
            "About 1.5 MB · 3 audio clips"
        );
        assert_eq!(
            estimate_line(&json!({"bytes":2048,"clips":0,"notes":false}), true),
            "About 2 KB · no clips: the notes cite no moments with audio · without notes"
        );
        assert_eq!(
            estimate_line(&json!({"bytes":900,"clips":0,"notes":true}), false),
            "About 1 KB"
        );
    }
}
