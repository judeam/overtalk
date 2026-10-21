//! Playback and timeline regressions through the production UI and controller.
//! Requires OVERTALK_FIXTURE, OVERTALK_SERVICE and an isolated XDG_DATA_HOME.
use overtalk_native::{AppWindow, State};
use slint::platform::{
    software_renderer::{MinimalSoftwareWindow, RepaintBufferType},
    EventLoopProxy, Platform, PlatformError, WindowAdapter,
};
use slint::{ComponentHandle, Model, Rgb8Pixel};
use std::{
    cell::RefCell,
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
        let started = Instant::now();
        let mut stage = 0;
        let mut stage_at = Instant::now();
        let mut pixels = vec![Rgb8Pixel::default(); 1280 * 720];
        while !self.quit.load(Ordering::Acquire) {
            if let Ok(task) = self.receiver.recv_timeout(Duration::from_millis(5)) {
                task();
            }
            while let Ok(task) = self.receiver.try_recv() {
                task();
            }
            slint::platform::update_timers_and_animations();
            let ui = self
                .capture
                .borrow()
                .ui
                .as_ref()
                .and_then(slint::Weak::upgrade);
            if let Some(ui) = ui {
                let st = ui.global::<State>();
                assert!(
                    st.get_error().is_empty(),
                    "stage {stage}: {}",
                    st.get_error()
                );
                let settled = stage_at.elapsed() > Duration::from_millis(200);
                match stage {
                    0 if st.get_engine_ready() && st.get_meetings().row_count() > 0 => {
                        st.invoke_action("open-meeting".into(), "m1".into());
                        stage = 1;
                    }
                    1 if !st.get_loading() && st.get_lanes().row_count() >= 5 => {
                        assert!(
                            st.get_meeting_people().starts_with("3 speakers"),
                            "speaker fallback"
                        );
                        assert_eq!(
                            st.get_playhead(),
                            0.0,
                            "selecting the default note moved playback"
                        );
                        st.invoke_action("seek-absolute".into(), "780".into());
                        stage = 2;
                        stage_at = Instant::now();
                    }
                    2 if settled && st.get_active_transcript_id() == "t156" => {
                        assert!(
                            st.get_transcripts().row_count() >= 200,
                            "playback did not load the next page"
                        );
                        check_visible(&st, &pixels);
                        st.set_follow_playback(false);
                        st.set_transcript_scroll_y(0.0);
                        st.invoke_action("seek-absolute".into(), "250".into());
                        stage = 3;
                        stage_at = Instant::now();
                    }
                    3 if settled => {
                        assert_eq!(st.get_active_transcript_id(), "t50");
                        assert_eq!(
                            st.get_transcript_scroll_y(),
                            0.0,
                            "disabled follow changed scrolling"
                        );
                        st.set_follow_playback(true);
                        stage = 4;
                        stage_at = Instant::now();
                    }
                    4 if settled => {
                        check_visible(&st, &pixels);
                        st.set_reading_size(2);
                        self.window.set_size(slint::PhysicalSize::new(960, 720));
                        st.invoke_action("seek-absolute".into(), "10".into());
                        stage = 5;
                        stage_at = Instant::now();
                    }
                    5 if settled => {
                        assert_eq!(st.get_active_transcript_id(), "t2");
                        check_visible(&st, &pixels);
                        let lane = st.get_lanes().iter().find(|lane| lane.kind == 1).unwrap();
                        let group = lane
                            .clips
                            .iter()
                            .find(|clip| clip.count > 1)
                            .expect("crowded moments were not grouped");
                        st.invoke_action("select-note-group".into(), group.members);
                        stage = 6;
                        stage_at = Instant::now();
                    }
                    6 if settled => {
                        assert!(st.get_modal_choices().row_count() >= 2);
                        let id = st.get_modal_choices().row_data(1).unwrap().id;
                        st.invoke_action("modal-choice".into(), id.clone());
                        assert_eq!(st.get_selected_note_id(), id);
                        assert_eq!(
                            st.get_clock(),
                            "00:00:10",
                            "selecting a note moved playback"
                        );
                        st.invoke_action("toggle-play".into(), "".into());
                        stage = 7;
                        stage_at = Instant::now();
                    }
                    7 if settled && st.get_playing() => {
                        assert_eq!(
                            st.get_active_transcript_id(),
                            "t2",
                            "play resumed at the selected note instead of the playhead"
                        );
                        st.invoke_action("play-selection".into(), "".into());
                        stage = 8;
                        stage_at = Instant::now();
                    }
                    8 if settled => {
                        assert!(
                            st.get_playhead() > 0.03,
                            "play selected moment failed to seek"
                        );
                        check_visible(&st, &pixels);
                        st.invoke_action("stop-playback".into(), "".into());
                        stage = 9;
                        stage_at = Instant::now();
                    }
                    9 if settled => {
                        assert_eq!(st.get_active_transcript_id(), "t0");
                        check_visible(&st, &pixels);
                        println!("PASS: paginated playback follows measured rows, handles backwards seeks and disabled follow, restores following, preserves playback on note selection, and opens clustered notes");
                        self.quit.store(true, Ordering::Release);
                    }
                    _ => {}
                }
            }
            let size = self.window.size();
            pixels.resize((size.width * size.height) as usize, Rgb8Pixel::default());
            self.window.draw_if_needed(|r| {
                r.render(&mut pixels, self.window.size().width as usize);
            });
            assert!(
                started.elapsed() < Duration::from_secs(20),
                "playback regression timed out at {stage}"
            );
        }
        Ok(())
    }
}
fn check_visible(st: &State<'_>, pixels: &[Rgb8Pixel]) {
    let active_pixels = pixels
        .iter()
        .filter(|p| p.r == 26 && p.g == 48 && p.b == 51)
        .count();
    if let Ok(path) = std::env::var("OVERTALK_PLAYBACK_CAPTURE") {
        let bytes: Vec<u8> = pixels.iter().flat_map(|p| [p.r, p.g, p.b]).collect();
        image::save_buffer(
            path,
            &bytes,
            st.get_window_width() as u32,
            st.get_window_height() as u32,
            image::ColorType::Rgb8,
        )
        .unwrap();
    }
    assert!(st.get_transcript_viewport_height() >= 100.0);
    // Check the actual rendered highlight, not the virtual row's estimated geometry.
    // A full line of the active passage must be visible, including for paragraphs
    // taller than the viewport. All other rows use a different background color.
    assert!(
        active_pixels > 10_000,
        "the active passage was not painted in the visible transcript: {active_pixels} pixels"
    );
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    assert!(
        std::env::var_os("OVERTALK_FIXTURE").is_some()
            && std::env::var_os("XDG_DATA_HOME").is_some()
    );
    let capture = Rc::new(RefCell::new(Capture { ui: None }));
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
        ui.show().unwrap();
        window.set_size(slint::PhysicalSize::new(1280, 720));
    })
}
