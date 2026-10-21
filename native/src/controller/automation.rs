//! Automatic filing, briefs before meetings and the settings behind them.
//! The engine decides and explains each choice; this module presents it and
//! sends the person's overrides, which always win.

use super::*;

#[derive(Default)]
pub(super) struct Automation {
    /// The recorder's project for the next recording. `None` leaves it to the
    /// calendar event or the edition; `Some("")` is an explicit "No project".
    next_project: Option<String>,
    /// The engine's project state for the recording in progress.
    capture: Value,
    settings: Value,
    reminders: Vec<u32>,
    agenda: String,
    sources: Vec<(String, f64)>,
}

const ASSIGN: [&str; 3] = ["confident", "suggest", "off"];
const BRIEFS: [&str; 3] = ["projects", "all", "off"];
const REMINDERS: [u32; 4] = [0, 5, 10, 15];

fn option(id: &str, title: &str, description: &str, icon: &str) -> DetailRow {
    DetailRow {
        id: id.into(),
        title: title.into(),
        description: description.into(),
        value: icon.into(),
        checked: false,
    }
}

fn local_time(input: &str, format: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(input)
        .map(|date| {
            date.with_timezone(&chrono::Local)
                .format(format)
                .to_string()
        })
        .unwrap_or_default()
}

fn brief_item(item: &Value, sources: &mut Vec<(String, f64)>) -> BriefItem {
    let source = &item["source"];
    let meeting = text(source, "meeting_id");
    if meeting.is_empty() {
        return BriefItem {
            text: text(item, "text").into(),
            ..Default::default()
        };
    }
    let start = source["start"].as_f64().unwrap_or(0.0);
    let label = text(source, "label");
    sources.push((meeting, start));
    BriefItem {
        text: text(item, "text").into(),
        source: (sources.len() - 1).to_string().into(),
        source_label: if label.is_empty() {
            format!("Source · {}", time(start, false))
        } else {
            label
        }
        .into(),
    }
}

impl Controller {
    pub(super) fn automation_initialize(&mut self, ui: &AppWindow) {
        let st = ui.global::<State>();
        crate::autostart::repair();
        st.set_autostart_available(crate::autostart::supported());
        st.set_autostart(crate::autostart::enabled());
        self.request(ui, "workspace_edition", json!({}), "automation-edition", "");
    }

    fn project_name(&self, id: &str) -> Option<String> {
        self.projects
            .iter()
            .find(|p| text(p, "id") == id)
            .map(|p| text(p, "name"))
    }

    /// Add the recorder's project choice to the capture options.
    pub(super) fn automation_capture_options(&self, options: &mut Value, sound_check: bool) {
        match self.automation.next_project.as_deref() {
            // A sound check is never filed in a project.
            _ if sound_check => options["project_mode"] = "none".into(),
            Some("") => options["project_mode"] = "none".into(),
            Some(project) => options["project_id"] = project.into(),
            None => {}
        }
    }

    pub(super) fn automation_capture_started(&mut self, ui: &AppWindow, value: &Value) {
        self.automation.capture = value["capture"].clone();
        if !self.sound_check_active {
            self.automation.next_project = None;
        }
        self.automation_render_capture(ui);
    }

    pub(super) fn automation_render_capture(&mut self, ui: &AppWindow) {
        let st = ui.global::<State>();
        let automatic = st.get_edition_auto_assign();
        // A project removed since it was picked falls back to the default.
        if let Some(id) = self.automation.next_project.as_deref() {
            if !id.is_empty() && self.project_name(id).is_none() {
                self.automation.next_project = None;
            }
        }
        st.set_capture_project_next(
            match self.automation.next_project.as_deref() {
                None if automatic => "Decide automatically".into(),
                None | Some("") => "No project".into(),
                Some(id) => clip(&self.project_name(id).unwrap_or_default(), 32),
            }
            .into(),
        );
        let capture = &self.automation.capture;
        let name = clip(&text(capture, "project_name"), 32);
        let reason = text(capture, "project_reason");
        let (label, detail, state) = match text(capture, "project_origin").as_str() {
            "undecided" if automatic => ("Decided after the meeting".into(), String::new(), ""),
            _ if name.is_empty() => ("No project".into(), String::new(), ""),
            "automatic" if reason.is_empty() => (name, "Chosen automatically".into(), "automatic"),
            "automatic" => (
                name,
                format!("Chosen automatically · {reason}"),
                "automatic",
            ),
            _ if reason.is_empty() => (name, "Chosen by you".into(), "manual"),
            _ => (name, reason, "manual"),
        };
        st.set_capture_project(label.into());
        st.set_capture_project_detail(detail.into());
        st.set_capture_project_state(state.into());
    }

