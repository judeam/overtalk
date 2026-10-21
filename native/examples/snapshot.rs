//! Render the real native components at arbitrary sizes with synthetic data.
//! No display server, production database, audio devices or network required.
use overtalk_native::{notes, AppWindow, LaneRow, MeetingRow, ResultRow, State, TranscriptRow};
use slint::{
    platform::{
        software_renderer::{MinimalSoftwareWindow, RepaintBufferType},
        Platform, PlatformError, WindowAdapter,
    },
    ComponentHandle, Model, ModelRc, VecModel,
};
use std::{rc::Rc, time::Instant};
struct Headless {
    window: Rc<MinimalSoftwareWindow>,
}
impl Platform for Headless {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
        Ok(self.window.clone())
    }
}
fn rows<T: Clone + 'static>(values: Vec<T>) -> ModelRc<T> {
    Rc::new(VecModel::from(values)).into()
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arguments: Vec<_> = std::env::args().collect();
    let width = arguments
        .get(1)
        .and_then(|v| v.parse().ok())
        .unwrap_or(1280u32);
    let height = arguments
        .get(2)
        .and_then(|v| v.parse().ok())
        .unwrap_or(720u32);
    let view = arguments
        .get(3)
        .and_then(|v| v.parse().ok())
        .unwrap_or(1i32);
    let output = arguments
        .get(4)
        .cloned()
        .unwrap_or_else(|| "/tmp/overtalk-native-snapshot.png".into());
    let window = MinimalSoftwareWindow::new(RepaintBufferType::ReusedBuffer);
    slint::platform::set_platform(Box::new(Headless {
        window: window.clone(),
    }))?;
    let ui = AppWindow::new()?;
    let state = ui.global::<State>();
    state.set_view(view);
    state.set_engine_ready(true);
    state.set_title("Q4 Product Roadmap and Feature Prioritization".into());
    state.set_subtitle("14 Oct · 48 minutes · 3 speakers".into());
    state.set_meetings(rows(
        (0..30)
            .map(|i| MeetingRow {
                id: i.to_string().into(),
                title: [
                    "Release planning",
                    "Design review: onboarding",
                    "Customer feedback",
                ][i % 3]
                    .into(),
                subtitle: "14 Oct · 48:23".into(),
                pinned: i < 2,
            })
            .collect(),
    ));
    state.set_transcripts(rows(
        (0..2000)
            .map(|i| TranscriptRow {
                translation: Default::default(),
                id: i.to_string().into(),
                stamp: format!("{:02}:{:02}", i / 60, i % 60).into(),
                speaker: ["Lena", "Marco", "Priya"][i % 3].into(),
                body: [
                    "Let's agree on what we can deliver this month and what needs more time.",
                    "Search is the most requested improvement. We should make it fast, clear, and \
                     easy to use.",
                    "I will review the export changes and share an update with the team on Friday.",
                ][i % 3]
                    .into(),
                tone: slint::Color::from_rgb_u8(55, 193, 186),
                ink: slint::Color::from_rgb_u8(63, 198, 198),
            })
            .collect(),
    ));
    state.set_lanes(rows(
        ["Lena", "Marco", "Priya"]
            .iter()
            .map(|name| LaneRow {
                name: (*name).into(),
                tone: slint::Color::from_rgb_u8(55, 193, 186),
                waveform: Default::default(),
                ..Default::default()
            })
            .collect(),
    ));
    state.set_results(rows(
        (0..30)
            .map(|i| ResultRow {
                id: i.to_string().into(),
                title: "Q4 Product Roadmap".into(),
                subtitle: "14 October · Transcript · 02:10".into(),
                body: "We agreed to ship search first, then improve exports. Priya will confirm \
                       the timeline on Friday."
                    .into(),
                ..Default::default()
            })
            .collect(),
    ));
    state.set_mic_devices(rows(vec!["Default microphone".into()]));
    state.set_system_devices(rows(vec!["Default call audio".into()]));
    state.set_folder("Documents/Overtalk recordings".into());
    state.set_notes_preview(notes::preview(
        "# Meeting notes\n\n## Decisions\n\n- Ship search first.\n- Review export changes.\n\n## \
         Actions\n\n**Priya** will share an update on Friday.",
    ));
    if view == 8 {
        state.set_view(1);
        state.set_meeting_tab(1);
    }
    ui.show()?;
    window.set_size(slint::PhysicalSize::new(width, height));
    let mut pixels = vec![slint::Rgb8Pixel::default(); width as usize * height as usize];
    let start = Instant::now();
    for _ in 0..4 {
        slint::platform::update_timers_and_animations();
        window.draw_if_needed(|renderer| {
            renderer.render(&mut pixels, width as usize);
        });
    }
    let data: Vec<u8> = pixels.iter().flat_map(|p| [p.r, p.g, p.b]).collect();
    image::save_buffer(
        output,
        data.as_slice(),
        width,
        height,
        image::ColorType::Rgb8,
    )?;
    println!(
        "{width}x{height} view={view} rendered {} rows in {:.1} ms",
        state.get_transcripts().row_count(),
        start.elapsed().as_secs_f64() * 1000.0
    );
    Ok(())
}
