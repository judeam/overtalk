use super::*;

impl Controller {
    pub(super) fn privacy_start(&mut self, ui: &AppWindow, kind: &str) {
        if ui.global::<State>().get_recording() || ui.global::<State>().get_busy() {
            return;
        }
        if kind == "start-recording" {
            // The engine words the notice for this setup and says whether it is required.
            self.request(
                ui,
                "workspace_privacy",
                json!({
                    "action": {
                        "op": "recording_notice",
                        "auto_notes": ui.global::<State>().get_auto_summary()
                    }
                }),
                "privacy-notice-preview",
                "",
            );
            return;
        }
        self.dialog(
            ui,
            "privacy-capture",
            "Before you record",
            "Tell everyone this conversation will be recorded and explain how it will be used. \
             Confirm your recording is permitted. System-audio capture may include other sounds on \
             this computer. This confirmation records your acknowledgment, not other participants' \
             signatures.",
            "confirm",
            json!(kind),
        );
        ui.global::<State>()
            .set_modal_submit("I’ve informed participants · continue".into());
    }
    pub(super) fn privacy_action(&mut self, ui: &AppWindow, action: &str, value: &str) {
        let snapshot = self
            .settings
            .get("privacy-status")
            .cloned()
            .unwrap_or_default();
        let st = ui.global::<State>();
        match action {
            "privacy-notice" => {
                if self.dialog.0 != "privacy-notice"
                    || !matches!(value, "copied" | "read_aloud" | "skipped")
                {
                    return;
                }
                if value == "copied" {
                    ui.invoke_copy_text(st.get_recording_notice_text());
                    st.set_message("Notice copied. Paste it into the call chat.".into());
                }
                self.request(
                    ui,
                    "workspace_privacy",
                    json!({
                        "action": {
                            "op": "acknowledge_recording",
                            "method": value,
                            "auto_notes": st.get_auto_summary()
                        }
                    }),
                    "privacy-capture-ready",
                    "start-recording",
                );
            }
            "privacy-refresh" => self.request(
                ui,
                "workspace_privacy",
                json!({"action":{"op":"status"}}),
                "privacy-status",
                "",
            ),
            "privacy-policy" => {
                let mut policy = snapshot["policy"].clone();
                if !policy.is_object() {
                    st.set_error("Wait for privacy settings to load.".into());
                    return;
                }
                policy["approved_hosts_text"] = json!(array(&policy["approved_hosts"])
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(", "));
                policy.as_object_mut().unwrap().remove("approved_hosts");
                self.form(ui, "privacy-policy", "Recording & data rules", policy);
                st.set_modal_text(
                    "Remote destinations require HTTPS. Protected mode requires verified encrypted \
                     storage and documented service agreements. Changes apply across the app."
                        .into(),
                );
            }
            "privacy-voices" => {
                let mut rows = vec![("add".into(), "Record a voice authorization".into())];
                rows.extend(array(&snapshot["authorizations"]).iter().map(|a| {
                    (
                        text(a, "id"),
                        format!(
                            "{} · {}",
                            text(a, "name"),
                            if a["withdrawn"].is_null() {
                                "review or withdraw"
                            } else {
                                "withdrawn"
                            }
                        ),
                    )
                }));
                self.choices(
                    ui,
                    "privacy-authorization-choice",
                    "Voice authorizations",
                    rows,
                    snapshot["authorizations"].clone(),
                );
            }
            "privacy-subject" => {
                self.dialog(
                    ui,
                    "privacy-subject",
                    "Find a person's data",
                    "Search saved speaker names and transcript text. Results require review: \
                     matching a name does not verify someone's identity.",
                    "field",
                    Value::Null,
                );
                st.set_modal_submit("Review matches".into());
            }
            "privacy-audit" => self.request(
                ui,
                "workspace_privacy",
                json!({"action":{"op":"audit"}}),
                "privacy-audit",
                "",
            ),
            "privacy-usage" => self.request(ui, "usage_report", json!({}), "privacy-usage", ""),
            "privacy-verify" => self.request(
                ui,
                "workspace_privacy",
                json!({"action":{"op":"verify_audit"}}),
                "privacy-verified",
                "",
            ),
            "privacy-migrate-keys" => self.request(
                ui,
                "workspace_privacy",
                json!({"action":{"op":"migrate_credentials"}}),
                "privacy-saved",
                "",
            ),
            "privacy-retry" => self.request(
                ui,
                "workspace_privacy",
                json!({"action":{"op":"retry_deletion"}}),
                "privacy-saved",
                "",
            ),
            "privacy-clear-logs" => {
                self.dialog(
                    ui,
                    "privacy-clear-logs",
                    "Clear diagnostic logs?",
                    "Remove Overtalk’s local diagnostic log files, including older logs that may \
                     contain meeting content. This does not remove security activity records.",
                    "confirm",
                    Value::Null,
                );
                st.set_modal_submit("Clear diagnostic logs".into());
            }
            "privacy-retention" => {
                let ids = snapshot["retention_candidates"]
                    .as_array()
                    .map_or(0, Vec::len);
                if ids == 0 {
                    st.set_message(
                        "No meetings currently meet your retention deletion rule.".into(),
                    );
                    return;
                }
                self.dialog(
                    ui,
                    "privacy-retention",
                    "Review retention deletion",
                    &format!(
                        "Delete {ids} expired meetings and their managed recordings? Held meetings \
                         are excluded. This cannot be undone. Existing external exports and \
                         backups need separate handling."
                    ),
                    "confirm",
                    snapshot["retention_candidates"].clone(),
                );
                st.set_modal_submit("Delete expired meetings".into());
            }
            "privacy-holds" => {
                let rows = array(&snapshot["meetings"])
                    .iter()
                    .map(|m| {
                        let held = array(&snapshot["holds"]).iter().any(|h| h["id"] == m["id"]);
                        (
                            text(m, "id"),
                            format!(
                                "{} · {}",
                                text(m, "title"),
                                if held { "held · review" } else { "add hold" }
                            ),
                        )
                    })
                    .collect();
                self.choices(
                    ui,
                    "privacy-hold-choice",
                    "Retention holds",
                    rows,
                    snapshot["holds"].clone(),
                );
                st.set_modal_text(
                    "Holds prevent automatic and manual deletion. Use a documented reason and \
                     release a hold when it no longer applies."
                        .into(),
                );
            }
            _ => {
                let _ = value;
            }
        }
    }
    pub(super) fn privacy_modal_save(&mut self, ui: &AppWindow, action: &str, mut data: Value) {
        let st = ui.global::<State>();
        let op = match action {
            "privacy-usage-save" => {
                self.request(ui, "usage_export", json!({}), "privacy-usage-saved", "");
                return;
            }
            "privacy-capture" => {
                self.request(
                    ui,
                    "workspace_privacy",
                    json!({"action":{"op":"acknowledge_recording"}}),
                    "privacy-capture-ready",
                    data.as_str().unwrap_or("start-recording"),
                );
                return;
            }
            "privacy-policy" if data["automatic_retention"] == true => {
                self.dialog(ui,"privacy-policy-confirm","Enable automatic retention?","Meetings older than your chosen retention period and their recordings will be permanently deleted while the app is running. Held meetings are excluded. Check the day count before enabling this.","confirm",data);
                st.set_modal_submit("Enable automatic deletion".into());
                return;
            }
            "privacy-policy" | "privacy-policy-confirm" => {
                let hosts = text(&data, "approved_hosts_text")
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .collect::<Vec<_>>();
                data.as_object_mut().unwrap().remove("approved_hosts_text");
                data["approved_hosts"] = json!(hosts);
                json!({"op":"save_policy","policy":data})
            }
            "privacy-voice-add" => {
                json!({"op":"authorize_voice","name":data["name"],"basis":data["basis"],"reference":data["reference"],"days":data["days"]})
            }
            "privacy-withdraw" => json!({"op":"withdraw_authorization","id":data}),
            "privacy-subject" => {
                self.request(
                    ui,
                    "workspace_privacy",
                    json!({"action":{"op":"subject_preview","name":st.get_modal_value().as_str()}}),
                    "privacy-subject-preview",
                    "",
                );
                return;
            }
            "privacy-erase-subject" => {
                json!({"op":"erase_subject","name":data["name"],"expected_ids":data["ids"]})
            }
            "privacy-retention" => json!({"op":"apply_retention","expected_ids":data}),
            "privacy-clear-logs" => json!({"op":"clear_diagnostic_logs"}),
            "privacy-release-hold" => json!({"op":"release_hold","meeting_id":data}),
            _ if action.starts_with("privacy-set-hold:") => {
                json!({"op":"hold","meeting_id":action.trim_start_matches("privacy-set-hold:"),"reason":st.get_modal_value().as_str()})
            }
            _ => {
                st.set_modal("".into());
                return;
            }
        };
        self.request(
            ui,
            "workspace_privacy",
            json!({"action":op}),
            "privacy-saved",
            "",
        );
    }
    pub(super) fn privacy_choice(
        &mut self,
        ui: &AppWindow,
        action: &str,
        value: &str,
        data: Value,
    ) {
        match action {
            "privacy-hold-choice" => {
                if let Some(held) = array(&data).iter().find(|h| text(h, "id") == value) {
                    self.dialog(
                        ui,
                        "privacy-release-hold",
                        "Release this retention hold?",
                        &format!(
                            "Current reason: {}. Releasing it allows normal deletion and your \
                             retention rule to apply.",
                            text(held, "reason")
                        ),
                        "confirm",
                        json!(value),
                    );
                    ui.global::<State>().set_modal_submit("Release hold".into());
                } else {
                    self.dialog(
                        ui,
                        &format!("privacy-set-hold:{value}"),
                        "Keep this meeting",
                        "Record why this meeting must be kept. It cannot be deleted until the hold \
                         is released.",
                        "field",
                        Value::Null,
                    );
                    ui.global::<State>()
                        .set_modal_submit("Set retention hold".into());
                }
            }
            "privacy-authorization-choice" if value == "add" => {
                self.form(
                    ui,
                    "privacy-voice-add",
                    "Record voice authorization",
                    json!({"name":"","basis":"explicit_consent","reference":"","days":365}),
                );
                ui.global::<State>().set_modal_text(
                    "Record the person's actual authorization, its reference and expiry. Naming \
                     someone does not give consent. Choose a basis only after it has been verified."
                        .into(),
                );
            }
            "privacy-authorization-choice" => {
                let name = array(&data)
                    .iter()
                    .find(|a| text(a, "id") == value)
                    .map(|a| text(a, "name"))
                    .unwrap_or_default();
                self.dialog(
                    ui,
                    "privacy-withdraw",
                    "Withdraw voice authorization",
                    &format!(
                        "Stop recognizing {name} and delete the stored voice profile. Names \
                         already written in meetings remain; use Find a person's data to review \
                         them."
                    ),
                    "confirm",
                    json!(value),
                );
                ui.global::<State>()
                    .set_modal_submit("Withdraw & forget voice".into());
            }
            "privacy-subject-choice" if value == "export" => {
                self.request(
                    ui,
                    "workspace_privacy",
                    json!({
                        "action": {
                            "op": "export_subject",
                            "name": data["name"],
                            "expected_ids": data["ids"]
                        }
                    }),
                    "privacy-saved",
                    "",
                );
            }
            "privacy-subject-choice" if value == "erase" => {
                self.dialog(
                    ui,
                    "privacy-erase-subject",
                    "Erase reviewed matches",
                    "This deletes every listed meeting in full, including other participants' \
                     contributions, and removes the person's voice profile and project identity. \
                     It cannot erase copies held elsewhere. Continue only after reviewing the \
                     matches.",
                    "confirm",
                    data,
                );
                ui.global::<State>()
                    .set_modal_submit("Delete matched meetings".into());
            }
            _ => {}
        }
    }
    pub(super) fn privacy_reply(&mut self, ui: &AppWindow, tag: &str, context: &str, value: Value) {
        let st = ui.global::<State>();
        match tag{
            "privacy-status"=>{
                let rows=array(&value["storage"]);
                let ready=!rows.is_empty() && rows.iter().all(|r|r["verified"]==true);
                st.set_privacy_encrypted(ready);
                st.set_privacy_summary(if ready{"Device encryption verified for configured storage."}else{"Device encryption needs attention. Protected mode cannot be enabled until all storage is verified."}.into());
                st.set_privacy_credentials(value["pending_credentials"].as_i64().unwrap_or(0) as i32);
                st.set_privacy_pending(value["pending_deletions"].as_i64().unwrap_or(0) as i32);
                self.settings.insert("privacy-status".into(),value);
            },
            "privacy-notice-preview"=>{
                // Turned off in settings: no step, and the engine records the notice as skipped.
                if value["required"]!=true{self.action(ui,"begin-recording","");return;}
                st.set_recording_notice_text(text(&value,"text").into());
                st.set_recording_notice_route(text(&value,"route").into());
                st.set_recording_notice_detail(text(&value,"detail").into());
                self.dialog(ui,"privacy-notice","Tell people you’re recording","Paste it into the call chat or read it aloud. How you told people is saved with this meeting.","recording-notice",Value::Null);
            },
            "privacy-capture-ready"=>{st.set_modal("".into());self.action(ui,if context=="sound-check"{"begin-sound-check"}else{"begin-recording"},"");},
            "privacy-saved"=>{st.set_modal("".into());st.set_message("Privacy changes saved.".into());self.privacy_action(ui,"privacy-refresh","");self.refresh_list(ui);},
            "privacy-verified"=>st.set_message(if value["valid"]==true{"Audit event integrity verified. This does not establish that no events were removed."}else{"Audit integrity check failed. Preserve the data and investigate."}.into()),
            "privacy-audit"=>{
                let body=array(&value).iter().map(|r|format!("{}  {}  {}\n{}",text(r,"at"),text(r,"operation"),text(r,"outcome"),text(r,"object_id"))).collect::<Vec<_>>().join("\n\n");
                self.dialog(ui,"privacy-readonly","Security activity",&body,"about",Value::Null);
            },
            "privacy-usage"=>{
                self.dialog(ui,"privacy-usage-save","Your usage report",&usage_summary(&value),"confirm",Value::Null);
                st.set_modal_submit("Save as a file…".into());
            },
            "privacy-usage-saved"=>{
                st.set_modal("".into());
                if let Some(path)=value["saved"].as_str() {
                    st.set_message(format!("Usage report saved to {path}. It stays there until you send it to someone.").into());
                }
            },
            "privacy-subject-preview"=>{
                let mut description=text(&value,"notice");
                for meeting in array(&value["meetings"]){description.push_str(&format!("\n• {}",text(&meeting,"title")));}
                let rows=if array(&value["ids"]).is_empty(){vec![("close".into(),"No matching meetings".into())]}else{vec![("export".into(),"Export matching data for review…".into()),("erase".into(),"Erase these matched meetings…".into()),("close".into(),"Keep data and close".into())]};
                self.choices(ui,"privacy-subject-choice","Review matching records",rows,value);
                st.set_modal_text(description.into());
            },
            _=>{}
        }
    }
}