    pub(super) fn automation_action(&mut self, ui: &AppWindow, action: &str, value: &str) {
        let st = ui.global::<State>();
        let automatic = st.get_edition_auto_assign();
        match action {
            "automation-capture" => {
                let recording = st.get_recording();
                let capture = self.automation.capture.clone();
                let meeting = text(&capture, "meeting_id");
                if recording && meeting.is_empty() {
                    st.set_error(
                        "This recording cannot change project yet. Try again in a moment.".into(),
                    );
                    return;
                }
                // Nothing is marked while the engine has yet to decide.
                let current = if recording {
                    match text(&capture, "project_id") {
                        id if !id.is_empty() => format!("project:{id}"),
                        _ if text(&capture, "project_origin") == "undecided" => String::new(),
                        _ => "none".into(),
                    }
                } else {
                    match self.automation.next_project.as_deref() {
                        None if automatic => "automatic".into(),
                        None | Some("") => "none".into(),
                        Some(id) => format!("project:{id}"),
                    }
                };
                // The engine can only hand a recording in progress back to
                // automatic filing before it starts, so the option is omitted then.
                let mut rows = Vec::new();
                if automatic && !recording {
                    rows.push(option(
                        "automatic",
                        "Decide automatically",
                        "From the calendar invite, or from the transcript afterwards",
                        "calendar",
                    ));
                }
                rows.push(option("none", "No project", "Keep it unsorted", "close"));
                rows.extend(self.projects.iter().map(|p| {
                    option(
                        &format!("project:{}", text(p, "id")),
                        &text(p, "name"),
                        "",
                        "folder",
                    )
                }));
                for row in &mut rows {
                    if row.id == current.as_str() {
                        row.description =
                            if recording && text(&capture, "project_origin") == "automatic" {
                                format!(
                                    "Current · chosen automatically · {}",
                                    text(&capture, "project_reason")
                                )
                                .trim_end_matches(" · ")
                                .into()
                            } else {
                                "Current choice".into()
                            };
                    }
                }
                self.rich_choices(
                    ui,
                    "automation-capture",
                    if recording {
                        "Project for this recording"
                    } else {
                        "Project for the next recording"
                    },
                    rows,
                    json!({"meeting_id": if recording { meeting } else { String::new() }}),
                );
            }
            "automation-event" => {
                let Some(event) = self
                    .workflow
                    .events
                    .iter()
                    .find(|e| e["id"] == value)
                    .cloned()
                else {
                    return;
                };
                let project = &event["project"];
                let origin = text(project, "origin");
                let reason = text(project, "reason");
                let current = match text(project, "id") {
                    _ if origin != "manual" => if automatic { "automatic" } else { "none" }.into(),
                    id if id.is_empty() => "none".into(),
                    id => format!("project:{id}"),
                };
                let mut rows = Vec::new();
                if automatic {
                    let decided = match origin.as_str() {
                        "automatic" => format!("Current · {} · {reason}", text(project, "name")),
                        "manual" => "From invitees, organisation, title and series".into(),
                        _ => "Current · from invitees, organisation, title and series".into(),
                    };
                    rows.push(option(
                        "automatic",
                        "Decide automatically",
                        decided.trim_end_matches(" · "),
                        "calendar",
                    ));
                }
                rows.push(option("none", "No project", "Keep it unsorted", "close"));
                rows.extend(self.projects.iter().map(|p| {
                    let id = text(p, "id");
                    let suggested = origin == "suggested" && text(project, "id") == id;
                    option(
                        &format!("project:{id}"),
                        &text(p, "name"),
                        &if suggested {
                            format!("Suggested · {reason}")
                        } else {
                            String::new()
                        },
                        if suggested { "check" } else { "folder" },
                    )
                }));
                for row in &mut rows {
                    if row.id == current.as_str() && row.id != "automatic" {
                        row.description = "Current choice".into();
                    }
                }
                self.rich_choices(
                    ui,
                    "automation-event",
                    &format!("Project for {}", clip(&text(&event, "title"), 36)),
                    rows,
                    json!({"event_id": value}),
                );
            }
            "automation-brief" => {
                self.request(
                    ui,
                    "workspace_workflow",
                    json!({"action":{"op":"brief","event_id":value}}),
                    "automation-brief",
                    value,
                );
                self.request(
                    ui,
                    "usage_record",
                    json!({"event":"brief_opened"}),
                    "usage",
                    "",
                );
            }
            "automation-brief-source" => {
                let source = value
                    .parse::<usize>()
                    .ok()
                    .and_then(|index| self.automation.sources.get(index))
                    .cloned();
                if let Some((meeting, start)) = source {
                    self.request(
                        ui,
                        "usage_record",
                        json!({"event":"brief_source_played"}),
                        "usage",
                        "",
                    );
                    st.set_modal("".into());
                    self.dialog = Default::default();
                    self.hub_reply(
                        ui,
                        "hub-fact-source",
                        "",
                        json!({"meeting_id":meeting,"start":start}),
                    );
                }
            }
            "automation-assign" | "automation-briefs" | "automation-reminder" => {
                let index = value.parse::<usize>().unwrap_or(0);
                let mut settings = self.automation.settings.clone();
                if !settings.is_object() {
                    settings = json!({"auto_assign":"confident","briefs":"projects","reminder_minutes":10});
                }
                match action {
                    "automation-assign" => settings["auto_assign"] = ASSIGN[index.min(2)].into(),
                    "automation-briefs" => settings["briefs"] = BRIEFS[index.min(2)].into(),
                    _ => {
                        let reminders = if self.automation.reminders.is_empty() {
                            REMINDERS.to_vec()
                        } else {
                            self.automation.reminders.clone()
                        };
                        settings["reminder_minutes"] =
                            reminders.get(index).copied().unwrap_or(0).into();
                    }
                }
                self.request(
                    ui,
                    "workspace_workflow",
                    json!({"action":{"op":"automation","settings":settings}}),
                    "automation-settings",
                    "",
                );
            }
            "automation-autostart" => {
                let enable = value == "true";
                match crate::autostart::set(enable) {
                    Ok(()) => st.set_message(
                        if enable {
                            "Overtalk will open when you sign in"
                        } else {
                            "Overtalk will no longer open when you sign in"
                        }
                        .into(),
                    ),
                    Err(error) => st
                        .set_error(format!("Could not change the sign-in setting: {error}").into()),
                }
                st.set_autostart(crate::autostart::enabled());
            }
            _ => {}
        }
    }

