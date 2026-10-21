//! Allocation-free level collection on audio callbacks; presentation samples it
//! separately. No locks, network calls or UI work are performed on audio threads.
use serde::Serialize;
use std::sync::{
    atomic::{AtomicU32, AtomicU64, Ordering},
    LazyLock,
};
struct Meter {
    peak: AtomicU32,
    last: AtomicU64,
}
static CLOCK: LazyLock<std::time::Instant> = LazyLock::new(std::time::Instant::now);
static MIC: Meter = Meter {
    peak: AtomicU32::new(0),
    last: AtomicU64::new(0),
};
static SYSTEM: Meter = Meter {
    peak: AtomicU32::new(0),
    last: AtomicU64::new(0),
};
pub fn collect(microphone: bool, samples: &[f32]) {
    let meter = if microphone { &MIC } else { &SYSTEM };
    let peak = samples
        .iter()
        .filter(|s| s.is_finite())
        .fold(0.0_f32, |a, s| a.max(s.abs()))
        .min(1.);
    meter.peak.fetch_max(peak.to_bits(), Ordering::Relaxed);
    meter
        .last
        .store(CLOCK.elapsed().as_millis() as u64 + 1, Ordering::Relaxed);
}
#[derive(Serialize)]
pub struct Levels {
    pub microphone: f32,
    pub system: f32,
    pub microphone_connected: bool,
    pub system_connected: bool,
    pub clipping: bool,
}
#[tauri::command]
pub fn workspace_recording_health() -> Levels {
    let now = CLOCK.elapsed().as_millis() as u64 + 1;
    let microphone = f32::from_bits(MIC.peak.swap(0, Ordering::Relaxed));
    let system = f32::from_bits(SYSTEM.peak.swap(0, Ordering::Relaxed));
    let connected = |m: &Meter| {
        let last = m.last.load(Ordering::Relaxed);
        last > 0 && now.saturating_sub(last) < 2000
    };
    Levels {
        microphone,
        system,
        microphone_connected: connected(&MIC),
        system_connected: connected(&SYSTEM),
        clipping: microphone >= 0.995 || system >= 0.995,
    }
}
