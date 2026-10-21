//! Exercise the real shared menu, including pointer hit targets and keyboard activation.
use overtalk_native::{AppWindow, DetailRow, State, Theme};
use slint::platform::{
    software_renderer::{MinimalSoftwareWindow, RepaintBufferType},
    Platform, PlatformError, PointerEventButton, WindowAdapter, WindowEvent,
};
use slint::{ComponentHandle, LogicalPosition, ModelRc, VecModel};
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
fn described_menu_uses_real_hit_targets_and_blocks_the_workspace() {
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    let time = Rc::new(Cell::new(Duration::ZERO));
    slint::platform::set_platform(Box::new(Headless(window.clone(), time.clone()))).unwrap();
    let ui = AppWindow::new().unwrap();
    let state = ui.global::<State>();
    let events = Rc::new(RefCell::new(Vec::new()));
    state.on_action({
        let events = events.clone();
        move |action, value| {
            events
                .borrow_mut()
                .push((action.to_string(), value.to_string()))
        }
    });
    state.set_reduced_motion(true);
    state.set_modal("choice".into());
    state.set_modal_title("Meeting tools".into());
    state.set_choice_total(1);
    state.set_choice_height(78);
    state.set_modal_choices(ModelRc::from(Rc::new(VecModel::from(vec![DetailRow {
        id: "hub-history".into(),
        title: "Notes history".into(),
        description: "Compare saved versions and restore earlier notes.".into(),
        value: "history".into(),
        ..Default::default()
    }]))));
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
            "The modal must block underlying controls"
        );
        let scale = ui.global::<Theme>().get_scale();
        let top = (height as f32 - 178.0 * scale) / 2.0;
        click(&ui, &time, width as f32 / 2.0, top + 113.0 * scale);
        assert_eq!(
            &*events.borrow(),
            &[("modal-choice".into(), "hub-history".into())]
        );
        ui.window()
            .dispatch_event(WindowEvent::KeyPressed { text: "\n".into() });
        assert_eq!(
            events.borrow().len(),
            2,
            "The focused option must support Enter"
        );
    }
    // Connection cards use the same pointer/keyboard split. A focused overlay
    // must not swallow the click that opens account setup.
    window.set_size(slint::PhysicalSize::new(760, 540));
    state.set_modal("connection-picker".into());
    state.set_modal_title("Connect your tools".into());
    state.set_modal_choices(ModelRc::from(Rc::new(VecModel::from(vec![DetailRow {
        id: "linear_mcp".into(),
        title: "Linear".into(),
        ..Default::default()
    }]))));
    slint::platform::update_timers_and_animations();
    let mut pixels = vec![slint::Rgb8Pixel::default(); 760 * 540];
    window.draw_if_needed(|r| {
        r.render(&mut pixels, 760);
    });
    events.borrow_mut().clear();
    click(&ui, &time, 380.0, 225.0);
    assert_eq!(
        &*events.borrow(),
        &[("modal-choice".into(), "linear_mcp".into())]
    );
}
