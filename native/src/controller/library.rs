//! Library navigation, filtering and source-backed search.

use super::*;

impl Controller {
    pub(super) fn refresh_list(&self, ui: &AppWindow) {
        self.request(
            ui,
            "meeting_overviews",
            json!({"includePattern":false}),
            "overviews",
            "",
        );
        self.request(
            ui,
            "workspace_meeting_states",
            json!({}),
            "organization",
            "",
        );
    }

    pub(super) fn search_request(&self, ui: &AppWindow, query: &str, limit: u32) -> Value {
        let st = ui.global::<State>();
        let optional = |value: slint::SharedString| {
            let value = value.trim();
            if value.is_empty() {
                Value::Null
            } else {
                Value::String(value.into())
            }
        };
        json!({"request": {
            "query": query, "limit": limit,
            "project_id": optional(st.get_project_id()),
            "after": optional(st.get_filter_after()),
            "before": optional(st.get_filter_before()),
            "tag": optional(st.get_filter_tag()),
        }})
    }

    pub(super) fn search_library(&mut self, ui: &AppWindow, query: &str, ask: bool) {
        self.library_revision += 1;
        let mut args = self.search_request(ui, query, if ask { 12 } else { 100 });
        if ask {
            args["jobId"] =
                format!("native-{}-{}", std::process::id(), self.library_revision).into();
        }
        ui.global::<State>().set_loading(true);
        ui.global::<State>().set_busy(ask);
        ui.global::<State>().set_answer_text("".into());
        self.request(
            ui,
            if ask {
                "workspace_ask"
            } else {
                "workspace_search"
            },
            args,
            if ask { "question" } else { "search" },
            &self.library_revision.to_string(),
        );
    }

    fn filed(&self, id: &str) -> bool {
        self.project_membership
            .iter()
            .any(|p| text(p, "meeting_id") == id)
    }

    /// Active meetings without a project, and how many of them have a suggestion.
    pub(super) fn unfiled_counts(&self) -> (usize, usize) {
        let unfiled: Vec<String> = self
            .overviews
            .iter()
            .map(|row| text(row, "id"))
            .filter(|id| {
                self.organization
                    .get(id)
                    .is_none_or(|v| v["trashed_at"].is_null() && v["archived"] != true)
                    && !self.filed(id)
            })
            .collect();
        let suggested = unfiled
            .iter()
            .filter(|id| {
                self.project_suggestions
                    .iter()
                    .any(|s| text(s, "meeting_id") == id.as_str())
            })
            .count();
        (unfiled.len(), suggested)
    }

    pub(super) fn filter_list(&self, ui: &AppWindow) {
        let st = ui.global::<State>();
        let query = st.get_list_query().to_lowercase();
        st.set_list_filter_label(
            match self.list_filter.as_str() {
                "pinned" => "PINNED",
                "archived" => "ARCHIVED",
                "needs-project" => "NEEDS A PROJECT",
                _ => "MEETINGS",
            }
            .into(),
        );
        let rows = self
            .overviews
            .iter()
            .filter(|row| {
                let id = text(row, "id");
                let state = self.organization.get(&id);
                state.is_none_or(|v| v["trashed_at"].is_null())
                    && (st.get_project_id().is_empty()
                        || self.project_membership.iter().any(|p| {
                            text(p, "meeting_id") == id
                                && text(p, "project_id") == st.get_project_id().as_str()
                        }))
                    && text(row, "title").to_lowercase().contains(&query)
                    && match self.list_filter.as_str() {
                        "pinned" => state.is_some_and(|v| v["pinned"] == true),
                        "archived" => state.is_some_and(|v| v["archived"] == true),
                        "needs-project" => {
                            !state.is_some_and(|v| v["archived"] == true) && !self.filed(&id)
                        }
                        _ => !state.is_some_and(|v| v["archived"] == true),
                    }
            })
            .map(|row| MeetingRow {
                id: text(row, "id").into(),
                title: text(row, "title").into(),
                subtitle: format!(
                    "{} · {}{}",
                    short_date(&text(row, "created_at")),
                    time(row["duration"].as_f64().unwrap_or(0.0), false),
                    // A waiting suggestion is worth a glance from the list.
                    self.project_suggestions
                        .iter()
                        .find(|s| text(s, "meeting_id") == text(row, "id"))
                        .filter(|_| !self.filed(&text(row, "id")))
                        .and_then(|s| {
                            self.projects
                                .iter()
                                .find(|p| text(p, "id") == text(s, "project_id"))
                        })
                        .map(|p| format!(" · {}?", clip(&text(p, "name"), 20)))
                        .unwrap_or_default()
                )
                .into(),
                pinned: self
                    .organization
                    .get(&text(row, "id"))
                    .is_some_and(|v| v["pinned"].as_bool() == Some(true)),
            })
            .collect();
        st.set_meetings(model(rows));
    }

    pub(super) fn trashed_meetings(&self) -> Vec<Value> {
        self.overviews
            .iter()
            .filter_map(|overview| {
                let id = text(overview, "id");
                let state = self.organization.get(&id)?;
                if state["trashed_at"].is_null() {
                    return None;
                }
                let mut row = state.clone();
                row["id"] = id.into();
                row["title"] = overview["title"].clone();
                row["recorded_at"] = overview["created_at"].clone();
                row["body"] = "In trash".into();
                Some(row)
            })
            .collect()
    }

