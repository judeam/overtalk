//! The recording notice step: every choice is reachable in the smallest window,
//! in order, and the step blocks the workspace behind it.
use overtalk_native::{AppWindow, State, Theme};
use slint::platform::{
    software_renderer::{MinimalSoftwareWindow, RepaintBufferType},
    Platform, PlatformError, PointerEventButton, WindowAdapter, WindowEvent,
};
use slint::{ComponentHandle, LogicalPosition};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};

struct Headless(Rc<MinimalSoftwareWindow>, Rc<Cell<Duration>>);
impl Platform for Headless {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
        Ok(self.0.clone())
    }
    fn duration_since_start(&self) -> Duration {
        self.1.get()
    }
}
fn click(ui: &AppWindow, time: &Cell<Duration>, x: f32, y: f32) {
    let position = LogicalPosition::new(x, y);
    for event in [
        WindowEvent::PointerMoved { position },
        WindowEvent::PointerPressed {
            position,
            button: PointerEventButton::Left,
        },
        WindowEvent::PointerReleased {
            position,
            button: PointerEventButton::Left,
        },
    ] {
        ui.window().dispatch_event(event);
        time.set(time.get() + Duration::from_millis(150));
        slint::platform::update_timers_and_animations();
    }
}

#[test]
fn every_notice_choice_is_reachable_and_the_step_blocks_the_workspace() {
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    let time = Rc::new(Cell::new(Duration::ZERO));
    slint::platform::set_platform(Box::new(Headless(window.clone(), time.clone()))).unwrap();
    let ui = AppWindow::new().unwrap();
    let state = ui.global::<State>();
    let events = Rc::new(RefCell::new(Vec::<(String, String)>::new()));
    state.on_action({
        let events = events.clone();
        move |action, value| {
            events
                .borrow_mut()
                .push((action.to_string(), value.to_string()))
        }
    });
    state.set_reduced_motion(true);
    state.set_modal("recording-notice".into());
    state.set_modal_title("Tell people you’re recording".into());
    state.set_modal_text("Paste it into the call chat or read it aloud. How you told people is saved with this meeting.".into());
    state.set_recording_notice_text("I'm recording this call to take notes. It is recorded on my computer, and the audio is sent to Deepgram for transcription and the transcript to Anthropic to write the notes.".into());
    state.set_recording_notice_route("external".into());
    state.set_recording_notice_detail("Your setup: the audio is sent to Deepgram for transcription and the transcript to Anthropic to write the notes.".into());
    ui.show().unwrap();
    ui.window()
        .dispatch_event(WindowEvent::WindowActiveChanged(true));
    for (width, height) in [(760, 540), (1280, 720)] {
        window.set_size(slint::PhysicalSize::new(width, height));
        slint::platform::update_timers_and_animations();
        let mut pixels = vec![slint::Rgb8Pixel::default(); width as usize * height as usize];
        window.draw_if_needed(|r| {
            r.render(&mut pixels, width as usize);
        });
        events.borrow_mut().clear();
        click(&ui, &time, 4.0, 160.0);
        assert!(
            events.borrow().is_empty(),
            "The notice step must block underlying controls"
        );
        let scale = ui.global::<Theme>().get_scale();
        let dialog = (640.0 * scale).min(width as f32 - 32.0 * scale);
        let left = (width as f32 - dialog) / 2.0;
        let right = left + dialog - 24.0 * scale - 8.0;
        // Up from the bottom edge, the first control is the primary choice.
        let mut row = height as f32 - 2.0;
        while events.borrow().is_empty() && row > 0.0 {
            click(&ui, &time, right, row);
            row -= 4.0;
        }
        row += 4.0;
        assert_eq!(
            events.borrow().last(),
            Some(&("privacy-notice".into(), "copied".into()))
        );
        // Then every choice, right to left along the same row.
        let mut x = right;
        while x > left {
            click(&ui, &time, x, row);
            x -= 6.0;
        }
        let mut choices = events.borrow().clone();
        choices.dedup();
        assert_eq!(
            choices,
            ["copied", "read_aloud", "skipped"]
                .map(|method| ("privacy-notice".to_owned(), method.to_owned()))
        );
    }
    events.borrow_mut().clear();
    ui.window().dispatch_event(WindowEvent::KeyPressed {
        text: slint::platform::Key::Escape.into(),
    });
    assert_eq!(
        &*events.borrow(),
        &[("modal-cancel".into(), "".into())],
        "Escape closes the step without recording"
    );
}
