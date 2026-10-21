//! Opt-in timings contain no meeting content and are disabled in normal builds/runs.
use slint::ComponentHandle;
use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        LazyLock,
    },
    time::Instant,
};
static ENABLED: LazyLock<bool> =
    LazyLock::new(|| std::env::var_os("OVERTALK_TRACE_RENDER").is_some());
static START: LazyLock<Instant> = LazyLock::new(Instant::now);
static OPEN: AtomicU64 = AtomicU64::new(0);
pub(crate) fn meeting_opened() {
    if *ENABLED {
        OPEN.store(START.elapsed().as_nanos() as u64 + 1, Ordering::Release);
    }
}
pub(crate) fn install(ui: &crate::AppWindow) {
    if !*ENABLED {
        return;
    }
    let mut frame = Instant::now();
    let result = ui
        .window()
        .set_rendering_notifier(move |state, _| match state {
            slint::RenderingState::BeforeRendering => frame = Instant::now(),
            slint::RenderingState::AfterRendering => {
                let opened = OPEN.swap(0, Ordering::AcqRel);
                eprintln!(
                    "NATIVE_PERF render_cpu_ms={:.3}",
                    frame.elapsed().as_secs_f64() * 1000.0
                );
                if opened > 0 {
                    eprintln!(
                        "NATIVE_PERF open_first_frame_ms={:.3}",
                        (START.elapsed().as_nanos() as u64 + 1 - opened) as f64 / 1e6
                    );
                }
            }
            _ => {}
        });
    if let Err(error) = result {
        eprintln!("Native rendering timings unavailable: {error:?}");
    }
}