    pub(super) fn render_results(&self, ui: &AppWindow) {
        let st = ui.global::<State>();
        let mut previous = String::new();
        let rows = self
            .results
            .iter()
            .map(|v| {
                let id = result_id(v);
                let title = if text(v, "title").is_empty() {
                    text(v, "summary")
                } else {
                    text(v, "title")
                };
                let group_title = if previous != title {
                    previous = title.clone();
                    title.clone()
                } else {
                    String::new()
                };
                let kind = text(v, "source_kind");
                ResultRow {
                    id: id.into(),
                    title: title.into(),
                    group_title: group_title.into(),
                    kind: if st.get_view() == 7 {
                        text(v, "meeting_url").into()
                    } else if kind == "summary" {
                        "Notes".into()
                    } else {
                        "Transcript".into()
                    },
                    stamp: time(v["start_time"].as_f64().unwrap_or(0.0), false).into(),
                    subtitle: if st.get_view() == 7 {
                        chrono::DateTime::parse_from_rfc3339(&text(v, "starts_at"))
                            .map(|date| {
                                if v["all_day"] == true {
                                    format!("{} · All day", date.format("%a %-d %b"))
                                } else {
                                    date.with_timezone(&chrono::Local)
                                        .format("%a %-d %b · %H:%M")
                                        .to_string()
                                }
                            })
                            .unwrap_or_else(|_| text(v, "starts_at"))
                            .into()
                    } else if st.get_view() == 6 {
                        text(v, "owner").into()
                    } else {
                        library_date(&text(v, "recorded_at")).into()
                    },
                    body: if st.get_view() == 7 {
                        format!(
                            "{} · {}",
                            text(v, "location"),
                            serde_json::from_str::<Value>(&text(v, "participants"))
                                .ok()
                                .map(|people| array(&people)
                                    .iter()
                                    .map(|p| {
                                        let name = text(p, "name");
                                        if name.is_empty() {
                                            text(p, "email")
                                        } else {
                                            name
                                        }
                                    })
                                    .collect::<Vec<_>>()
                                    .join(", "))
                                .unwrap_or_default()
                        )
                        .trim()
                        .into()
                    } else if st.get_view() == 6 {
                        text(v, "status").into()
                    } else {
                        text(v, "body").into()
                    },
                }
            })
            .collect();
        let meetings = self
            .results
            .iter()
            .map(|v| text(v, "meeting_id"))
            .collect::<std::collections::HashSet<_>>()
            .len();
        st.set_result_summary(if st.get_view() == 7 {
            format!("{} upcoming events", self.results.len()).into()
        } else if st.get_view() == 6 {
            format!("{} actions", self.results.len()).into()
        } else if st.get_view() == 5 {
            format!("{} meetings in Trash", self.results.len()).into()
        } else {
            format!("{meetings} meetings · {} passages", self.results.len()).into()
        });
        st.set_results(model(rows));
        if !self
            .results
            .iter()
            .any(|v| result_id(v) == st.get_preview_id().as_str())
        {
            if let Some(row) = self.results.first() {
                self.select_result(ui, &result_id(row));
            } else {
                st.set_preview_id("".into());
                st.set_preview_title("".into());
                st.set_preview_body("".into());
                st.set_preview_date("".into());
                st.set_preview_stamp("".into());
            }
        }
    }
    pub(super) fn select_result(&self, ui: &AppWindow, id: &str) {
        if let Some(row) = self.results.iter().find(|v| result_id(v) == id) {
            let st = ui.global::<State>();
            st.set_preview_id(id.into());
            st.set_preview_stamp(
                time(row["start_time"].as_f64().unwrap_or(0.0), false)
                    .trim_start_matches('0')
                    .into(),
            );
            st.set_preview_title(text(row, "title").into());
            st.set_preview_body(text(row, "body").into());
            st.set_preview_date(library_date(&text(row, "recorded_at")).into());
            st.set_preview_kind(
                format!(
                    "{} · {}",
                    if text(row, "source_kind") == "summary" {
                        "Notes"
                    } else {
                        "Transcript"
                    },
                    time(row["start_time"].as_f64().unwrap_or(0.0), false)
                )
                .into(),
            );
        }
    }

    pub(super) fn show_answer(&mut self, ui: &AppWindow, answer: &Value) {
        let st = ui.global::<State>();
        let mut content = array(&answer["claims"])
            .iter()
            .map(|claim| {
                let citations = array(&claim["citations"])
                    .iter()
                    .enumerate()
                    .map(|(index, citation)| {
                        format!(
                            "[Source {}](#answer-source-{})",
                            index + 1,
                            text(citation, "id")
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(" · ");
                format!("{}\n\n{}", text(claim, "text"), citations)
            })
            .collect::<Vec<_>>()
            .join("\n\n");
        if content.trim().is_empty() {
            content =
                "There is not enough evidence in these meetings to answer this question.".into();
        }
        st.set_answer_id(text(answer, "id").into());
        st.set_library_ask(true);
        st.set_answer_stale(answer["stale"].as_bool().unwrap_or(false));
        st.set_answer_content(crate::notes::preview(&content));
        st.set_answer_text(content.into());
        self.results = array(&answer["sources"]);
        self.render_results(ui);
    }
}
