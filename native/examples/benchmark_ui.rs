//! Exercise the actual GPU window without desktop input automation or private data.
use overtalk_native::State;
use slint::{ComponentHandle, Model};
use std::{
    cell::Cell,
    rc::Rc,
    time::{Duration, Instant},
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var_os("OVERTALK_FIXTURE").is_none() || std::env::var_os("XDG_DATA_HOME").is_none()
    {
        return Err("Benchmark requires a synthetic fixture and an isolated profile".into());
    }
    let width = std::env::args()
        .nth(1)
        .and_then(|v| v.parse().ok())
        .unwrap_or(1920);
    let height = std::env::args()
        .nth(2)
        .and_then(|v| v.parse().ok())
        .unwrap_or(1080);
    let timer = Rc::new(slint::Timer::default());
    let retained = timer.clone();
    let started = Instant::now();
    let phase = Rc::new(Cell::new(0u32));
    let stable_size_since = Rc::new(Cell::new(None::<Instant>));
    overtalk_native::run_with_setup(move |ui| {
        ui.window()
            .set_size(slint::PhysicalSize::new(width, height));
        let weak = ui.as_weak();
        retained.start(
            slint::TimerMode::Repeated,
            Duration::from_millis(8),
            move || {
                let Some(ui) = weak.upgrade() else { return };
                let state = ui.global::<State>();
                let tick = phase.get();
                if tick == 0 {
                    if state.get_engine_ready() && state.get_meetings().row_count() > 0 {
                        let size = ui.window().size();
                        if size.width != width || size.height != height {
                            stable_size_since.set(None);
                            assert!(
                                started.elapsed() < Duration::from_secs(5),
                                "Window manager refused the requested benchmark size"
                            );
                            ui.window()
                                .set_size(slint::PhysicalSize::new(width, height));
                            return;
                        }
                        // Mapping and DPI negotiation may deliver a delayed
                        // resize. Do not time a frame at a transient geometry.
                        let settled = stable_size_since.get().unwrap_or_else(|| {
                            let now = Instant::now();
                            stable_size_since.set(Some(now));
                            now
                        });
                        if settled.elapsed() < Duration::from_millis(250) {
                            return;
                        }
                        eprintln!("NATIVE_BENCH_SIZE {}x{}", size.width, size.height);
                        state.invoke_action("open-meeting".into(), "m1".into());
                        phase.set(1);
                    }
                } else if tick < 250 {
                    state.set_playhead((tick % 100) as f32 / 100.0);
                    phase.set(tick + 1);
                    if tick.is_multiple_of(50) {
                        state.invoke_action("open-meeting".into(), "m1".into());
                    }
                } else if tick == 250 {
                    eprintln!("NATIVE_BENCH_IDLE_BEGIN");
                    phase.set(tick + 1);
                } else if tick < 500 {
                    phase.set(tick + 1);
                } else {
                    let size = ui.window().size();
                    eprintln!("NATIVE_BENCH_FINAL_SIZE {}x{}", size.width, size.height);
                    eprintln!("NATIVE_BENCH_COMPLETE");
                    let _ = slint::quit_event_loop();
                }
                if started.elapsed() > Duration::from_secs(30) {
                    let _ = slint::quit_event_loop();
                }
            },
        );
    })?;
    drop(timer);
    Ok(())
}
