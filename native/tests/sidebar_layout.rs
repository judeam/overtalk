//! Keep wrapped meeting titles and their dates inside the selectable row.
use overtalk_native::{AppWindow, MeetingRow, State};
use slint::{
    platform::{
        software_renderer::{MinimalSoftwareWindow, RepaintBufferType},
        Platform, PlatformError, WindowAdapter,
    },
    ComponentHandle, ModelRc, Rgb8Pixel, VecModel,
};
use std::rc::Rc;

struct Headless(Rc<MinimalSoftwareWindow>);
impl Platform for Headless {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
        Ok(self.0.clone())
    }
}

#[test]
fn long_meeting_titles_and_dates_stay_inside_the_selected_row() {
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(Headless(window.clone()))).unwrap();
    let ui = AppWindow::new().unwrap();
    let state = ui.global::<State>();
    state.set_engine_ready(true);
    state.set_view(1);
    state.set_selected_id("long-title".into());
    state.set_meetings(ModelRc::from(Rc::new(VecModel::from(vec![
        MeetingRow {
            id: "long-title".into(),
            title: "Meeting: Interconnected Participants and Walking Discussion with a much longer title that cannot fit in the sidebar".into(),
            subtitle: "14 Oct · 01:25".into(),
            pinned: false,
        },
        MeetingRow { id: "empty".into(), ..Default::default() },
    ]))));
    ui.show().unwrap();

    for (width, height, zoom) in [
        (1280, 720, 1.0),
        (1920, 1080, 1.5),
        (3840, 2160, 1.0),
        (7680, 4320, 1.0),
    ] {
        state.set_zoom(zoom);
        window.set_size(slint::PhysicalSize::new(width, height));
        slint::platform::update_timers_and_animations();
        let mut pixels = vec![Rgb8Pixel::default(); (width * height) as usize];
        window.draw_if_needed(|renderer| {
            renderer.render(&mut pixels, width as usize);
        });
        if let Some(directory) = std::env::var_os("OVERTALK_SIDEBAR_CAPTURE_DIR") {
            let directory = std::path::PathBuf::from(directory);
            std::fs::create_dir_all(&directory).unwrap();
            let data: Vec<_> = pixels.iter().flat_map(|p| [p.r, p.g, p.b]).collect();
            image::save_buffer(
                directory.join(format!("sidebar-{width}-{zoom}.png")),
                &data,
                width,
                height,
                image::ColorType::Rgb8,
            )
            .unwrap();
        }
        let scale = zoom * (14.0 + width as f32 * 0.0021).clamp(18.0, 24.0) / 16.0;
        let pixel = |x: u32, y: u32| pixels[(y * width + x) as usize];
        let is_color = |p: Rgb8Pixel, rgb: [u8; 3]| [p.r, p.g, p.b] == rgb;
        let selected_y: Vec<_> = (0..height)
            .filter(|&y| is_color(pixel((15.0 * scale) as u32, y), [31, 40, 51]))
            .collect();
        let top = *selected_y.first().expect("selected meeting row is visible");
        let bottom = *selected_y.last().unwrap();
        let mut title_pixels = 0;
        let mut date_pixels = 0;
        for y in top..(bottom + (65.0 * scale) as u32).min(height) {
            for x in (20.0 * scale) as u32..(180.0 * scale) as u32 {
                let p = pixel(x, y);
                let title = is_color(p, [232, 236, 240]);
                let date = is_color(p, [141, 151, 163]);
                title_pixels += usize::from(title);
                date_pixels += usize::from(date);
                assert!(
                    !(title || date) || y < bottom,
                    "meeting text paints beyond its selected row at {width}x{height}, zoom {zoom}: y={y}, row bottom={bottom}"
                );
            }
        }
        assert!(title_pixels > 20, "meeting title must remain visible");
        assert!(date_pixels > 4, "meeting date must remain visible");
    }
}
