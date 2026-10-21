//! An absent translation must not reserve half the transcript's reading width.
use overtalk_native::{AppWindow, LaneRow, State, TranscriptRow};
use slint::platform::{
    software_renderer::{MinimalSoftwareWindow, RepaintBufferType},
    Platform, PlatformError, WindowAdapter,
};
use slint::{ComponentHandle, ModelRc, Rgb8Pixel, VecModel};
use std::rc::Rc;

struct Headless(Rc<MinimalSoftwareWindow>);
impl Platform for Headless {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
        Ok(self.0.clone())
    }
}

#[test]
fn transcript_uses_the_reading_width_without_a_translation() {
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(Headless(window.clone()))).unwrap();
    let ui = AppWindow::new().unwrap();
    let state = ui.global::<State>();
    state.set_view(1);
    state.set_reduced_motion(true);
    state.set_lanes(ModelRc::from(Rc::new(VecModel::from(
        (0..5)
            .map(|i| LaneRow {
                id: format!("speaker-{i}").into(),
                name: format!("Speaker {}", i + 1).into(),
                ..Default::default()
            })
            .collect::<Vec<_>>(),
    ))));
    state.set_transcripts(ModelRc::from(Rc::new(VecModel::from(vec![TranscriptRow {
        id: "passage".into(), speaker: "Alex".into(), stamp: "00:01".into(),
        body: "This sentence should use the full reading width when there is no translation. The empty translation column must take no space and the words must wrap inside the panel. ".repeat(3).into(),
        ..Default::default()
    }]))));
    ui.show().unwrap();
    window.set_size(slint::PhysicalSize::new(1280, 720));
    slint::platform::update_timers_and_animations();
    let mut pixels = vec![Rgb8Pixel::default(); 1280 * 720];
    window.draw_if_needed(|renderer| {
        renderer.render(&mut pixels, 1280);
    });
    let ink_in_right_column = (525..690)
        .flat_map(|y| (1100..1230).map(move |x| (x, y)))
        .filter(|&(x, y)| {
            let p = pixels[y * 1280 + x];
            p.r == 232 && p.g == 236 && p.b == 240
        })
        .count();
    assert!(
        ink_in_right_column > 20,
        "Text was clipped or an empty translation reserved the right side of the reading pane"
    );
}
