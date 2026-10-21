use super::*;
impl Controller {
    pub(super) fn update_meeting_project(&self, ui: &AppWindow) {
        let st = ui.global::<State>();
        let name = |id: &str| {
            self.projects
                .iter()
                .find(|p| text(p, "id") == id)
                .map(|p| text(p, "name"))
                .unwrap_or_default()
        };
        let membership = self
            .project_membership
            .iter()
            .find(|m| text(m, "meeting_id") == self.active);
        let suggestion = self
            .project_suggestions
            .iter()
            .find(|s| text(s, "meeting_id") == self.active);
        // A suggestion is only shown while the meeting has no project.
        let (id, project, state, reason) = match (membership, suggestion) {
            (Some(m), _) => (
                text(m, "project_id"),
                name(&text(m, "project_id")),
                if text(m, "origin") == "automatic" {
                    "automatic"
                } else {
                    "manual"
                },
                text(m, "reason"),
            ),
            (None, Some(s)) if !name(&text(s, "project_id")).is_empty() => (
                String::new(),
                name(&text(s, "project_id")),
                "suggested",
                text(s, "reason"),
            ),
            _ => Default::default(),
        };
        st.set_meeting_project_id(id.into());
        st.set_meeting_project_name(clip(&project, 24).into());
        st.set_meeting_project_state(state.into());
        st.set_meeting_project_reason(reason.into());
    }
    pub(super) fn hub_refresh(&self, ui: &AppWindow) {
        let st = ui.global::<State>();
        let project = st.get_project_id().to_string();
        let tab = st.get_hub_tab();
        let context = json!({"project":project,"tab":tab}).to_string();
        st.set_hub_loading(true);
        match tab {
            4 => self.request(ui,"workspace_review",json!({"action":{"op":"list","project_id":if project.is_empty(){None}else{Some(project)},"meeting_id":null}}),"hub-rows",&context),
            5 => self.request(ui,"workspace_jobs",json!({"action":{"op":"list"}}),"hub-rows",&context),
            6 => self.request(ui,"workspace_review",json!({"action":{"op":"due"}}),"hub-rows",&context),
            7 => self.request(ui,"workspace_backup_settings",json!({"settings":null}),"hub-backups",&context),
            8 => self.request(ui,"workspace_connections",json!({"action":{"op":"list"}}),"hub-rows",&context),
            _ if !project.is_empty() => self.request(ui,"workspace_project",json!({"action":{"op":"get","id":project}}),"hub-project-data",&context),
            _ => {st.set_hub_loading(false);st.set_hub_rows(model(Vec::new()));}
        }
    }
    pub(super) fn hub_action(&mut self, ui: &AppWindow, action: &str, value: &str) {
        let st = ui.global::<State>();
        let project = st.get_project_id().to_string();
        let tab = st.get_hub_tab();
        let selected = self
            .hub_rows
            .iter()
            .find(|r| text(r, "id") == value)
            .cloned()
            .unwrap_or(Value::Null);
        match action {
            "hub" => {
                st.set_view(8);
                self.hub_labels(ui);
                self.request(
                    ui,
                    "workspace_project",
                    json!({"action":{"op":"list"}}),
                    "hub-projects",
                    "",
                );
                self.hub_refresh(ui);
            }
            "hub-tab" => {
                st.set_view(8);
                st.set_hub_tab(value.parse().unwrap_or(0));
                st.set_hub_selected("".into());
                st.set_hub_detail("".into());
                self.hub_labels(ui);
                self.hub_refresh(ui);
            }
            "hub-project" => {
                let index = value.parse::<usize>().unwrap_or(0);
                st.set_project_index(index as i32);
                let p = index
                    .checked_sub(1)
                    .and_then(|i| self.projects.get(i))
                    .cloned()
                    .unwrap_or(Value::Null);
                st.set_project_id(text(&p, "id").into());
                st.set_project_name(if p.is_null() {
                    "All projects".into()
                } else {
                    text(&p, "name").into()
                });
                self.hub_action(ui, "hub-tab", &tab.to_string());
                self.filter_list(ui);
            }
            "hub-new-project" | "hub-edit-project" => {
                let p = self
                    .projects
                    .iter()
                    .find(|p| text(p, "id") == project)
                    .cloned()
                    .unwrap_or(Value::Null);
                self.dialog(
                    ui,
                    "hub-save-project",
                    if action == "hub-new-project" {
                        "New project"
                    } else {
                        "Edit project"
                    },
                    "Give this project a clear name. Context can be edited in the Context tab.",
                    "field",
                    if action == "hub-new-project" {
                        Value::Null
                    } else {
                        p.clone()
                    },
                );
                st.set_modal_value(if action == "hub-new-project" {
                    "".into()
                } else {
                    text(&p, "name").into()
                });
            }
            "hub-primary" => match tab {
                0 => {
                    if project.is_empty() {
                        self.hub_action(ui, "hub-new-project", "");
                    } else {
                        self.choices(
                            ui,
                            "hub-add-meeting",
                            "Add an existing meeting",
                            self.overviews
                                .iter()
                                .take(250)
                                .map(|m| (text(m, "id"), text(m, "title")))
                                .collect(),
                            json!({"project_id":project}),
                        );
                    }
                }
                1 | 2 => {
                    if project.is_empty() {
                        self.hub_action(ui, "hub-new-project", "");
                        return;
                    }
                    self.dialog(
                        ui,
                        if tab == 1 {
                            "hub-save-person"
                        } else {
                            "hub-save-term"
                        },
                        if tab == 1 {
                            "Add a project person"
                        } else {
                            "Add vocabulary"
                        },
                        if tab == 1 {
                            "Name this person. Confirm their voice from a meeting before enabling \
                             recognition."
                        } else {
                            "Enter the exact spelling. Hints guide future transcription and never \
                             rewrite saved text."
                        },
                        "field",
                        Value::Null,
                    );
                    st.set_modal_secondary_label(
                        if tab == 1 {
                            "Role (optional)"
                        } else {
                            "Meaning (optional)"
                        }
                        .into(),
                    );
                }
                3 => {
                    if project.is_empty() {
                        self.hub_action(ui, "hub-new-project", "");
                        return;
                    }
                    self.dialog(
                        ui,
                        "hub-save-context",
                        "Project context",
                        "Background for this project. Meeting evidence takes precedence over \
                         background.",
                        "edit",
                        Value::Null,
                    );
                    st.set_modal_value(st.get_project_context());
                }
                4 => self.hub_refresh(ui),
                9 if project.is_empty() => self.hub_action(ui, "hub-new-project", ""),
                9 => self.choices(
                    ui,
                    "hub-rule-kind",
                    "Add a signal",
                    vec![
                        ("email".into(), "Invitee".into()),
                        ("domain".into(), "Organisation".into()),
                        ("title".into(), "Title phrase".into()),
                    ],
                    Value::Null,
                ),
                5 => self.hub_action(ui, "hub-job-options", ""),
                6 => self.action(ui, "navigate", "6"),
                7 => self.hub_action(ui, "hub-backup-options", ""),
                8 => self.hub_action(ui, "hub-connection-options", ""),
                _ => {}
            },
            "hub-select" => {
                st.set_hub_job_state(text(&selected, "state").into());
                st.set_hub_selected(value.into());
                st.set_hub_detail(if tab == 9 {
                    format!(
                        "{}\n\n{}\n\nMeetings matching this signal are filed in {}.",
                        text(&selected, "label"),
                        rule_description(&selected),
                        st.get_project_name()
                    )
                    .into()
                } else if tab == 3 {
                    format!(
                        "{}\n\nSource: {}\n\n{}",
                        text(&selected, "body"),
                        text(&selected, "source_quote"),
                        if selected["source_current"] == false {
                            "Source changed — review before reuse."
                        } else {
                            ""
                        }
                    )
                    .into()
                } else {
                    [
                        "title", "name", "role", "term", "meaning", "text", "detail", "owner",
                        "due_date",
                    ]
                    .iter()
                    .map(|key| text(&selected, key))
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join("\n\n")
                    .into()
                });
            }
            "hub-open" => match tab {
                0 => self.open(ui, value),
                1 | 2 => {
                    self.dialog(
                        ui,
                        if tab == 1 {
                            "hub-save-person"
                        } else {
                            "hub-save-term"
                        },
                        if tab == 1 {
                            "Edit person"
                        } else {
                            "Edit vocabulary"
                        },
                        "Changes apply to this project.",
                        "field",
                        selected.clone(),
                    );
                    st.set_modal_value(
                        text(&selected, if tab == 1 { "name" } else { "term" }).into(),
                    );
                    st.set_modal_secondary_label(if tab == 1 { "Role" } else { "Meaning" }.into());
                    st.set_modal_secondary(
                        text(&selected, if tab == 1 { "role" } else { "meaning" }).into(),
                    );
                }
                3 => self.request(
                    ui,
                    "workspace_project",
                    json!({"action":{"op":"source","id":value}}),
                    "hub-fact-source",
                    "",
                ),
                4 => {
                    let meeting = text(&selected, "meeting_id");
                    let meeting = if meeting.is_empty() {
                        text(&selected, "source_meeting_id")
                    } else {
                        meeting
                    };
                    if !meeting.is_empty() {
                        self.open(ui, &meeting);
                        if text(&selected, "kind").starts_with("notes_") {
                            self.action(ui, "meeting-tab", "1");
                        } else {
                            self.action(ui, "meeting-tab", "0");
                            self.position = selected["start"].as_f64().unwrap_or(0.);
                            self.request(
                                ui,
                                "native_audio",
                                json!({
                                    "action": "load",
                                    "meetingId": meeting,
                                    "value": self.position
                                }),
                                "audio-start",
                                "",
                            );
                        }
                    }
                }
                5 => self.request(
                    ui,
                    "workspace_jobs",
                    json!({"action":{"op":"retry","id":value}}),
                    "hub-mutated",
                    "",
                ),
                6 => {
                    self.dialog(
                        ui,
                        "hub-remind",
                        "Remind me",
                        "Local time: YYYY-MM-DD HH:MM. Reminders appear while Overtalk is running.",
                        "field",
                        selected,
                    );
                    st.set_modal_value(
                        (chrono::Local::now() + chrono::Duration::hours(1))
                            .format("%Y-%m-%d %H:%M")
                            .to_string()
                            .into(),
                    );
                }
                8 => self.hub_action(ui, "hub-connection-edit", value),
                _ => {}
            },
            "hub-accept" | "hub-dismiss" => {
                if tab == 3 {
                    self.request(
                        ui,
                        "workspace_project",
                        json!({
                            "action": {
                                "op": "resolve_fact",
                                "id": value,
                                "status": if action == "hub-accept" {
                                    "accepted"
                                } else {
                                    "rejected"
                                }
                            }
                        }),
                        "hub-mutated",
                        "",
                    );
                }
                if tab == 4 {
                    self.request(
                        ui,
                        "workspace_review",
                        json!({
                            "action": {
                                "op": "resolve",
                                "id": value,
                                "fingerprint": selected["fingerprint"],
                                "resolution": if action == "hub-accept" {
                                    "reviewed"
                                } else {
                                    "dismissed"
                                }
                            }
                        }),
                        "hub-mutated",
                        "",
                    );
                }
                if tab == 5 {
                    self.request(
                        ui,
                        "workspace_jobs",
                        json!({"action":{"op":"cancel","id":value}}),
                        "hub-mutated",
                        "",
                    );
                }
            }
            "hub-remove" => self.request(
                ui,
                "workspace_project",
                json!({
                    "action": {
                        "op": match tab {
                            1 => "remove_person",
                            9 => "remove_rule",
                            _ => "remove_term",
                        },
                        "id": value
                    }
                }),
                "hub-mutated",
                "",
            ),
            "hub-assign" => {
                let current = st.get_meeting_project_id().to_string();
                let state = st.get_meeting_project_state().to_string();
                let reason = st.get_meeting_project_reason().to_string();
                let suggested = self
                    .project_suggestions
                    .iter()
                    .find(|s| text(s, "meeting_id") == self.active)
                    .map(|s| text(s, "project_id"))
                    .filter(|_| state == "suggested");
                let row = |id: &str, title: String, description: String, icon: &str| DetailRow {
                    id: id.into(),
                    title: title.into(),
                    description: description.into(),
                    value: icon.into(),
                    checked: false,
                };
                let mut rows = Vec::new();
                if suggested.is_some() {
                    rows.push(row(
                        "suggestion:accept",
                        format!("Accept {}", st.get_meeting_project_name()),
                        if reason.is_empty() {
                            "Suggested for this meeting".into()
                        } else {
                            format!("Suggested · {reason}")
                        },
                        "check",
                    ));
                    rows.push(row(
                        "suggestion:dismiss",
                        "Dismiss suggestion".into(),
                        "Keep this meeting unsorted".into(),
                        "close",
                    ));
                } else if !current.is_empty() {
                    rows.push(row(
                        "",
                        "No project".into(),
                        "Remove it from its project".into(),
                        "close",
                    ));
                }
                rows.extend(
                    self.projects
                        .iter()
                        .filter(|p| suggested.as_deref() != Some(text(p, "id").as_str()))
                        .map(|p| {
                            let id = text(p, "id");
                            let description = if id != current {
                                String::new()
                            } else if state != "automatic" {
                                "Current project".into()
                            } else if reason.is_empty() {
                                "Filed automatically".into()
                            } else {
                                format!("Filed automatically · {reason}")
                            };
                            row(&id, text(p, "name"), description, "folder")
                        }),
                );
                if rows.is_empty() {
                    rows.push(row(
                        "",
                        "No project".into(),
                        "Create a project in Projects first".into(),
                        "close",
                    ));
                }
                self.rich_choices(
                    ui,
                    "hub-assign-choice",
                    "Assign meeting to project",
                    rows,
                    json!({"meeting_id":self.active}),
                );
            }
            "hub-learn" => {
                if self.active.is_empty() || st.get_meeting_project_id().is_empty() {
                    st.set_error(
                        "Assign this meeting to a project before learning its context.".into(),
                    );
                    return;
                }
                self.request(
                    ui,
                    "workspace_jobs",
                    json!({
                        "action": {
                            "op": "submit",
                            "kind": "learn",
                            "meeting_id": self.active,
                            "language": null
                        }
                    }),
                    "hub-mutated",
                    "",
                );
            }
            "hub-person-voice" => {
                if st.get_meeting_project_id().is_empty() {
                    st.set_error("Assign this meeting to a project first.".into());
                    return;
                }
                self.request(
                    ui,
                    "workspace_project",
                    json!({"action":{"op":"get","id":st.get_meeting_project_id().as_str()}}),
                    "hub-voice-people",
                    value,
                );
            }
            "hub-history" => self.request(
                ui,
                "workspace_review",
                json!({"action":{"op":"versions","meeting_id":self.active}}),
                "hub-history",
                "",
            ),
            "hub-reading" => {
                self.dialog(
                    ui,
                    "hub-translate",
                    "Translate transcript",
                    "Choose the target language. A reading copy preserves every passage; the \
                     original stays available.",
                    "field",
                    Value::Null,
                );
                st.set_modal_value(st.get_reading_language());
            }
            _ => self.hub_extra_action(ui, action, value),
        }
    }
    fn hub_labels(&self, ui: &AppWindow) {
        let st = ui.global::<State>();
        let tab = st.get_hub_tab().clamp(0, 9) as usize;
        st.set_hub_heading(
            [
                "Project meetings",
                "People & remembered voices",
                "Project vocabulary",
                "Context & sourced knowledge",
                "Needs review",
                "Background work",
                "Due & overdue actions",
                "Encrypted backups",
                "Connected tools",
                "Automatic filing",
            ][tab]
                .into(),
        );
        st.set_hub_description(
            [
                "Recurring series keep previous decisions and unfinished work together.",
                "Names are confirmed by you; recognition uses this project's voices.",
                "Correct spelling and meaning for names, acronyms and specialist terms.",
                "Proposed facts require review. Changed sources stop being used automatically.",
                "Listen before correcting. Flags describe issues, not confidence percentages.",
                "One heavy job at a time. Recording takes priority.",
                "Continue unfinished work across meetings without duplicating tasks.",
                "Schedule encrypted copies and restore a verified workspace.",
                "Review outgoing changes before creating or updating external content.",
                "Meetings matching these signals are filed in this project. Remove anything that is wrong.",
            ][tab]
                .into(),
        );
        st.set_hub_primary(
            [
                if st.get_project_id().is_empty() {
                    "New project"
                } else {
                    "Assign meeting"
                },
                "Add person",
                "Add term",
                "Edit context",
                "Refresh",
                "Queue work",
                "Manage actions",
                "Backup options",
                "Add connection",
                if st.get_project_id().is_empty() {
                    "New project"
                } else {
                    "Add signal"
                },
            ][tab]
                .into(),
        );
    }
    pub(super) fn hub_reply(&mut self, ui: &AppWindow, tag: &str, context: &str, value: Value) {
        let st = ui.global::<State>();
        if matches!(tag, "hub-project-data" | "hub-rows" | "hub-backups") && !context.is_empty() {
            let expected: Value = serde_json::from_str(context).unwrap_or(Value::Null);
            if expected["project"].as_str() != Some(st.get_project_id().as_str())
                || expected["tab"].as_i64() != Some(st.get_hub_tab() as i64)
            {
                return;
            }
        }
        st.set_hub_loading(false);
        match tag {
            "hub-fact-source" => {
                let meeting = text(&value, "meeting_id");
                self.open(ui, &meeting);
                self.action(ui, "meeting-tab", "0");
                self.position = value["start"].as_f64().unwrap_or(0.);
                self.request(
                    ui,
                    "native_audio",
                    json!({"action":"load","meetingId":meeting,"value":self.position}),
                    "audio-start",
                    "",
                );
            }
            "hub-created" => {
                st.set_modal("".into());
                st.set_project_id(text(&value, "id").into());
                self.hub_action(ui, "hub-tab", "0");
                self.request(
                    ui,
                    "workspace_project",
                    json!({"action":{"op":"list"}}),
                    "hub-projects",
                    "",
                );
            }
            "hub-projects" => {
                self.projects = array(&value["projects"]);
                self.project_membership = array(&value["membership"]);
                self.project_suggestions = array(&value["suggestions"]);
                self.automation_render_capture(ui);
                let mut names = vec!["All projects".into()];
                names.extend(self.projects.iter().map(|p| text(p, "name").into()));
                st.set_project_names(model(names));
                st.set_project_index(
                    self.projects
                        .iter()
                        .position(|p| text(p, "id") == st.get_project_id().as_str())
                        .map_or(0, |i| i + 1) as i32,
                );
                self.update_meeting_project(ui);
                self.hub_labels(ui);
                self.filter_list(ui);
            }
            "hub-mutated" => {
                st.set_modal("".into());
                self.request(
                    ui,
                    "workspace_project",
                    json!({"action":{"op":"list"}}),
                    "hub-projects",
                    "",
                );
                self.hub_refresh(ui);
                self.refresh_list(ui);
            }
            "hub-project-data" => {
                st.set_project_context(text(&value["project"], "context").into());
                st.set_project_name(text(&value["project"], "name").into());
                let tasks = array(&value["tasks"])
                    .iter()
                    .take(8)
                    .map(|t| {
                        format!(
                            "• {} — {}{}",
                            text(t, "title"),
                            text(t, "owner"),
                            if t["due_date"].is_string() {
                                format!(" · due {}", text(t, "due_date"))
                            } else {
                                String::new()
                            }
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                let facts = array(&value["facts"])
                    .iter()
                    .filter(|f| f["status"] == "accepted" && f["source_current"] == true)
                    .take(5)
                    .map(|f| format!("• {}", text(f, "body")))
                    .collect::<Vec<_>>()
                    .join("\n");
                st.set_project_brief(
                    format!(
                        "{}\n\nPrevious decisions & context\n{}\n\nOpen actions\n{}",
                        st.get_project_context(),
                        if facts.is_empty() {
                            "No confirmed facts yet."
                        } else {
                            &facts
                        },
                        if tasks.is_empty() {
                            "No open actions."
                        } else {
                            &tasks
                        }
                    )
                    .into(),
                );
                self.hub_rows = array(
                    &value[match st.get_hub_tab() {
                        9 => "rules",
                        tab => {
                            ["meetings", "people", "vocabulary", "facts"][tab.clamp(0, 3) as usize]
                        }
                    }],
                );
                self.hub_render_rows(ui);
            }
            "hub-rows" => {
                self.hub_rows = array(&value);
                self.hub_render_rows(ui);
            }
            "hub-history" => {
                self.hub_options["notes_hash"] = value["current_hash"].clone();
                self.hub_versions = array(&value["versions"]);
                self.choices(
                    ui,
                    "hub-version",
                    "Notes history",
                    self.hub_versions
                        .iter()
                        .map(|v| {
                            (
                                v["id"].to_string(),
                                format!("{} · {}", text(v, "created_at"), text(v, "origin")),
                            )
                        })
                        .collect(),
                    Value::Null,
                );
            }
            "hub-voice-people" => self.choices(
                ui,
                "hub-voice-person",
                "Whose voice is this?",
                array(&value["people"])
                    .iter()
                    .map(|p| {
                        (
                            text(p, "id"),
                            format!("{} · {}", text(p, "name"), text(p, "role")),
                        )
                    })
                    .collect(),
                json!({"speaker_id":context,"people":value["people"]}),
            ),
            "hub-reading-data" => {
                if context != self.active {
                    return;
                }
                if value.is_null() {
                    st.set_error(
                        "No current translation is saved. Choose Translate to generate one.".into(),
                    );
                    return;
                }
                let map: HashMap<_, _> = array(&value["segments"])
                    .iter()
                    .map(|r| (text(r, "id"), text(r, "text")))
                    .collect();
                self.translations = map;
                st.set_reading_ready(!self.translations.is_empty());
                st.set_reading_mode(2);
                for m in self.cache.values_mut() {
                    m.rows = None;
                }
                self.render_meeting(ui);
            }
            _ => self.hub_extra_reply(ui, tag, context, value),
        }
    }
    fn hub_render_rows(&self, ui: &AppWindow) {
        let tab = ui.global::<State>().get_hub_tab();
        let rows = self
            .hub_rows
            .iter()
            .map(|v| {
                let (title, description, status) = match tab {
                    0 => (text(v, "title"), text(v, "series"), text(v, "date")),
                    1 => (
                        text(v, "name"),
                        text(v, "role"),
                        if v["profile_id"].is_string() {
                            "Voice remembered".into()
                        } else {
                            "Voice not enrolled".into()
                        },
                    ),
                    2 => (text(v, "term"), text(v, "meaning"), text(v, "alternatives")),
                    3 => (
                        text(v, "body"),
                        text(v, "source_quote"),
                        if v["source_current"] == false {
                            "Source changed".into()
                        } else {
                            text(v, "status")
                        },
                    ),
                    4 => (
                        format!(
                            "{} · {}",
                            match text(v, "kind").as_str() {
                                "unassigned" => "Speaker not assigned",
                                "overlap" => "Overlapping speech",
                                "repetition" => "Repeated phrase",
                                "notes_stale" => "Notes need refresh",
                                "notes_sources" => "Missing source links",
                                _ => "Needs review",
                            },
                            text(v, "title")
                        ),
                        text(v, "text"),
                        time(v["start"].as_f64().unwrap_or(0.), false),
                    ),
                    5 => (
                        match text(v, "kind").as_str() {
                            "translate" => "Translate transcript",
                            "learn" => "Learn project context",
                            "notes" => "Generate notes",
                            "transcribe" => "Transcribe meeting",
                            _ => "Background work",
                        }
                        .into(),
                        text(v, "detail"),
                        text(v, "state"),
                    ),
                    6 => (text(v, "title"), text(v, "owner"), text(v, "due_date")),
                    9 => (text(v, "label"), rule_description(v), String::new()),
                    8 => (
                        text(v, "name"),
                        text(v, "description"),
                        if text(v, "account").is_empty() {
                            text(v, "status")
                        } else {
                            format!("{} · {}", text(v, "status"), text(v, "account"))
                        },
                    ),
                    _ => (text(v, "name"), text(v, "description"), text(v, "status")),
                };
                DetailRow {
                    id: text(v, "id").into(),
                    title: title.into(),
                    description: description.into(),
                    value: status.into(),
                    ..Default::default()
                }
            })
            .collect();
        let st = ui.global::<State>();
        let selected = self
            .hub_rows
            .iter()
            .find(|row| text(row, "id") == st.get_hub_selected().as_str());
        st.set_hub_job_state(
            selected
                .map(|row| text(row, "state"))
                .unwrap_or_default()
                .into(),
        );
        st.set_hub_rows(model(rows));
    }
    pub(super) fn hub_modal_save(&mut self, ui: &AppWindow, action: &str, data: Value) {
        let st = ui.global::<State>();
        let name = st.get_modal_value().to_string();
        let secondary = st.get_modal_secondary().to_string();
        let project = st.get_project_id().to_string();
        let command = match action {
            "hub-save-project" => Some(
                json!({"op":"save","id":data.get("id"),"name":name,"context":data["context"].as_str().unwrap_or(""),"archived":false}),
            ),
            "hub-save-person" => Some(
                json!({"op":"person","id":data.get("id"),"project_id":project,"name":name,"role":secondary}),
            ),
            "hub-save-term" => Some(
                json!({"op":"term","id":data.get("id"),"project_id":project,"term":name,"meaning":secondary,"alternatives":data["alternatives"].as_str().unwrap_or("")}),
            ),
            "hub-save-context" => Some(
                json!({"op":"save","id":project,"name":st.get_project_name().as_str(),"context":name,"archived":false}),
            ),
            "hub-rule-save" => {
                Some(json!({"op":"rule","project_id":project,"kind":data["kind"],"value":name}))
            }
            "hub-translate" => {
                st.set_reading_language(name.clone().into());
                self.request(ui,"workspace_jobs",json!({"action":{"op":"submit","kind":"translate","meeting_id":self.active,"language":name}}),"hub-mutated","");
                None
            }
            "hub-version-restore" => {
                self.request(ui,"workspace_review",json!({"action":{"op":"restore","meeting_id":self.active,"version_id":data,"expected_hash":self.hub_options["notes_hash"]}}),"hub-restored","");
                None
            }
            "hub-remind" => {
                use chrono::TimeZone;
                match chrono::NaiveDateTime::parse_from_str(&name, "%Y-%m-%d %H:%M")
                    .ok()
                    .and_then(|dt| chrono::Local.from_local_datetime(&dt).single())
                {
                    Some(at) => self.request(
                        ui,
                        "workspace_review",
                        json!({"action":{"op":"remind","task_id":data["id"],"at":at.to_rfc3339()}}),
                        "hub-mutated",
                        "",
                    ),
                    None => st.set_error(
                        "Enter an unambiguous local date and time: YYYY-MM-DD HH:MM".into(),
                    ),
                };
                None
            }
            _ => {
                self.hub_extra_save(ui, action, data);
                None
            }
        };
        if let Some(action) = command {
            let tag = if action["op"] == "save" && action["id"].is_null() {
                "hub-created"
            } else {
                "hub-mutated"
            };
            self.request(ui, "workspace_project", json!({"action":action}), tag, "");
        }
    }
    pub(super) fn hub_modal_choice(
        &mut self,
        ui: &AppWindow,
        action: &str,
        data: Value,
        value: &str,
    ) {
        let st = ui.global::<State>();
        match action {
            "hub-assign-choice" if value.starts_with("suggestion:") => self.request(
                ui,
                "workspace_project",
                json!({
                    "action": {
                        "op": if value == "suggestion:accept" {
                            "accept_suggestion"
                        } else {
                            "dismiss_suggestion"
                        },
                        "meeting_id": data["meeting_id"]
                    }
                }),
                "hub-mutated",
                "",
            ),
            "hub-rule-kind" => {
                let (title, hint) = match value {
                    "email" => (
                        "Add an invitee",
                        "Meetings this person is invited to are filed here. Enter their email \
                         address, such as maya@example.com.",
                    ),
                    "domain" => (
                        "Add an organisation",
                        "Meetings with anyone from this email domain are filed here, such as \
                         example.com.",
                    ),
                    _ => (
                        "Add a title phrase",
                        "Meetings whose title contains this phrase are filed here.",
                    ),
                };
                self.dialog(
                    ui,
                    "hub-rule-save",
                    title,
                    hint,
                    "field",
                    json!({"kind":value}),
                );
                st.set_modal_submit("Add signal".into());
            }
            "hub-add-meeting" => self.request(
                ui,
                "workspace_project",
                json!({
                    "action": {
                        "op": "assign",
                        "meeting_id": value,
                        "project_id": data["project_id"],
                        "series": ""
                    }
                }),
                "hub-mutated",
                "",
            ),
            "hub-assign-choice" => self.request(
                ui,
                "workspace_project",
                json!({
                    "action": {
                        "op": "assign",
                        "meeting_id": data["meeting_id"],
                        "project_id": if value.is_empty() { None } else { Some(value) },
                        "series": null
                    }
                }),
                "hub-mutated",
                "",
            ),
            "hub-voice-person" => {
                let name = array(&data["people"])
                    .iter()
                    .find(|p| text(p, "id") == value)
                    .map(|p| text(p, "name"))
                    .unwrap_or_default();
                self.voice_consent_form(ui, &text(&data, "speaker_id"), &name, value);
            }
            "hub-version" => {
                if let Some(version) = self.hub_versions.iter().find(|v| v["id"] == value).cloned()
                {
                    let content: Value =
                        serde_json::from_str(&text(&version, "content")).unwrap_or_default();
                    self.dialog(
                        ui,
                        "hub-version-restore",
                        "Compare notes versions",
                        "Restoring keeps the current notes in history.",
                        "compare",
                        version["id"].clone(),
                    );
                    st.set_comparison_current(st.get_notes());
                    match overtalk_documents::notes_markdown(&content) {
                        Ok(notes) => st.set_comparison_selected(notes.into()),
                        Err(error) => {
                            st.set_error(error.into());
                            st.set_modal("".into());
                            return;
                        }
                    }
                    st.set_modal_submit("Restore version".into());
                }
            }
            _ => self.hub_extra_choice(ui, action, data, value),
        }
    }
}

/// "Invitee · added by you", "Organisation · learned from 3 meetings".
fn rule_description(rule: &Value) -> String {
    let kind = match text(rule, "kind").as_str() {
        "email" => "Invitee",
        "domain" => "Organisation",
        "series" => "Recurring series",
        "title" => "Title phrase",
        "account" => "Calendar account",
        _ => "Signal",
    };
    match (
        text(rule, "origin").as_str(),
        rule["confirmations"].as_u64().unwrap_or(0),
    ) {
        ("learned", 1) => format!("{kind} · learned from 1 meeting"),
        ("learned", n) if n > 1 => format!("{kind} · learned from {n} meetings"),
        ("learned", _) => format!("{kind} · learned from your meetings"),
        _ => format!("{kind} · added by you"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matching_signals_say_what_they_are_and_where_they_came_from() {
        let rule = |kind: &str, origin: &str, confirmations: u64| {
            rule_description(&json!({"kind":kind,"origin":origin,"confirmations":confirmations}))
        };
        assert_eq!(rule("email", "manual", 0), "Invitee · added by you");
        assert_eq!(
            rule("domain", "learned", 1),
            "Organisation · learned from 1 meeting"
        );
        assert_eq!(
            rule("series", "learned", 4),
            "Recurring series · learned from 4 meetings"
        );
        assert_eq!(
            rule("account", "learned", 0),
            "Calendar account · learned from your meetings"
        );
        assert_eq!(clip("Quarterly planning", 10), "Quarterly…");
        assert_eq!(clip("Orion", 10), "Orion");
    }
}
