//! Motion must end and leave the native renderer asleep, including on weak PCs.
use overtalk_native::{AppWindow, LaneRow, State, TranscriptRow};
use slint::platform::{
    software_renderer::{MinimalSoftwareWindow, RepaintBufferType},
    Platform, PlatformError, WindowAdapter,
};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::{cell::Cell, rc::Rc, time::Duration};

struct Headless {
    window: Rc<MinimalSoftwareWindow>,
    time: Rc<Cell<Duration>>,
}
impl Platform for Headless {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
        Ok(self.window.clone())
    }
    fn duration_since_start(&self) -> Duration {
        self.time.get()
    }
}
fn paint(window: &MinimalSoftwareWindow) -> bool {
    slint::platform::update_timers_and_animations();
    let size = window.size();
    let mut rendered = false;
    window.draw_if_needed(|renderer| {
        let mut pixels = vec![slint::Rgb8Pixel::default(); (size.width * size.height) as usize];
        renderer.render(&mut pixels, size.width as usize);
        rendered = true;
    });
    rendered
}

#[test]
fn speaker_and_notification_motion_settles_and_respects_reduced_motion() {
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    let time = Rc::new(Cell::new(Duration::ZERO));
    slint::platform::set_platform(Box::new(Headless {
        window: window.clone(),
        time: time.clone(),
    }))
    .unwrap();
    let ui = AppWindow::new().unwrap();
    let state = ui.global::<State>();
    state.set_view(1);
    state.set_lanes(ModelRc::from(Rc::new(VecModel::from(vec![LaneRow {
        id: "speaker-1".into(),
        name: "Alex".into(),
        tone: slint::Color::from_rgb_u8(15, 154, 159),
        ..Default::default()
    }]))));
    ui.show().unwrap();
    window.set_size(slint::PhysicalSize::new(1280, 720));
    paint(&window);
    time.set(Duration::from_millis(500));
    paint(&window);
    state.set_playing(true);
    state.set_speaking_id("speaker-1".into());
    state.set_message("Saved locally".into());
    paint(&window);
    assert!(
        ui.window().has_active_animations(),
        "normal feedback should animate"
    );
    time.set(Duration::from_millis(750));
    paint(&window);
    assert!(!ui.window().has_active_animations(), "feedback must finish");
    assert!(
        !paint(&window),
        "an idle app must not request another frame"
    );

    state.set_reduced_motion(true);
    state.set_speaking_id("".into());
    state.set_message("".into());
    paint(&window);
    assert!(
        !ui.window().has_active_animations(),
        "reduced motion must be immediate"
    );
    assert!(
        !paint(&window),
        "reduced motion must also leave the renderer idle"
    );

    state.set_meeting_tab(1);
    state.set_reduced_motion(false);
    state.set_notes_generating(true);
    paint(&window);
    assert!(
        ui.window().has_active_animations(),
        "notes generation should animate its SVG"
    );
    time.set(Duration::from_millis(2800));
    paint(&window);
    state.set_notes_generating(false);
    paint(&window);
    time.set(Duration::from_millis(3200));
    paint(&window);
    assert!(
        !ui.window().has_active_animations(),
        "completed generation must stop animating"
    );
    assert!(
        !paint(&window),
        "completed generation must leave the renderer idle"
    );

    state.set_reduced_motion(true);
    state.set_notes_generating(true);
    paint(&window);
    time.set(Duration::from_millis(5200));
    assert!(
        !paint(&window),
        "reduced motion must not run the generation timer"
    );

    // Word focus uses shaped text geometry, settles between words, and does
    // not keep the app rendering after pause (or under reduced motion).
    state.set_notes_generating(false);
    state.set_meeting_tab(0);
    state.set_reduced_motion(false);
    state.set_follow_playback(true);
    state.set_transcripts(ModelRc::from(Rc::new(VecModel::from(vec![
        TranscriptRow {
            id: "words".into(),
            body: "Hello world café नमस्ते 日本語".into(),
            ..Default::default()
        },
    ]))));
    state.set_active_transcript_id("words".into());
    state.set_active_transcript_index(0);
    state.set_active_word_start(0);
    state.set_active_word_end(5);
    state.set_playing(true);
    paint(&window);
    time.set(Duration::from_millis(5600));
    paint(&window);
    state.set_active_word_start(6);
    state.set_active_word_end(11);
    paint(&window);
    assert!(
        ui.window().has_active_animations(),
        "word focus should transition"
    );
    time.set(Duration::from_millis(5800));
    paint(&window);
    state.set_playing(false);
    paint(&window);
    time.set(Duration::from_millis(6200));
    paint(&window);
    assert!(
        !paint(&window),
        "paused word focus must leave the renderer idle"
    );
    state.set_reduced_motion(true);
    state.set_active_word_start(12);
    state.set_active_word_end(17);
    paint(&window);
    assert!(!ui.window().has_active_animations());
}
