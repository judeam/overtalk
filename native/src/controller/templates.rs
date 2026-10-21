use super::*;
use crate::TemplateSectionRow;

impl Controller {
    pub(super) fn template_action(&mut self, ui: &AppWindow, action: &str, value: &str) {
        let st = ui.global::<State>();
        match action {
            "template-editor" => self.request(
                ui,
                "api_get_template",
                json!({"templateId":self.selected_template}),
                "template-loaded",
                "",
            ),
            "template-load" => {
                if let Some(template) = self
                    .settings
                    .get("api_list_templates")
                    .and_then(Value::as_array)
                    .and_then(|rows| {
                        value
                            .parse::<usize>()
                            .ok()
                            .and_then(|index| rows.get(index))
                    })
                {
                    self.request(
                        ui,
                        "api_get_template",
                        json!({"templateId":template["id"]}),
                        "template-loaded",
                        "",
                    );
                }
            }
            "template-new" => {
                self.template_draft = json!({"id":null,"is_custom":true,"template":{"name":"","description":"","sections":[{"title":"Key points","instruction":"Summarize the main discussion and preserve source references.","format":"list"}]}});
                self.render_template(ui);
            }
            "template-copy" => {
                self.sync_template_fields(ui);
                self.template_draft["id"] = Value::Null;
                self.template_draft["is_custom"] = true.into();
                let name = format!("{} (copy)", text(&self.template_draft["template"], "name"));
                self.template_draft["template"]["name"] = name.into();
                self.render_template(ui);
            }
            "template-add-section" => {
                self.sync_template_fields(ui);
                if let Some(sections) = self.template_draft["template"]["sections"].as_array_mut() {
                    if sections.len() < 32 {
                        sections.push(json!({"title":"","instruction":"","format":"list"}));
                    }
                }
                self.render_template(ui);
            }
            "template-remove-section" => {
                self.sync_template_fields(ui);
                if let Some(sections) = self.template_draft["template"]["sections"].as_array_mut() {
                    if let Some(index) = value
                        .parse::<usize>()
                        .ok()
                        .filter(|&index| sections.len() > 1 && index < sections.len())
                    {
                        sections.remove(index);
                    }
                }
                self.render_template(ui);
            }
            "template-delete" => {
                if let Some(id) = self.template_draft["id"].as_str().map(str::to_owned) {
                    self.dialog(
                        ui,
                        "template-delete",
                        "Delete template?",
                        "This removes your custom template. Existing meeting notes are preserved.",
                        "confirm",
                        id.into(),
                    );
                    st.set_modal_submit("Delete template".into());
                }
            }
            "template-import" => self.request(
                ui,
                "native_template_file",
                json!({"action":"import"}),
                "template-imported",
                "",
            ),
            "template-export" => {
                self.sync_template_fields(ui);
                self.request(
                    ui,
                    "native_template_file",
                    json!({"action":"export","template":self.template_draft["template"]}),
                    "template-exported",
                    "",
                );
            }
            _ => {}
        }
    }
    fn sync_template_fields(&mut self, ui: &AppWindow) {
        let st = ui.global::<State>();
        self.template_draft["template"]["name"] = st.get_template_name().as_str().into();
        self.template_draft["template"]["description"] =
            st.get_template_description().as_str().into();
    }
    pub(super) fn save_template(&mut self, ui: &AppWindow) {
        if !ui.global::<State>().get_template_custom() {
            ui.global::<State>().set_modal("".into());
            return;
        }
        self.sync_template_fields(ui);
        self.request(ui, "api_save_template", json!({"templateId":self.template_draft["id"],"templateJson":self.template_draft["template"].to_string()}), "template-saved", "");
    }
    pub(super) fn template_field(&mut self, key: &str, value: &str) {
        if self.template_draft["is_custom"] != true {
            return;
        }
        let Some((index, field)) = key.split_once(':') else {
            return;
        };
        if !matches!(field, "title" | "instruction" | "format" | "item_format") {
            return;
        }
        if let Some(section) = index
            .parse::<usize>()
            .ok()
            .and_then(|index| self.template_draft["template"]["sections"].get_mut(index))
        {
            section[field] = value.into();
            if field == "item_format" {
                section
                    .as_object_mut()
                    .map(|s| s.remove("example_item_format"));
            }
        }
    }
    pub(super) fn render_template(&self, ui: &AppWindow) {
        let st = ui.global::<State>();
        st.set_modal("template".into());
        st.set_modal_title("Summary templates".into());
        st.set_modal_text("Choose what to extract from your meetings.".into());
        st.set_modal_submit(
            if self.template_draft["is_custom"] == true {
                "Save template"
            } else {
                "Done"
            }
            .into(),
        );
        st.set_template_custom(self.template_draft["is_custom"] == true);
        st.set_template_name(text(&self.template_draft["template"], "name").into());
        st.set_template_description(text(&self.template_draft["template"], "description").into());
        let templates = self
            .settings
            .get("api_list_templates")
            .map(array)
            .unwrap_or_default();
        st.set_template_index(
            templates
                .iter()
                .position(|row| row["id"] == self.template_draft["id"])
                .unwrap_or(0) as i32,
        );
        st.set_template_names(model(
            templates
                .iter()
                .map(|row| text(row, "name").into())
                .collect(),
        ));
        st.set_template_sections(model(
            array(&self.template_draft["template"]["sections"])
                .iter()
                .enumerate()
                .map(|(index, row)| TemplateSectionRow {
                    index: index as i32,
                    title: text(row, "title").into(),
                    instruction: text(row, "instruction").into(),
                    format: match text(row, "format").as_str() {
                        "paragraph" => 0,
                        "string" => 2,
                        _ => 1,
                    },
                    item_format: row["item_format"]
                        .as_str()
                        .or_else(|| row["example_item_format"].as_str())
                        .unwrap_or_default()
                        .into(),
                })
                .collect(),
        ));
    }
    pub(super) fn template_reply(&mut self, ui: &AppWindow, tag: &str, value: Value) {
        match tag {
            "template-loaded" | "template-saved" => {
                if tag == "template-saved" {
                    self.selected_template = text(&value, "id");
                    ui.global::<State>()
                        .set_summary_template_id(self.selected_template.clone().into());
                    ui.global::<State>()
                        .set_selected_template(text(&value["template"], "name").into());
                    ui.global::<State>().set_message("Template saved".into());
                    self.request(
                        ui,
                        "api_list_templates",
                        json!({}),
                        "setting",
                        "api_list_templates",
                    );
                }
                self.template_draft = value;
                self.render_template(ui);
            }
            "template-imported" if !value.is_null() => {
                self.template_draft = json!({"id":null,"is_custom":true,"template":value});
                self.render_template(ui);
            }
            "template-deleted" => {
                self.selected_template = "standard_meeting".into();
                ui.global::<State>()
                    .set_summary_template_id(self.selected_template.clone().into());
                ui.global::<State>().set_modal("".into());
                self.request(
                    ui,
                    "api_list_templates",
                    json!({}),
                    "setting",
                    "api_list_templates",
                );
            }
            "template-exported" if !value.is_null() => {
                ui.global::<State>().set_message("Template exported".into())
            }
            _ => {}
        }
    }
}
