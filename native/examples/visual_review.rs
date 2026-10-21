//! Capture the production application and controller through a synthetic engine.
//! Requires OVERTALK_FIXTURE, OVERTALK_SERVICE and an isolated XDG_DATA_HOME.
use overtalk_native::{AppWindow, State};
use slint::platform::{
    software_renderer::{MinimalSoftwareWindow, RepaintBufferType},
    EventLoopProxy, Platform, PlatformError, WindowAdapter,
};
use slint::{ComponentHandle, Model, Rgb8Pixel};
use std::{
    cell::RefCell,
    path::PathBuf,
    rc::Rc,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    time::{Duration, Instant},
};
type Task = Box<dyn FnOnce() + Send>;
struct Proxy {
    sender: mpsc::Sender<Task>,
    quit: Arc<AtomicBool>,
}
impl EventLoopProxy for Proxy {
    fn quit_event_loop(&self) -> Result<(), slint::EventLoopError> {
        self.quit.store(true, Ordering::Release);
        Ok(())
    }
    fn invoke_from_event_loop(&self, event: Task) -> Result<(), slint::EventLoopError> {
        self.sender
            .send(event)
            .map_err(|_| slint::EventLoopError::EventLoopTerminated)
    }
}
struct Capture {
    ui: Option<slint::Weak<AppWindow>>,
    width: u32,
    height: u32,
    view: i32,
    output: PathBuf,
    phase: u8,
    ready_at: Option<Instant>,
    demo: Option<Demo>,
}

