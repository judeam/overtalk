//! Exercise the real Slint components without a display server or a database.
use overtalk_native::{AppWindow, MeetingRow, State};
use slint::platform::{
    software_renderer::{MinimalSoftwareWindow, RepaintBufferType},
    Platform, PlatformError, PointerEventButton, WindowAdapter, WindowEvent,
};
use slint::{ComponentHandle, LogicalPosition, ModelRc, VecModel};
use std::{cell::RefCell, rc::Rc};
struct Headless(Rc<MinimalSoftwareWindow>);
impl Platform for Headless {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
        Ok(self.0.clone())
    }
}
fn paint(window: &MinimalSoftwareWindow) {
    slint::platform::update_timers_and_animations();
    let size = window.size();
    let mut pixels = vec![slint::Rgb8Pixel::default(); (size.width * size.height) as usize];
    window.draw_if_needed(|renderer| {
        renderer.render(&mut pixels, size.width as usize);
    });
}
fn click(ui: &AppWindow, window: &MinimalSoftwareWindow, x: f32, y: f32) {
    let position = LogicalPosition::new(x, y);
    ui.window()
        .dispatch_event(WindowEvent::PointerMoved { position });
    ui.window().dispatch_event(WindowEvent::PointerPressed {
        position,
        button: PointerEventButton::Left,
    });
    paint(window);
    ui.window().dispatch_event(WindowEvent::PointerReleased {
        position,
        button: PointerEventButton::Left,
    });
    paint(window);
}
#[test]
fn first_pointer_click_activates_navigation_and_controls_once() {
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(Headless(window.clone()))).unwrap();
    let ui = AppWindow::new().unwrap();
    ui.global::<State>().set_engine_ready(true);
    ui.global::<State>()
        .set_mic_devices(ModelRc::from(Rc::new(VecModel::from(vec![
            "Default microphone".into(),
        ]))));
    ui.global::<State>()
        .set_system_devices(ModelRc::from(Rc::new(VecModel::from(vec![
            "Default system audio".into(),
        ]))));
    ui.global::<State>()
        .set_meetings(ModelRc::from(Rc::new(VecModel::from(vec![MeetingRow {
            id: "fixture".into(),
            title: "A meeting".into(),
            subtitle: "14 Oct · 00:30".into(),
            pinned: false,
        }]))));
    let actions = Rc::new(RefCell::new(Vec::new()));
    let recorded = actions.clone();
    let weak = ui.as_weak();
    ui.global::<State>().on_action(move |action, value| {
        recorded
            .borrow_mut()
            .push((action.to_string(), value.to_string()));
        if let Some(ui) = weak.upgrade() {
            match action.as_str() {
                "navigate" => ui.global::<State>().set_view(2),
                "hub-tab" => ui.global::<State>().set_view(8),
                "home" | "new-recording" => ui.global::<State>().set_view(0),
                "open-meeting" => ui.global::<State>().set_view(1),
                _ => {}
            }
        }
    });
    ui.show().unwrap();
    window.set_size(slint::PhysicalSize::new(1280, 720));
    paint(&window);
    click(&ui, &window, 80.0, 232.0);
    assert_eq!(
        actions.borrow().as_slice(),
        [("navigate".into(), "2".into())]
    );
    assert_eq!(ui.global::<State>().get_view(), 2);
    click(&ui, &window, 80.0, 270.0);
    assert_eq!(
        actions.borrow().last().unwrap(),
        &("hub-tab".into(), "0".into())
    );
    assert_eq!(ui.global::<State>().get_view(), 8);
    click(&ui, &window, 80.0, 194.0);
    assert_eq!(actions.borrow().last().unwrap().0, "home");
    click(&ui, &window, 110.0, 102.0);
    assert_eq!(actions.borrow().last().unwrap().0, "new-recording");
    click(&ui, &window, 100.0, 500.0);
    assert_eq!(
        actions.borrow().last().unwrap(),
        &("open-meeting".into(), "fixture".into())
    );
    assert_eq!(actions.borrow().len(), 5);
    window.set_size(slint::PhysicalSize::new(760, 540));
    paint(&window);
    click(&ui, &window, 65.0, 28.0);
    assert!(ui.get_compact_navigation_open());
    let escape: slint::SharedString = slint::platform::Key::Escape.into();
    ui.window().dispatch_event(WindowEvent::KeyPressed {
        text: escape.clone(),
    });
    ui.window()
        .dispatch_event(WindowEvent::KeyReleased { text: escape });
    paint(&window);
    assert!(!ui.get_compact_navigation_open());
    // Escape returns focus to the toggle: Space reopens it without a pointer.
    ui.window()
        .dispatch_event(WindowEvent::KeyPressed { text: " ".into() });
    ui.window()
        .dispatch_event(WindowEvent::KeyReleased { text: " ".into() });
    paint(&window);
    assert!(ui.get_compact_navigation_open());
    click(&ui, &window, 80.0, 232.0);
    assert_eq!(ui.global::<State>().get_view(), 2);
    assert!(!ui.get_compact_navigation_open());
}