    pub(super) fn automation_choice(
        &mut self,
        ui: &AppWindow,
        action: &str,
        data: Value,
        value: &str,
    ) {
        let project = value.strip_prefix("project:");
        match action {
            "automation-capture" => {
                let meeting = text(&data, "meeting_id");
                if meeting.is_empty() {
                    self.automation.next_project = match value {
                        "automatic" => None,
                        "none" => Some(String::new()),
                        _ => project.map(str::to_owned),
                    };
                    self.automation_render_capture(ui);
                } else {
                    self.request(
                        ui,
                        "workspace_workflow",
                        json!({"action":{"op":"capture_project","meeting_id":meeting,"project_id":project}}),
                        "automation-captured",
                        &meeting,
                    );
                }
            }
            "automation-event" => self.request(
                ui,
                "workspace_project",
                json!({"action":{"op":"event_project","event_id":data["event_id"],"project_id":project,"automatic":value == "automatic"}}),
                "automation-event-saved",
                "",
            ),
            _ => {}
        }
    }

    pub(super) fn automation_save(&mut self, ui: &AppWindow, action: &str) {
        let st = ui.global::<State>();
        match action {
            "automation-brief" if !self.automation.agenda.is_empty() => {
                ui.invoke_copy_text(self.automation.agenda.clone().into());
                st.set_message("Agenda copied".into());
            }
            _ => {}
        }
    }

    pub(super) fn automation_reply(
        &mut self,
        ui: &AppWindow,
        tag: &str,
        context: &str,
        result: Result<Value, String>,
    ) {
        let st = ui.global::<State>();
        if tag == "automation-edition" {
            // An engine without editions is the open edition.
            let edition = result.unwrap_or(Value::Null);
            let capabilities = array(&edition["capabilities"]);
            let has = |name: &str| capabilities.iter().any(|c| c.as_str() == Some(name));
            st.set_edition_auto_assign(has("auto-assign"));
            st.set_edition_briefs(has("briefs"));
            st.set_edition_name(
                match edition["name"].as_str() {
                    None | Some("open") => "Overtalk".into(),
                    Some(name) => format!("Overtalk {name}"),
                }
                .into(),
            );
            let features = [
                (has("auto-assign"), "automatic filing"),
                (has("briefs"), "briefs before meetings"),
            ]
            .iter()
            .filter(|(on, _)| *on)
            .map(|(_, name)| *name)
            .collect::<Vec<_>>()
            .join(" and ");
            st.set_edition_detail(features.into());
            self.automation_render_capture(ui);
            if has("auto-assign") || has("briefs") {
                self.request(
                    ui,
                    "workspace_workflow",
                    json!({"action":{"op":"automation","settings":null}}),
                    "automation-settings",
                    "",
                );
            }
            return;
        }
        let value = match result {
            Ok(value) => value,
            Err(error) => {
                if tag == "automation-settings" && self.automation.settings.is_object() {
                    self.automation_render_settings(ui, self.automation.settings.clone());
                }
                st.set_error(error.into());
                return;
            }
        };
        match tag {
            "automation-captured" => {
                if st.get_recording() && text(&self.automation.capture, "meeting_id") == context {
                    self.automation.capture = value;
                    self.automation_render_capture(ui);
                }
            }
            "automation-event-saved" => self.workflow_calendar_reload(ui),
            "automation-brief" => self.automation_show_brief(ui, &value),
            "automation-settings" => self.automation_render_settings(ui, value),
            _ => {}
        }
    }