// A bounded, reproducible README demo of the actual controller. No real library
// is opened: the same synthetic engine and isolated profile as still captures apply.
struct Demo {
    started: Option<Instant>,
    frames: usize,
}
impl Demo {
    fn capture(
        &mut self,
        ui: &AppWindow,
        window: &MinimalSoftwareWindow,
        pixels: &mut [Rgb8Pixel],
        size: (u32, u32),
        output: &std::path::Path,
    ) -> bool {
        let st = ui.global::<State>();
        let started = *self.started.get_or_insert_with(|| {
            let start = std::env::var("OVERTALK_VISUAL_DEMO_START").unwrap_or_else(|_| "72".into());
            st.invoke_action("seek-absolute".into(), start.into());
            st.invoke_action("toggle-play".into(), "".into());
            Instant::now()
        });
        if started.elapsed() < Duration::from_millis(self.frames as u64 * 100) {
            return false;
        }
        if self.frames == 20 {
            st.invoke_action("meeting-tab".into(), "1".into());
        }
        if self.frames == 30 {
            st.invoke_action("toggle-play".into(), "".into());
        }
        window.draw_if_needed(|renderer| {
            renderer.render(pixels, size.0 as usize);
        });
        let bytes: Vec<u8> = pixels.iter().flat_map(|p| [p.r, p.g, p.b]).collect();
        image::save_buffer(
            output.join(format!("frame-{:03}.png", self.frames)),
            &bytes,
            size.0,
            size.1,
            image::ColorType::Rgb8,
        )
        .expect("write demo frame");
        self.frames += 1;
        self.frames == 48
    }
}
// Project lists that select their first row before capture.
fn hub_list(view: i32) -> bool {
    (10..=14).contains(&view) || view == 70
}
// Project views that wait for the Coming up cards.
fn upcoming(view: i32) -> bool {
    (64..=67).contains(&view) || (73..=78).contains(&view)
}
struct Headless {
    window: Rc<MinimalSoftwareWindow>,
    sender: mpsc::Sender<Task>,
    receiver: mpsc::Receiver<Task>,
    quit: Arc<AtomicBool>,
    capture: Rc<RefCell<Capture>>,
}
impl Platform for Headless {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
        Ok(self.window.clone())
    }
    fn new_event_loop_proxy(&self) -> Option<Box<dyn EventLoopProxy>> {
        Some(Box::new(Proxy {
            sender: self.sender.clone(),
            quit: self.quit.clone(),
        }))
    }
    fn run_event_loop(&self) -> Result<(), PlatformError> {
        let start = Instant::now();
        let c = self.capture.borrow();
        let width = c.width;
        let height = c.height;
        drop(c);
        let mut pixels = vec![Rgb8Pixel::default(); width as usize * height as usize];
        while !self.quit.load(Ordering::Acquire) {
            // Checked first: views that wait for a reply skip the rest of the loop.
            assert!(
                start.elapsed() < Duration::from_secs(20),
                "synthetic app did not reach the requested view"
            );
            if let Ok(task) = self.receiver.recv_timeout(Duration::from_millis(5)) {
                task();
            }
            while let Ok(task) = self.receiver.try_recv() {
                task();
            }
            slint::platform::update_timers_and_animations();
            let mut c = self.capture.borrow_mut();
            if let Some(ui) = c.ui.as_ref().and_then(slint::Weak::upgrade) {
                let st = ui.global::<State>();
                if c.phase == 0 && st.get_engine_ready() && st.get_meetings().row_count() > 0 {
                    match c.view {
                        62 | 63 => st.invoke_action("settings".into(), "3".into()),
                        71 => st.invoke_action("settings".into(), "5".into()),
                        72 => st.invoke_action("settings".into(), "0".into()),
                        10..=14 | 16..=21 | 26..=30 | 70 => {
                            st.invoke_action("hub".into(), "".into())
                        }
                        1
                        | 8
                        | 9
                        | 25
                        | 31
                        | 33..=35
                        | 37
                        | 50
                        | 53
                        | 54
                        | 55
                        | 68
                        | 69
                        | 91
                        | 92
                        | 95..=97 => st.invoke_action("open-meeting".into(), "m1".into()),
                        2 => st.invoke_action("navigate".into(), "2".into()),
                        38..=40 => st.invoke_action(
                            "navigate".into(),
                            (if c.view == 38 {
                                7
                            } else if c.view == 39 {
                                6
                            } else {
                                5
                            })
                            .to_string()
                            .into(),
                        ),
                        41..=45 | 49 => st.invoke_action("settings".into(), "0".into()),
                        58..=60 | 94 => st.invoke_action("settings".into(), "7".into()),
                        61 | 90 => st.invoke_action("start-recording".into(), "".into()),
                        46..=48 | 52 => st.invoke_action("hub".into(), "".into()),
                        57 => st.invoke_action("audio-sources".into(), "".into()),
                        56 => st.invoke_action("new-recording".into(), "".into()),
                        51 => st.invoke_action("navigate".into(), "6".into()),
                        3 => st.invoke_action("settings".into(), "0".into()),
                        15 => st.invoke_action("settings".into(), "3".into()),
                        22..=24 => st.invoke_action("guide".into(), "".into()),
                        98 => st.invoke_action("about".into(), "".into()),
                        _ => st.invoke_action("home".into(), "".into()),
                    };
                    c.phase = 1;
                }
                let ready = match c.view {
                    10..=14 | 16..=21 | 26..=30 | 70 => st.get_project_names().row_count() > 1,
                    1
                    | 8
                    | 9
                    | 25
                    | 31
                    | 33..=35
                    | 37
                    | 50
                    | 53
                    | 54
                    | 55
                    | 68
                    | 69
                    | 91
                    | 92
                    | 95..=97 => {
                        st.get_transcripts().row_count() > 0
                            && st.get_lanes().row_count() >= 5
                            && !st.get_notes().is_empty()
                            && (!matches!(c.view, 68 | 69)
                                || st.get_meeting_project_state() == "suggested")
                            && (!matches!(c.view, 91 | 92) || !st.get_meeting_notice().is_empty())
                    }
                    71 => st.get_view() == 3 && st.get_edition_briefs(),
                    72 => st.get_view() == 3 && st.get_edition_name() == "Overtalk",
                    view if upcoming(view) => {
                        st.get_upcoming_meetings().row_count() >= 3 && st.get_edition_auto_assign()
                    }
                    2 => st.get_results().row_count() > 0,
                    3 | 15 | 41..=45 | 49 => st.get_view() == 3,
                    58..=60 | 94 => {
                        st.get_view() == 3
                            && st.get_privacy_summary() != "Checking privacy controls…"
                    }
                    46..=48 | 52 => st.get_project_names().row_count() > 1,
                    _ => st.get_speech_model() != "Local speech",
                };
                if c.phase == 1 && ready {
                    if std::env::var_os("OVERTALK_VISUAL_LIGHT").is_some() {
                        st.set_dark(false);
                    }
                    if c.view == 25 {
                        let mut row = st
                            .get_transcripts()
                            .row_data(0)
                            .expect("fixture transcript");
                        row.body = "Hello everyone. 2040ユーロは以前の案です。 Let's review the updated plan together.".into();
                        let start = row.body.find("以前").unwrap();
                        st.set_active_transcript_id(row.id.clone());
                        st.set_active_transcript_index(0);
                        st.set_active_word_start(start as i32);
                        st.set_active_word_end((start + "以前".len()) as i32);
                        st.get_transcripts().set_row_data(0, row);
                    }
                    if c.view == 59 {
                        st.invoke_action("privacy-policy".into(), "".into());
                        assert_eq!(st.get_modal_title(), "Recording & data rules");
                    }
                    if c.view == 94 {
                        st.invoke_action("privacy-usage".into(), "".into());
                    }
                    if c.view == 60 {
                        st.invoke_action("privacy-voices".into(), "".into());
                        st.invoke_action("modal-choice".into(), "add".into());
                        assert_eq!(st.get_modal_title(), "Record voice authorization");
                    }
                    if c.view == 92 {
                        st.invoke_action("meeting-details".into(), "".into());
                        assert!(st.get_modal_value().contains("(notice copied to chat)"));
                    }
                    if (22..=24).contains(&c.view) {
                        assert_eq!(st.get_view(), 4);
                        assert_eq!(st.get_guide_chapters().row_count(), 13);
                        st.invoke_action("guide-chapter".into(), "1".into());
                        assert_eq!(st.get_guide_steps().row_count(), 5);
                        st.invoke_action("guide-read".into(), "".into());
                        assert!(st.get_guide_is_read());
                        assert_eq!(st.get_guide_read_count(), 1);
                        st.invoke_action("guide-search".into(), "HEADPHONES".into());
                        assert_eq!(st.get_guide_chapters().row_count(), 1);
                        assert_eq!(st.get_guide_index(), 0);
                        st.invoke_action("guide-search".into(), "no-such-feature".into());
                        assert_eq!(st.get_guide_chapters().row_count(), 0);
                        st.invoke_action("guide-chapter".into(), "0".into());
                        if c.view == 23 {
                            st.invoke_action("guide-chapter".into(), "1".into());
                        }
                        if c.view == 24 {
                            st.invoke_action("guide-search".into(), "speakers".into());
                        }
                    }
                    if (16..=21).contains(&c.view) || (26..=30).contains(&c.view) {
                        st.invoke_action("hub-project".into(), "1".into());
                        st.invoke_action("hub-tab".into(), "8".into());
                        if c.view != 21 {
                            st.invoke_action("hub-connection-options".into(), "".into());
                            assert_eq!(st.get_modal(), "connection-picker");
                            st.invoke_action("hub-connection-search".into(), "notion".into());
                            assert_eq!(st.get_modal_choices().row_count(), 1);
                            st.invoke_action(
                                "hub-connection-search".into(),
                                "no-such-provider".into(),
                            );
                            assert_eq!(st.get_modal_choices().row_count(), 0);
                            st.invoke_action("hub-connection-search".into(), "".into());
                            assert_eq!(st.get_modal_choices().row_count(), 7);
                        }
                        if (17..=20).contains(&c.view) {
                            let provider = ["linear_mcp", "atlassian_mcp", "google", "confluence"]
                                [(c.view - 17) as usize];
                            st.invoke_action("modal-choice".into(), provider.into());
                            assert_eq!(st.get_modal(), "connection-setup");
                            assert_eq!(st.get_connection_provider(), provider);
                            assert!(!st.get_form_rows().iter().any(|row| row.key == "/provider"));
                            assert_eq!(
                                st.get_form_rows().row_count(),
                                if provider == "confluence" { 4 } else { 1 }
                            );
                        }
                    }
                    if (26..=30).contains(&c.view) {
                        let provider = [
                            "microsoft",
                            "slack",
                            "notion_mcp",
                            "github",
                            "github-custom",
                        ][(c.view - 26) as usize];
                        st.invoke_action("modal-choice".into(), provider.into());
                        assert_eq!(st.get_modal(), "connection-setup");
                        assert!(st.get_connection_browser());
                        assert!(!st.get_form_rows().iter().any(|row| row.kind == 2));
                        if c.view == 30 {
                            assert!(st.get_connection_ready());
                            assert_eq!(st.get_form_rows().row_count(), 2);
                            st.set_connection_code("ABCD-EFGH".into());
                            st.set_connection_busy(true);
                        }
                    }
                    if c.view == 15 {
                        st.invoke_action("choose-provider".into(), "".into());
                        assert!(st
                            .get_modal_choices()
                            .iter()
                            .any(|row| row.id == "privatemode"));
                        st.invoke_action("modal-choice".into(), "privatemode".into());
                        assert_eq!(st.get_provider(), "privatemode");
                        assert_eq!(st.get_model(), "glm-5.3");
                        assert_eq!(st.get_endpoint(), "http://127.0.0.1:8080/v1");
                    }
                    if hub_list(c.view) {
                        st.invoke_action("hub-project".into(), "1".into());
                        st.invoke_action(
                            "hub-tab".into(),
                            match c.view {
                                11 => "1",
                                12 => "3",
                                13 => "4",
                                14 => "5",
                                70 => "9",
                                _ => "0",
                            }
                            .into(),
                        );
                    }
                    match c.view {
                        37 => {
                            st.invoke_action("hub-meeting-tools".into(), "".into());
                            assert_eq!(st.get_modal(), "choice");
                            assert!(st
                                .get_modal_choices()
                                .iter()
                                .all(|row| !row.description.is_empty()));
                            assert!(st.get_choice_height() < 600);
                        }
                        41..=45 => st.invoke_action(
                            "settings".into(),
                            ["2", "1", "4", "5", "6"][(c.view - 41) as usize].into(),
                        ),
                        46..=48 => {
                            st.invoke_action("hub-project".into(), "1".into());
                            st.invoke_action(
                                "hub-tab".into(),
                                ["2", "6", "7"][(c.view - 46) as usize].into(),
                            );
                        }
                        49 => {
                            st.invoke_action("settings".into(), "3".into());
                            st.invoke_action("choose-provider".into(), "".into());
                            let count = st.get_modal_choices().row_count();
                            st.invoke_action("choice-search".into(), "no-such-provider".into());
                            assert_eq!(st.get_modal_choices().row_count(), 0);
                            st.invoke_action("choice-search".into(), "".into());
                            assert_eq!(st.get_modal_choices().row_count(), count);
                        }
                        50 => st.invoke_action("export".into(), "".into()),
                        51 => st.invoke_action("new-task".into(), "".into()),
                        52 => st.invoke_action("hub-new-project".into(), "".into()),
                        53 => st.invoke_action("hub-redact-audio".into(), "".into()),
                        54 => st.invoke_action("hub-reading-options".into(), "".into()),
                        55 => st.invoke_action("rename-speaker".into(), "speaker-1".into()),
                        // Sharing a copy: the page with clips, then a redacted Overtalk file.
                        96 | 97 => {
                            st.invoke_action("share-meeting".into(), "".into());
                            assert_eq!(st.get_modal(), "share");
                            if c.view == 97 {
                                st.invoke_action("share-kind".into(), "1".into());
                                st.set_share_terms("Lena, Marco".into());
                                st.set_share_contacts(true);
                                st.set_share_ranges("00:12–00:24".into());
                                st.invoke_action("share-changed".into(), "".into());
                            }
                        }
                        95 => {
                            st.invoke_action("remember-voice".into(), "speaker-1".into());
                            assert_eq!(st.get_modal(), "form");
                            assert_eq!(st.get_modal_title(), "Remember this voice");
                        }
                        // The first card with a ready (66) or partial (67) brief, so the
                        // same views also work against a real engine's calendar.
                        66 | 67 => {
                            let wanted = if c.view == 66 { "ready" } else { "facts" };
                            let cards = st.get_upcoming_meetings();
                            let card = cards
                                .iter()
                                .find(|card| card.brief == wanted)
                                .or_else(|| cards.iter().find(|card| !card.brief.is_empty()))
                                .expect("an upcoming meeting with a brief");
                            st.invoke_action("automation-brief".into(), card.id);
                        }
                        69 => {
                            st.invoke_action("hub-assign".into(), "".into());
                            assert_eq!(
                                st.get_modal_choices().row_data(0).map(|row| row.id),
                                Some("suggestion:accept".into())
                            );
                        }
                        73 => {
                            st.invoke_action("list-options".into(), "".into());
                            st.invoke_action("modal-choice".into(), "needs-project".into());
                            assert_eq!(st.get_list_filter_label(), "NEEDS A PROJECT");
                        }
                        74 => st.invoke_action("list-options".into(), "".into()),
                        76 => {
                            st.invoke_action("automation-capture".into(), "".into());
                            assert_eq!(st.get_capture_project_next(), "Decide automatically");
                        }
                        77 => st.invoke_action("automation-event".into(), "call-2".into()),
                        // The fixture asserts the engine arguments of each choice.
                        78 => {
                            st.invoke_action("open-meeting".into(), "m1".into());
                            assert_eq!(st.get_meeting_project_state(), "suggested");
                            st.invoke_action("hub-assign".into(), "".into());
                            st.invoke_action("modal-choice".into(), "suggestion:accept".into());
                            st.invoke_action("automation-capture".into(), "".into());
                            st.invoke_action("modal-choice".into(), "project:orion".into());
                            assert_eq!(st.get_capture_project_next(), "Orion");
                            st.invoke_action("automation-event".into(), "call-2".into());
                            st.invoke_action("modal-choice".into(), "automatic".into());
                            st.invoke_action("workflow-join".into(), "call-1".into());
                        }
                        _ => {}
                    }
                    // View 78 has already joined an event, which names the recording.
                    if c.view == 0 || (upcoming(c.view) && c.view != 78) {
                        if let Ok(data) =
                            std::fs::read(std::env::var_os("OVERTALK_FIXTURE").expect("fixture"))
                        {
                            if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&data) {
                                if let Some(title) = value["visual"]["recording_title"].as_str() {
                                    st.set_recording_title(title.into());
                                }
                                if let Some(date) = value["visual"]["recording_date"].as_str() {
                                    st.set_recording_date(date.into());
                                }
                            }
                        }
                    }
                    if c.view == 32 || c.view == 93 {
                        st.set_setup_needed(true);
                        st.invoke_action("workflow-check".into(), "".into());
                    }
                    if (33..=35).contains(&c.view) {
                        st.invoke_action("meeting-tab".into(), "1".into());
                        st.invoke_action("workflow-panel".into(), (c.view - 32).to_string().into());
                        if c.view == 34 {
                            st.set_assist_query("When is the launch review?".into());
                            st.invoke_action("workflow-ask".into(), st.get_assist_query());
                        }
                        if c.view == 35 {
                            st.invoke_action("workflow-followup".into(), "".into());
                        }
                    }
                    if c.view == 36 || c.view == 65 {
                        st.invoke_action("workflow-join".into(), "call-1".into());
                        st.invoke_action("workflow-join".into(), "call-1".into());
                    }
                    if c.view == 8 {
                        st.invoke_action("meeting-tab".into(), "1".into());
                    }
                    if c.view == 31 {
                        st.invoke_action("meeting-tab".into(), "1".into());
                        st.invoke_action("hub-share-notes".into(), "".into());
                    }
                    if c.view == 9 {
                        st.invoke_action("meeting-tab".into(), "2".into());
                    }
                    c.phase = 2;
                    c.ready_at = Some(Instant::now());
                }
            }
            if c.phase == 2
                && hub_list(c.view)
                && c.ready_at
                    .is_some_and(|at| at.elapsed() > Duration::from_millis(300))
            {
                if let Some(ui) = c.ui.as_ref().and_then(slint::Weak::upgrade) {
                    let state = ui.global::<State>();
                    if !state.get_hub_loading() {
                        if let Some(row) = state.get_hub_rows().row_data(0) {
                            state.invoke_action("hub-select".into(), row.id);
                            c.phase = 3;
                            c.ready_at = Some(Instant::now());
                        }
                    }
                }
            }
            if c.view == 78 {
                if let Some(ui) = c.ui.as_ref().and_then(slint::Weak::upgrade) {
                    let st = ui.global::<State>();
                    if c.phase == 2 && st.get_modal() == "recording-notice" {
                        st.invoke_action("privacy-notice".into(), "copied".into());
                    } else if c.phase == 2
                        && st.get_recording()
                        && st.get_capture_project() == "Orion"
                    {
                        assert_eq!(st.get_capture_project_detail(), "Chosen by you");
                        st.invoke_action("automation-capture".into(), "".into());
                        assert!(!st
                            .get_modal_choices()
                            .iter()
                            .any(|row| row.id == "automatic"));
                        st.invoke_action("modal-choice".into(), "project:launch".into());
                        c.phase = 3;
                    } else if c.phase == 3 && st.get_capture_project() == "Product launch" {
                        c.phase = 4;
                        c.ready_at = Some(Instant::now());
                    }
                }
            }
            if c.view == 31 {
                if let Some(ui) = c.ui.as_ref().and_then(slint::Weak::upgrade) {
                    let st = ui.global::<State>();
                    if c.phase == 2 && st.get_modal_title() == "Send notes to…" {
                        st.invoke_action("modal-choice".into(), "slack-test".into());
                        c.phase = 3;
                    } else if c.phase == 3 && st.get_modal_title() == "Choose destination" {
                        st.invoke_action("modal-choice".into(), "C_DESIGN".into());
                        assert_eq!(st.get_modal_title(), "Prepare outgoing notes");
                        st.invoke_action("modal-save".into(), "".into());
                        c.phase = 4;
                    } else if c.phase == 4 && st.get_modal_title() == "Review outgoing change" {
                        assert!(st.get_modal_text().contains("#design-review"));
                        assert_eq!(st.get_modal_submit(), "Publish change");
                        c.phase = 5;
                        c.ready_at = Some(Instant::now());
                    }
                }
            }
            self.window.draw_if_needed(|r| {
                r.render(&mut pixels, width as usize);
            });
            if (hub_list(c.view) && c.phase == 3
                || !hub_list(c.view) && c.view != 31 && c.view != 78 && c.phase == 2
                || c.view == 31 && c.phase == 5
                || c.view == 78 && c.phase == 4)
                && (!(16..=21).contains(&c.view)
                    || c.ui
                        .as_ref()
                        .and_then(slint::Weak::upgrade)
                        .is_some_and(|ui| !ui.global::<State>().get_hub_loading()))
                && c.ready_at
                    .is_some_and(|t| t.elapsed() > Duration::from_millis(300))
            {
                if std::env::var_os("OVERTALK_VISUAL_DETAILS").is_some() {
                    if let Some(ui) = c.ui.as_ref().and_then(slint::Weak::upgrade) {
                        ui.global::<State>().set_hub_inspector_open(true);
                        self.window.draw_if_needed(|r| {
                            r.render(&mut pixels, width as usize);
                        });
                    }
                }
                if std::env::var_os("OVERTALK_VISUAL_NAV").is_some() {
                    if let Some(ui) = c.ui.as_ref().and_then(slint::Weak::upgrade) {
                        ui.set_compact_navigation_open(true);
                        self.window.draw_if_needed(|r| {
                            r.render(&mut pixels, width as usize);
                        });
                    }
                }
                if let Some(ui) = c.ui.as_ref().and_then(slint::Weak::upgrade) {
                    let st = ui.global::<State>();
                    // Joining asks for the recording notice before capture starts.
                    if (c.view == 36 || c.view == 65) && st.get_modal() == "recording-notice" {
                        st.invoke_action("privacy-notice".into(), "copied".into());
                    }
                    if ((c.view == 32 || c.view == 93) && st.get_setup_checking())
                        || (c.view == 94 && st.get_modal_title() != "Your usage report")
                        || ((c.view == 61 || c.view == 90) && st.get_modal() != "recording-notice")
                        || ((c.view == 34 || c.view == 35) && st.get_assist_busy())
                        || (c.view == 36 && !st.get_recording())
                        || (c.view == 65 && st.get_capture_project() != "Product launch")
                        || ((c.view == 66 || c.view == 67) && st.get_modal() != "brief")
                        || (c.view == 75 && !st.get_message().starts_with("Brief ready"))
                        || ((c.view == 96 || c.view == 97)
                            && !st.get_share_estimate().starts_with("About"))
                    {
                        continue;
                    }
                    if c.view == 61 || c.view == 90 {
                        assert_eq!(st.get_modal_title(), "Tell people you’re recording");
                        assert_eq!(
                            st.get_recording_notice_route(),
                            if c.view == 61 { "local" } else { "external" }
                        );
                        assert!(st.get_recording_notice_text().ends_with(if c.view == 61 {
                            "nothing is sent to a cloud service."
                        } else {
                            "sent to Privatemode to write the notes."
                        }));
                    }
                    if c.view == 36 {
                        assert!(st.get_recording());
                        assert_eq!(
                            st.get_recording_title(),
                            "Product review · launch readiness"
                        );
                    }
                    if c.view == 32 {
                        assert!(st.get_setup_speech_ready());
                        assert!(st.get_setup_notes_ready());
                    }
                    if c.view == 93 {
                        assert!(
                            st.get_setup_speech_downloading() && st.get_setup_notes_downloading()
                        );
                        assert!(st.get_setup_speech_detail().contains("34%"));
                    }
                    if c.view == 65 {
                        assert_eq!(st.get_capture_project_state(), "automatic");
                    }
                    if c.view == 66 && std::env::var_os("OVERTALK_VISUAL_PROJECTS").is_some() {
                        assert_eq!(st.get_brief_sections().row_count(), 5);
                        assert_eq!(st.get_brief_agenda().row_count(), 3);
                    }
                    if c.view == 34 {
                        assert!(st.get_assist_answer().contains("Friday"));
                        assert_eq!(st.get_assist_sources().row_count(), 1);
                    }
                    if c.view == 35 {
                        assert!(st.get_followup_draft().contains("Friday"));
                        assert!(st.get_followup_ready());
                    }
                }
                let output = c.output.clone();
                let demo_ui = c.ui.as_ref().and_then(slint::Weak::upgrade);
                if let Some(demo) = c.demo.as_mut() {
                    let ui = demo_ui.expect("demo UI");
                    if demo.capture(&ui, &self.window, &mut pixels, (width, height), &output) {
                        println!("Captured 48 synthetic demo frames in {}", output.display());
                        self.quit.store(true, Ordering::Release);
                    }
                    continue;
                }
                let bytes: Vec<u8> = pixels.iter().flat_map(|p| [p.r, p.g, p.b]).collect();
                image::save_buffer(&c.output, &bytes, width, height, image::ColorType::Rgb8)
                    .expect("write visual capture");
                println!(
                    "{}x{} view={} captured {}",
                    width,
                    height,
                    c.view,
                    c.output.display()
                );
                self.quit.store(true, Ordering::Release);
            }
        }
        Ok(())
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var_os("OVERTALK_FIXTURE").is_none() || std::env::var_os("XDG_DATA_HOME").is_none()
    {
        return Err(
            "Set OVERTALK_FIXTURE, OVERTALK_SERVICE and isolated XDG_DATA_HOME for visual capture"
                .into(),
        );
    }
    let args: Vec<_> = std::env::args().collect();
    let width = args
        .get(1)
        .and_then(|v| v.parse().ok())
        .unwrap_or(1280u32)
        .clamp(760, 8192);
    let height = args
        .get(2)
        .and_then(|v| v.parse().ok())
        .unwrap_or(720u32)
        .clamp(540, 8192);
    let view = args.get(3).and_then(|v| v.parse().ok()).unwrap_or(1i32);
    let output = args
        .get(4)
        .map(PathBuf::from)
        .unwrap_or_else(|| "/tmp/overtalk-visual.png".into());
    let demo = if std::env::var_os("OVERTALK_VISUAL_DEMO").is_some() {
        assert_eq!(view, 1, "the README demo starts in the transcript view");
        std::fs::create_dir_all(&output)?;
        Some(Demo {
            started: None,
            frames: 0,
        })
    } else {
        None
    };
    let capture = Rc::new(RefCell::new(Capture {
        ui: None,
        width,
        height,
        view,
        output,
        phase: 0,
        ready_at: None,
        demo,
    }));
    let window = MinimalSoftwareWindow::new(RepaintBufferType::ReusedBuffer);
    let (sender, receiver) = mpsc::channel();
    slint::platform::set_platform(Box::new(Headless {
        window: window.clone(),
        sender,
        receiver,
        quit: Arc::new(AtomicBool::new(false)),
        capture: capture.clone(),
    }))?;
    overtalk_native::run_with_setup(move |ui| {
        capture.borrow_mut().ui = Some(ui.as_weak());
        ui.show().expect("show software window");
        let scale_factor = std::env::var("OVERTALK_VISUAL_SCALE")
            .ok()
            .and_then(|value| value.parse::<f32>().ok())
            .unwrap_or(1.0);
        ui.window()
            .dispatch_event(slint::platform::WindowEvent::ScaleFactorChanged { scale_factor });
        window.set_size(slint::PhysicalSize::new(width, height));
    })
}
