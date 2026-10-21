//! Drive the production controller and a real engine against an isolated test library.
use overtalk_native::{AppWindow, State};
use slint::{
    platform::{
        software_renderer::{MinimalSoftwareWindow, RepaintBufferType},
        EventLoopProxy, Platform, PlatformError, WindowAdapter,
    },
    ComponentHandle, Model,
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    time::{Duration, Instant},
};
type Task = Box<dyn FnOnce() + Send>;
struct Proxy(mpsc::Sender<Task>, Arc<AtomicBool>);
impl EventLoopProxy for Proxy {
    fn quit_event_loop(&self) -> Result<(), slint::EventLoopError> {
        self.1.store(true, Ordering::Release);
        Ok(())
    }
    fn invoke_from_event_loop(&self, task: Task) -> Result<(), slint::EventLoopError> {
        self.0
            .send(task)
            .map_err(|_| slint::EventLoopError::EventLoopTerminated)
    }
}
struct Headless {
    window: Rc<MinimalSoftwareWindow>,
    ui: Rc<RefCell<Option<slint::Weak<AppWindow>>>>,
    sender: mpsc::Sender<Task>,
    receiver: mpsc::Receiver<Task>,
    quit: Arc<AtomicBool>,
    passed: Rc<Cell<bool>>,
    audio: bool,
}
impl Platform for Headless {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
        Ok(self.window.clone())
    }
    fn new_event_loop_proxy(&self) -> Option<Box<dyn EventLoopProxy>> {
        Some(Box::new(Proxy(self.sender.clone(), self.quit.clone())))
    }
    fn run_event_loop(&self) -> Result<(), PlatformError> {
        let started = Instant::now();
        let mut stage = 0;
        let mut meeting = String::new();
        let mut pixels = vec![slint::Rgb8Pixel::default(); 1280 * 720];
        while !self.quit.load(Ordering::Acquire) {
            if let Ok(task) = self.receiver.recv_timeout(Duration::from_millis(5)) {
                task();
            }
            while let Ok(task) = self.receiver.try_recv() {
                task();
            }
            slint::platform::update_timers_and_animations();
            if let Some(ui) = self.ui.borrow().as_ref().and_then(slint::Weak::upgrade) {
                let state = ui.global::<State>();
                assert!(
                    state.get_error().is_empty(),
                    "workflow stage {stage}: {}",
                    state.get_error()
                );
                match stage {
                    0 if state.get_engine_ready() && state.get_meetings().row_count() > 0 => {
                        meeting = state
                            .get_meetings()
                            .iter()
                            .find(|row| {
                                std::env::var("OVERTALK_VERIFY_MEETING")
                                    .map(|id| row.id == id)
                                    .unwrap_or_else(|_| {
                                        row.title.starts_with("Renamed native workflow")
                                    })
                            })
                            .or_else(|| state.get_meetings().row_data(0))
                            .unwrap()
                            .id
                            .to_string();
                        state.invoke_action("open-meeting".into(), meeting.clone().into());
                        stage = 1;
                    }
                    1 if !state.get_loading()
                        && state.get_transcripts().row_count() > 0
                        && !state.get_notes().is_empty() =>
                    {
                        assert!(!state.get_title().is_empty());
                        state.invoke_action("meeting-tab".into(), "1".into());
                        state.set_notes(
                            "# Native UI notes\n\nSaved by the production controller.".into(),
                        );
                        state.invoke_notes_edited(state.get_notes());
                        stage = 2;
                    }
                    2 if state.get_notes_status() == "All changes saved" => {
                        state.invoke_action("navigate".into(), "2".into());
                        state.set_query("Monday".into());
                        state.invoke_action("search".into(), "Monday".into());
                        stage = 3;
                    }
                    3 if !state.get_loading() && state.get_results().row_count() > 0 => {
                        state.set_filter_tag("nonexistent-tag".into());
                        state.invoke_action("search".into(), "Monday".into());
                        stage = 4;
                    }
                    4 if !state.get_loading() && state.get_results().row_count() == 0 => {
                        state.invoke_action("settings".into(), "2".into());
                        assert!(state.get_details().row_count() > 0);
                        state.invoke_action("choose-provider".into(), "".into());
                        assert!(state.get_modal_choices().row_count() >= 3);
                        state.invoke_action("modal-cancel".into(), "".into());
                        assert!(
                            state.get_details().row_count() == 1,
                            "provider menu replaced settings rows"
                        );
                        state.invoke_action("motion-mode".into(), "1".into());
                        assert!(state.get_reduced_motion());
                        state.invoke_action("template-editor".into(), "".into());
                        stage = 10;
                    }
                    10 if state.get_modal() == "template" => {
                        state.invoke_action("template-new".into(), "".into());
                        state.set_template_name("Native controller template".into());
                        state.set_template_description(
                            "Created by the real UI workflow check".into(),
                        );
                        state.invoke_action("modal-save".into(), "".into());
                        stage = 11;
                    }
                    11 if state.get_message() == "Template saved" => {
                        state.invoke_action("modal-cancel".into(), "".into());
                        state.invoke_action("open-meeting".into(), meeting.clone().into());
                        stage = 5;
                    }
                    5 if !state.get_loading() => {
                        if self.audio {
                            state.invoke_action("toggle-play".into(), "".into());
                            stage = 6;
                        } else {
                            stage = 8;
                        }
                    }
                    6 if state.get_playing() => {
                        state.invoke_action("toggle-play".into(), "".into());
                        stage = 7;
                    }
                    7 if !state.get_playing() => {
                        state.invoke_action("stop-playback".into(), "".into());
                        stage = 8;
                    }
                    8 => {
                        state.invoke_action("hub".into(), "".into());
                        state.invoke_action("hub-new-project".into(), "".into());
                        state.set_modal_value("Controller workflow project".into());
                        state.invoke_action("modal-save".into(), "".into());
                        stage = 20;
                    }
                    20 if state.get_project_name() == "Controller workflow project"
                        && state.get_project_index() > 0
                        && !state.get_hub_loading() =>
                    {
                        state.invoke_action("hub-primary".into(), "".into());
                        state.invoke_action("modal-choice".into(), meeting.clone().into());
                        stage = 21;
                    }
                    21 if !state.get_hub_loading()
                        && state.get_hub_rows().iter().any(|row| row.id == meeting) =>
                    {
                        state.invoke_action("hub-tab".into(), "1".into());
                        state.invoke_action("hub-primary".into(), "".into());
                        state.set_modal_value("Taylor".into());
                        state.set_modal_secondary("Design".into());
                        state.invoke_action("modal-save".into(), "".into());
                        stage = 22;
                    }
                    22 if !state.get_hub_loading()
                        && state.get_hub_rows().iter().any(|row| row.title == "Taylor") =>
                    {
                        println!("PASS: native project creation selects the project, assigns a meeting, and saves a person");
                        println!("PASS: native UI opens meetings, edits/saves notes, searches with filters, uses menus and settings{}", if self.audio { ", and plays/pauses audio" } else { "" });
                        self.passed.set(true);
                        self.quit.store(true, Ordering::Release);
                    }
                    _ => {}
                }
            }
            self.window.draw_if_needed(|renderer| {
                renderer.render(&mut pixels, 1280);
            });
            assert!(
                started.elapsed() < Duration::from_secs(30),
                "native workflow timed out at stage {stage}"
            );
        }
        Ok(())
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var_os("XDG_DATA_HOME").is_none() || std::env::var_os("OVERTALK_SERVICE").is_none()
    {
        return Err("Use scripts/verify-native-workflows.py with an isolated library".into());
    }
    let window = MinimalSoftwareWindow::new(RepaintBufferType::ReusedBuffer);
    let ui = Rc::new(RefCell::new(None));
    let (sender, receiver) = mpsc::channel();
    let passed = Rc::new(Cell::new(false));
    slint::platform::set_platform(Box::new(Headless {
        window: window.clone(),
        ui: ui.clone(),
        sender,
        receiver,
        quit: Arc::new(AtomicBool::new(false)),
        passed: passed.clone(),
        audio: std::env::args().any(|a| a == "--audio"),
    }))?;
    overtalk_native::run_with_setup(move |app| {
        *ui.borrow_mut() = Some(app.as_weak());
        app.show().unwrap();
        window.set_size(slint::PhysicalSize::new(1280, 720));
    })?;
    assert!(passed.get(), "native workflow exited before completion");
    Ok(())
}