/// The usage report in a few lines: only what has happened so far.
fn usage_summary(report: &Value) -> String {
    let total = |event: &str| report["totals"][event].as_u64().unwrap_or(0);
    let mut lines =
        vec!["Kept on this computer; nothing is sent. It holds counts and dates only.".to_owned()];
    let opened = text(report, "first_opened");
    if !opened.is_empty() {
        lines.push(format!(
            "First opened {opened} · weeks with meetings: {}",
            report["weeks_with_meetings"].as_u64().unwrap_or(0)
        ));
    }
    let meetings = [
        ("recording_started", "Recordings"),
        ("transcript_ready", "transcripts"),
        ("notes_ready", "notes"),
        ("source_played", "sources played"),
        ("export_saved", "exports"),
        ("meeting_shared", "copies shared"),
        ("meeting_imported", "meetings opened from files"),
    ]
    .iter()
    .filter(|(event, _)| total(event) > 0)
    .map(|(event, label)| format!("{label}: {}", total(event)))
    .collect::<Vec<_>>();
    if !meetings.is_empty() {
        lines.push(meetings.join(" · "));
    }
    let run = &report["first_run"];
    let mut first = Vec::new();
    if let Some(minutes) = run["minutes_from_first_open_to_first_recording"].as_u64() {
        first.push(format!("{minutes} min to the first recording"));
    }
    if let Some(minutes) = run["model_download_minutes"].as_u64() {
        first.push(format!("models downloaded in {minutes} min"));
    }
    if let Some(minutes) = run["minutes_from_first_recording_to_first_transcript"].as_u64() {
        first.push(format!(
            "first transcript {minutes} min after the first recording"
        ));
    }
    if !first.is_empty() {
        lines.push(format!("First run: {}", first.join(" · ")));
    }
    let projects = [
        ("brief_prepared", "Briefs prepared"),
        ("brief_opened", "briefs opened"),
        ("brief_source_played", "brief sources played"),
        ("filed_automatically", "filed automatically"),
        ("suggestion_accepted", "suggestions accepted"),
        ("filing_changed", "filings you changed"),
    ]
    .iter()
    .filter(|(event, _)| total(event) > 0)
    .map(|(event, label)| format!("{label}: {}", total(event)))
    .collect::<Vec<_>>();
    if !projects.is_empty() {
        lines.push(projects.join(" · "));
    }
    if let Some(days) = run["days_from_first_open_to_first_brief_with_a_source"].as_u64() {
        lines.push(format!(
            "First brief with a source: {days} days after the first open"
        ));
    }
    lines.join("\n")
}

#[cfg(test)]
mod usage_tests {
    use super::*;

    #[test]
    fn the_summary_names_only_what_happened() {
        let report = json!({"first_opened":"2026-10-20","weeks_with_meetings":1,"totals":{"recording_started":2,"transcript_ready":1},"first_run":{"minutes_from_first_open_to_first_recording":4,"model_download_minutes":null}});
        let summary = usage_summary(&report);
        assert!(summary.contains("Recordings: 2 · transcripts: 1"));
        assert!(summary.contains("First run: 4 min to the first recording"));
        assert!(!summary.contains("Briefs"));
        assert!(!summary.contains("notes"));
    }
}