    fn automation_render_settings(&mut self, ui: &AppWindow, settings: Value) {
        let st = ui.global::<State>();
        let index = |options: &[&str], key: &str| {
            options
                .iter()
                .position(|option| *option == text(&settings, key))
                .unwrap_or(0) as i32
        };
        st.set_automation_assign(index(&ASSIGN, "auto_assign"));
        st.set_automation_briefs(index(&BRIEFS, "briefs"));
        let minutes = settings["reminder_minutes"].as_u64().unwrap_or(10) as u32;
        let mut reminders = REMINDERS.to_vec();
        if !reminders.contains(&minutes) {
            reminders.push(minutes);
            reminders.sort_unstable();
        }
        st.set_automation_reminders(model(
            reminders
                .iter()
                .map(|minutes| match minutes {
                    0 => "Off".into(),
                    1 => "1 minute before".into(),
                    _ => format!("{minutes} minutes before").into(),
                })
                .collect(),
        ));
        st.set_automation_reminder(reminders.iter().position(|m| *m == minutes).unwrap_or(0) as i32);
        self.automation.reminders = reminders;
        self.automation.settings = settings;
    }

    fn automation_show_brief(&mut self, ui: &AppWindow, brief: &Value) {
        let st = ui.global::<State>();
        if !brief.is_object() {
            st.set_error("This brief is not ready yet. Try again in a few minutes.".into());
            return;
        }
        let body = &brief["body"];
        let mut sources = Vec::new();
        let sections: Vec<BriefSection> = array(&body["sections"])
            .iter()
            .map(|section| BriefSection {
                title: text(section, "title").into(),
                items: model(
                    array(&section["items"])
                        .iter()
                        .map(|item| brief_item(item, &mut sources))
                        .collect(),
                ),
            })
            .collect();
        let agenda: Vec<BriefItem> = array(&body["agenda"])
            .iter()
            .map(|item| brief_item(item, &mut sources))
            .collect();
        let title = match text(brief, "title") {
            title if title.is_empty() => "Brief".to_owned(),
            title => title,
        };
        let subtitle = [
            local_time(&text(brief, "starts_at"), "%a %-d %b · %H:%M"),
            text(brief, "project_name"),
        ]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" · ");
        let note = match text(body, "note") {
            note if note.is_empty() && text(brief, "status") == "facts" => {
                "Talking points are not ready yet. The facts below come from earlier meetings."
                    .into()
            }
            note => note,
        };
        self.automation.agenda = if agenda.is_empty() {
            String::new()
        } else {
            format!(
                "Agenda · {title}\n{}",
                agenda
                    .iter()
                    .map(|item| format!("• {}", item.text))
                    .collect::<Vec<_>>()
                    .join("\n")
            )
        };
        self.automation.sources = sources;
        self.dialog(
            ui,
            "automation-brief",
            &title,
            &subtitle,
            "brief",
            json!({"event_id":brief["event_id"]}),
        );
        st.set_modal_submit("Copy agenda".into());
        st.set_brief_summary(text(body, "summary").into());
        st.set_brief_note(note.into());
        st.set_brief_note_first(text(brief, "status") != "ready");
        st.set_brief_sections(model(sections));
        st.set_brief_agenda(model(agenda));
    }

    pub(super) fn automation_brief_ready(&mut self, ui: &AppWindow, payload: &Value) {
        let st = ui.global::<State>();
        let at = local_time(&text(payload, "starts_at"), "%H:%M");
        let message = if at.is_empty() {
            format!("Brief ready · {}", text(payload, "title"))
        } else {
            format!("Brief ready · {} at {at}", text(payload, "title"))
        };
        st.set_message(message.clone().into());
        st.set_message_action_for(message.into());
        st.set_message_action_label("Open brief".into());
        st.set_message_action("automation-brief".into());
        st.set_message_action_value(text(payload, "event_id").into());
        self.workflow_calendar_reload(ui);
    }
}
