mod actions;
mod automation;
mod dialogs;
mod events;
mod guide;
mod library;
mod meeting;
mod menus;
mod models;
mod notes;
mod playback;
mod privacy;
mod projects;
mod replies;
mod settings;
mod share;
mod templates;
mod timeline;
mod workflow;
mod workspace_tools;
use crate::{
    engine::Engine, AppWindow, BriefItem, BriefSection, ClipRow, DetailRow, EventCard, FormRow,
    LaneRow, MeetingRow, NoteBlock, NoteHighlight, ResultRow, State, Theme, TickRow, TranscriptRow,
};
use serde_json::{json, Value};
use slint::{Color, ComponentHandle, Model, ModelRc, VecModel};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet, VecDeque},
    rc::Rc,
    time::Duration,
};

thread_local! { static CONTROLLER: RefCell<Option<Controller>> = const { RefCell::new(None) }; }
pub(crate) fn with_controller(f: impl FnOnce(&mut Controller)) {
    CONTROLLER.with(|slot| {
        if let Some(controller) = slot.borrow_mut().as_mut() {
            f(controller);
        }
    });
}
pub(crate) fn install(controller: Controller) {
    CONTROLLER.with(|slot| *slot.borrow_mut() = Some(controller));
}
fn text(value: &Value, key: &str) -> String {
    value[key].as_str().unwrap_or_default().to_owned()
}
fn result_id(value: &Value) -> String {
    let id = text(value, "id");
    if id.is_empty() {
        text(value, "meeting_id")
    } else {
        id
    }
}
fn short_date(input: &str) -> String {
    input
        .get(..10)
        .and_then(|s| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
        .map(|date| date.format("%-d %b").to_string())
        .unwrap_or_default()
}
fn library_date(input: &str) -> String {
    input
        .get(..10)
        .and_then(|s| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
        .map(|date| date.format("%b %-d, %Y").to_string())
        .unwrap_or_default()
}
fn array(value: &Value) -> Vec<Value> {
    value.as_array().cloned().unwrap_or_default()
}
fn model<T: Clone + 'static>(rows: Vec<T>) -> ModelRc<T> {
    Rc::new(VecModel::from(rows)).into()
}
fn time(seconds: f64, hours: bool) -> String {
    let seconds = seconds.max(0.0) as u64;
    if hours || seconds >= 3600 {
        format!(
            "{:02}:{:02}:{:02}",
            seconds / 3600,
            seconds / 60 % 60,
            seconds % 60
        )
    } else {
        format!("{:02}:{:02}", seconds / 60, seconds % 60)
    }
}
fn tone(index: usize) -> Color {
    crate::waveform::chip(index)
}
/// Shorten a name for a button, which has no room to wrap.
fn clip(input: &str, limit: usize) -> String {
    if input.chars().count() <= limit {
        input.into()
    } else {
        format!(
            "{}…",
            input.chars().take(limit - 1).collect::<String>().trim_end()
        )
    }
}

#[derive(Default)]
struct Meeting {
    metadata: Option<Value>,
    lines: Option<Vec<Value>>,
    total: usize,
    loading_more: bool,
    summary: Option<Value>,
    timeline: Option<Value>,
    peaks: Vec<u8>,
    peaks_loaded: bool,
    marks: Vec<Value>,
    rows: Option<ModelRc<TranscriptRow>>,
    lane_images: Option<(usize, u32, bool, ModelRc<LaneRow>)>,
}

pub(crate) struct Controller {
    workflow: workflow::Workflow,
    automation: automation::Automation,
    sound_check_generation: u64,
    sound_check_active: bool,
    sound_check_playback: bool,
    projects: Vec<Value>,
    project_membership: Vec<Value>,
    project_suggestions: Vec<Value>,
    hub_rows: Vec<Value>,
    hub_versions: Vec<Value>,
    translations: HashMap<String, String>,
    hub_options: Value,
    pub engine: Engine,
    overviews: Vec<Value>,
    organization: HashMap<String, Value>,
    cache: HashMap<String, Meeting>,
    recent: VecDeque<String>,
    active: String,
    settings: HashMap<String, Value>,
    results: Vec<Value>,
    dialog: (String, Value),
    choice_rows: Vec<DetailRow>,
    selected: Option<Value>,
    save_timer: slint::Timer,
    tick_timer: slint::Timer,
    recording_timer: slint::Timer,
    search_timer: slint::Timer,
    drafts: crate::drafts::Drafts,
    closing: bool,
    lane_width: usize,
    position: f64,
    duration: f64,
    audio_loaded: bool,
    process_id: Option<String>,
    selected_template: String,
    list_filter: String,
    mic_ids: Vec<Option<String>>,
    system_ids: Vec<Option<String>>,
    pending_export: Option<Value>,
    note_edit_base: Option<String>,
    solo_speaker: Option<String>,
    muted_speakers: HashSet<String>,
    playback_filter: crate::playback::Filter,
    live_completed: HashSet<u64>,
    library_revision: u64,
    library_dirty: bool,
    library_refresh_pending: bool,
    template_draft: Value,
    answers: Vec<Value>,
    word_track: Option<(String, String, crate::playback::WordTrack)>,
}

impl Controller {
    pub(crate) fn new(engine: Engine, selected_template: String) -> Self {
        Self {
            workflow: Default::default(),
            automation: Default::default(),
            sound_check_generation: 0,
            sound_check_active: false,
            sound_check_playback: false,
            projects: Vec::new(),
            project_membership: Vec::new(),
            project_suggestions: Vec::new(),
            hub_rows: Vec::new(),
            hub_versions: Vec::new(),
            translations: HashMap::new(),
            hub_options: json!({}),
            engine,
            overviews: Vec::new(),
            organization: HashMap::new(),
            cache: HashMap::new(),
            recent: VecDeque::new(),
            active: String::new(),
            settings: HashMap::new(),
            results: Vec::new(),
            dialog: Default::default(),
            choice_rows: Vec::new(),
            selected: None,
            save_timer: Default::default(),
            tick_timer: Default::default(),
            recording_timer: Default::default(),
            search_timer: Default::default(),
            drafts: Default::default(),
            closing: false,
            lane_width: 700,
            position: 0.0,
            duration: 0.0,
            audio_loaded: false,
            process_id: None,
            selected_template,
            list_filter: "active".into(),
            mic_ids: vec![None],
            system_ids: vec![None],
            pending_export: None,
            note_edit_base: None,
            solo_speaker: None,
            muted_speakers: HashSet::new(),
            playback_filter: Default::default(),
            live_completed: Default::default(),
            library_revision: 0,
            library_dirty: false,
            library_refresh_pending: false,
            template_draft: Value::Null,
            answers: Vec::new(),
            word_track: None,
        }
    }

    fn request(&self, ui: &AppWindow, command: &str, args: Value, tag: &str, context: &str) {
        let weak = ui.as_weak();
        let context = if tag.starts_with("audio") {
            self.active.clone()
        } else {
            context.to_owned()
        };
        let tag = tag.to_owned();
        self.engine.call(command, args, move |result| {
            let _ = weak.upgrade_in_event_loop(move |ui| {
                with_controller(|controller| controller.reply(&ui, &tag, &context, result))
            });
        });
    }

    pub(crate) fn initialize(&mut self, ui: &AppWindow) {
        self.workflow_initialize(ui);
        self.automation_initialize(ui);
        self.request(
            ui,
            "workspace_meeting_watch",
            json!({"settings":null}),
            "hub-watch-status",
            "",
        );
        self.request(
            ui,
            "workspace_project",
            json!({"action":{"op":"list"}}),
            "hub-projects",
            "",
        );
        ui.global::<State>().set_recording_title(
            format!("Meeting on {}", chrono::Local::now().format("%d %b, %H:%M")).into(),
        );
        ui.global::<State>()
            .set_recording_date(chrono::Local::now().format("%d %b").to_string().into());
        self.refresh_list(ui);
        for command in [
            "api_get_model_config",
            "api_get_transcript_config",
            "api_get_custom_openai_config",
            "get_onboarding_status",
            "get_recording_preferences",
            "get_notification_settings",
            "get_audio_devices",
            "api_list_templates",
            "speech_get_settings",
            "integrations_get_settings",
            "library_get_settings",
        ] {
            self.request(ui, command, json!({}), "setting", command);
        }
    }
}

fn summary_markdown(value: &Value) -> String {
    overtalk_documents::notes_markdown(&value["data"]).unwrap_or_default()
}
